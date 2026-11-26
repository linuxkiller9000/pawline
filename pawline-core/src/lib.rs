//! Pawline core: deterministic, offline command understanding and safe execution.
//!
//! Pipeline: parse (intent) -> resolve + validate (validate) -> Plan -> execute (exec).
//! The engine needs no model, no network and no Python. An optional embedded
//! assist (Needle3 via the Cactus runtime) may propose operations for inputs
//! this parser cannot handle; those proposals re-enter at validation and never
//! bypass it.

pub mod apps;
pub mod arabic;
pub mod weather;
pub mod assist;
pub mod chat;
pub mod context;
pub mod deck;
pub mod dirs;
pub mod error;
pub mod exec;
pub mod filesgate;
pub mod finder;
pub mod imagegen;
pub mod intent;
pub mod journal;
pub mod math;
pub mod netgate;
pub mod organize;
pub mod plan;
pub mod platform;
pub mod planmem;
pub mod search;
pub mod shrink;
pub mod searchmem;
pub mod textfallback;
pub mod typo;
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

    /// The folder requests mean when they name no place (the "folder pill"). `None` goes back to the home folder.
    /// A folder outside the home area or one that is gone is refused and the old setting is cleared.
    pub fn set_working_dir(&mut self, dir: Option<&std::path::Path>) -> Result<(), PawError> {
        let r = self.validator.set_work_dir(dir);
        if r.is_err() {
            let _ = self.validator.set_work_dir(None);
        }
        r
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
                    // The assist's own failure text (confidence numbers, empty proposals) is
                    // internal detail; the user sees the parser's plain explanation only.
                    // A model that panics is a model failure like any other: no proposal, the parser's explanation, nothing runs.
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.propose(input)))
                        .unwrap_or_else(|_| Err(PawError::Parse("assist panicked".into())))
                        .map_err(|_assist_err| match &parse_err {
                            PawError::Parse(m) => PawError::Parse(m.clone()),
                            other => PawError::Parse(other.to_string()),
                        })?,
                    true,
                ),
                None => return Err(parse_err),
            },
        };
        if from_assist && directives.is_empty() {
            // An empty proposal is a failed proposal: never show an empty confirm card.
            return Err(PawError::Parse(match intent::parse(input) {
                Err(PawError::Parse(m)) => m,
                Err(other) => other.to_string(),
                Ok(_) => "I did not understand that.".into(),
            }));
        }
        self.validator.literal_names.set(input.contains(['"', '\u{201c}', '\u{201d}', '`']));
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
    /// Folders (ranked, existing) that may be used to find a destination named in words. Replaced per request.
    /// Next requests always wait for a yes (they came from a chooser pick).
    pub fn set_force_confirm(&mut self, on: bool) {
        self.validator.force_confirm.set(on);
    }

    /// A folder named in words (home child, session folder, or a remembered one), confined to the home area.
    pub fn resolve_place(&self, words: &str) -> Result<Option<std::path::PathBuf>, PawError> {
        let direct = self.ctx.resolve_named(words).or_else(|| Some(self.validator.root().join(words)).filter(|p| p.is_dir()));
        if let Some(p) = direct.and_then(|p| std::fs::canonicalize(p).ok()).filter(|p| p.is_dir() && p.starts_with(self.validator.root())) {
            return Ok(Some(p));
        }
        // "downloads" for ~/Downloads: a folder directly in home, ignoring case; several that differ only by case: ask
        let want = words.trim().to_lowercase();
        let root = self.validator.root();
        let mut same: Vec<std::path::PathBuf> = std::fs::read_dir(root)
            .map(|rd| rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false) && e.file_name().to_string_lossy().to_lowercase() == want).map(|e| e.path()).collect())
            .unwrap_or_default();
        same.sort();
        match same.len() {
            1 => return Ok(std::fs::canonicalize(&same[0]).ok().filter(|p| p.starts_with(root))),
            n if n > 1 => return Err(PawError::Ambiguous { words: words.trim().to_string(), choices: same }),
            _ => {}
        }
        self.validator.find_place_pub(words, &self.ctx)
    }

    pub fn set_dir_hints(&mut self, hints: Vec<std::path::PathBuf>) {
        self.validator.dir_hints = hints;
    }

    pub fn handle_with(
        &mut self,
        input: &str,
        backend: &dyn ExecutionBackend,
        confirmed: bool,
        assist: Option<&dyn Assist>,
    ) -> Result<Response, PawError> {
        let plan = self.plan_with(input, assist)?;
        // Read-only mode (item 14): a plan that changes files never reaches a card
        // (a card would promise a change) and never runs. The refusal is a plain message.
        if filesgate::check_ops(&plan.ops).is_err() {
            return Ok(Response::Message(filesgate::READ_ONLY_MESSAGE.to_string()));
        }
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
        // Defense in depth: a card confirmed before read-only mode was switched on still writes nothing.
        if filesgate::check_ops(&plan.ops).is_err() {
            return Ok(Response::Message(filesgate::READ_ONLY_MESSAGE.to_string()));
        }
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
