//! Pawline core: deterministic, offline command understanding and safe execution.
//!
//! Pipeline: parse (intent) -> resolve + validate (validate) -> Plan -> execute (exec).
//! The engine needs no model, no network and no Python. An optional embedded
//! assist (Needle3 via the Cactus runtime) may propose operations for inputs
//! this parser cannot handle; those proposals re-enter at validation and never
//! bypass it.

pub mod actions;
pub mod assist;
pub mod context;
pub mod error;
pub mod exec;
pub mod intent;
pub mod journal;
pub mod math;
pub mod plan;
pub mod search;
pub mod validate;
pub mod wallpaper;

use assist::Assist;
use context::ContextManager;
use error::PawError;
use exec::{ExecutionBackend, OpOutcome};
use plan::Plan;
use validate::Validator;

pub struct Engine {
    validator: Validator,
    ctx: ContextManager,
    last_candidate: Option<actions::Candidate>,
}

/// What the UI shows after handling one message.
pub enum Response {
    /// Ran everything; here is what happened.
    Executed(Vec<OpOutcome>),
    /// The plan needs a yes first (destructive or hard to undo).
    Confirm(Plan),
    /// Nothing to do; a helpful message.
    Message(String),
    /// An operation failed mid-plan: what completed, what failed, what was
    /// never attempted. Context was only updated for completed operations.
    Failed {
        completed: Vec<OpOutcome>,
        failed: String,
        error: String,
        remaining: usize,
    },
}

impl Engine {
    pub fn new(root: std::path::PathBuf) -> Self {
        Engine {
            validator: Validator::new(root),
            ctx: ContextManager::new(),
            last_candidate: None,
        }
    }

    pub fn context(&self) -> &ContextManager {
        &self.ctx
    }

    /// Parse + validate only: what Pawline would do, before doing it.
    pub fn plan(&self, input: &str) -> Result<Plan, PawError> {
        self.plan_with(input, None)
    }

    /// Parse + validate, optionally consulting an assist for inputs the
    /// deterministic parser refuses. Assist proposals are validated exactly
    /// like parser output.
    pub fn plan_with(&self, input: &str, assist: Option<&dyn Assist>) -> Result<Plan, PawError> {
        let (directives, from_assist) = match intent::parse(input) {
            Ok(d) => (d, false),
            Err(parse_err) => match assist {
                Some(a) => (
                    a.propose(input).map_err(|assist_err| {
                        let inner = match &parse_err {
                            PawError::Parse(m) => m.clone(),
                            other => other.to_string(),
                        };
                        PawError::Parse(format!("{inner} (assist also failed: {assist_err})"))
                    })?,
                    true,
                ),
                None => return Err(parse_err),
            },
        };
        let mut plan = self.validator.build_plan(&directives, &self.ctx)?;
        if from_assist {
            // Anything the model proposed is shown to the user and waits for
            // an explicit yes, including plain creates.
            plan.assist_derived = true;
            plan.needs_confirmation = true;
            plan.confirmation_reasons
                .push("proposed by the AI assist, not the deterministic parser".into());
        }
        Ok(plan)
    }

    /// Full pipeline. Refuses to run a confirmation-gated plan without
    /// `confirmed == true`.
    pub fn handle(
        &mut self,
        input: &str,
        backend: &dyn ExecutionBackend,
        confirmed: bool,
    ) -> Result<Response, PawError> {
        self.handle_with(input, backend, confirmed, None)
    }

    /// Full pipeline with an optional assist; see `plan_with`.
    pub fn handle_with(
        &mut self,
        input: &str,
        backend: &dyn ExecutionBackend,
        confirmed: bool,
        assist: Option<&dyn Assist>,
    ) -> Result<Response, PawError> {
        self.last_candidate = None;
        let plan = self.plan_with(input, assist)?;
        if plan.needs_confirmation && !confirmed {
            return Ok(Response::Confirm(plan));
        }
        let candidate = self.candidate_for(input, &plan);
        let r = self.execute_plan(plan, backend)?;
        // "save that as NAME" refers to the last command that fully ran.
        // Anything else (failed, assist-derived, not saveable) clears it, so
        // it can never save an older command by mistake.
        self.last_candidate = match &r {
            Response::Executed(_) => candidate,
            _ => None,
        };
        Ok(r)
    }

    fn candidate_for(&self, input: &str, plan: &Plan) -> Option<actions::Candidate> {
        if plan.assist_derived {
            return None;
        }
        let directives = intent::parse(input).ok()?;
        actions::saveable(&directives, &plan.ops).ok()?;
        Some(actions::Candidate { command: input.trim().to_string(), steps: self.step_lines(plan) })
    }

    fn step_lines(&self, plan: &Plan) -> Vec<String> {
        plan.ops.iter().map(|o| o.describe()).collect()
    }

    /// The command "save that as NAME" would save, if there is one.
    pub fn save_candidate(&self) -> Option<&actions::Candidate> {
        self.last_candidate.as_ref()
    }

    /// Why the last command cannot be saved (for a helpful message).
    pub fn why_not_saveable(&self, input: &str) -> String {
        match intent::parse(input) {
            Ok(d) => match self.validator.build_plan(&d, &self.ctx) {
                Ok(p) => actions::saveable(&d, &p.ops).err().unwrap_or_else(|| "it has not run".into()),
                Err(_) => "it does not currently check out".into(),
            },
            Err(_) => "it was not a command Pawline's fixed parser understands".into(),
        }
    }

    /// Re-plan a saved action. The AI assist is never used. Any difference
    /// from the saved steps refuses the run and shows both. A matching plan
    /// always needs the user's explicit confirmation, every run.
    pub fn plan_saved(&self, saved: &actions::Saved) -> Result<Plan, PawError> {
        let mut plan = self.plan_with(&saved.command, None)?;
        let now = self.step_lines(&plan);
        if now != saved.steps {
            return Err(PawError::Validation(format!(
                "saved action \"{}\" would now do something different from when you saved it, so it was not run.\nSaved:\n{}\nNow:\n{}\nIf the new plan is what you want, run the command by hand and save it under a new name.",
                saved.name,
                saved.steps.iter().map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n"),
                now.iter().map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n"),
            )));
        }
        plan.needs_confirmation = true;
        plan.confirmation_reasons.push(format!(
            "saved action \"{}\": re-checked just now against the disk, same steps as when saved",
            saved.name
        ));
        Ok(plan)
    }

    /// Execute a previously shown plan after the user confirmed.
    pub fn execute_plan(
        &mut self,
        plan: Plan,
        backend: &dyn ExecutionBackend,
    ) -> Result<Response, PawError> {
        self.last_candidate = None;
        let total = plan.ops.len();
        let mut outcomes = Vec::new();
        for op in &plan.ops {
            match backend.execute(op) {
                Ok(outcome) => {
                    // update context only as things actually happen
                    match op {
                        plan::Operation::CreateDir { path }
                        | plan::Operation::CreateFile { path } => self.ctx.remember(path),
                        plan::Operation::Rename { to, .. }
                        | plan::Operation::Move { to, .. }
                        | plan::Operation::Copy { to, .. } => self.ctx.remember(to),
                        _ => {}
                    }
                    outcomes.push(outcome);
                }
                Err(e) => {
                    return Ok(Response::Failed {
                        remaining: total - outcomes.len() - 1,
                        failed: op.describe(),
                        error: e.to_string(),
                        completed: outcomes,
                    });
                }
            }
        }
        Ok(Response::Executed(outcomes))
    }
}
