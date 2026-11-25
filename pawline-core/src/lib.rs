//! Pawline core: deterministic, offline command understanding and safe execution.
//!
//! Pipeline: parse (intent) -> resolve + validate (validate) -> Plan -> execute (exec).
//! The engine needs no model, no network and no Python. An optional embedded
//! assist (Needle3 via the Cactus runtime) may propose operations for inputs
//! this parser cannot handle; those proposals re-enter at validation and never
//! bypass it.

pub mod assist;
pub mod context;
pub mod error;
pub mod exec;
pub mod intent;
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
        let plan = self.plan_with(input, assist)?;
        if plan.needs_confirmation && !confirmed {
            return Ok(Response::Confirm(plan));
        }
        self.execute_plan(plan, backend)
    }

    /// Execute a previously shown plan after the user confirmed.
    pub fn execute_plan(
        &mut self,
        plan: Plan,
        backend: &dyn ExecutionBackend,
    ) -> Result<Response, PawError> {
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
