//! Core bridge: runs Pawline's engine (parser, validator, journal, optional
//! Needle assist). UI independent; lives on its own thread.
use pawline_core::plan::Plan;
use pawline_core::{Engine, Response};
use pawline_core::exec::{DirectFsBackend, ExecutionBackend, OpOutcome};
use pawline_core::error::PawError;
use pawline_core::plan::{Operation, SearchEngine};
use pawline_needle::assist::NeedleAssist;
/// UI backend: confined fs execution plus the explicit online search the
/// user asked for by name.
struct UiBackend {
    inner: DirectFsBackend,
}

impl ExecutionBackend for UiBackend {
    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError> {
        if let Operation::Search { engine, query } = op {
            let text = pawline_core::search::run(*engine, query)?;
            return Ok(OpOutcome {
                op: op.clone(),
                detail: text,
            });
        }
        self.inner.execute(op)
    }
    fn name(&self) -> &'static str {
        "ui"
    }
    fn search(&self, engine: SearchEngine, query: &str) -> Result<String, PawError> {
        pawline_core::search::run(engine, query)
    }
}

/// Engine state owned by the worker thread.
pub struct UiState {
    pub engine: Engine,
    pub root: std::path::PathBuf,
    pub pending: Option<Plan>,
    pub assist: Option<NeedleAssist>,
    pub assist_tried: bool,
    /// Journal recovery findings not yet shown to the user.
    pub notes: Vec<String>,
}

pub fn startup_notes_from(root: &std::path::Path) -> Vec<String> {
    DirectFsBackend::new(root).map(|b| b.recovery_notes().to_vec()).unwrap_or_default()
}

pub struct UiReply {
    pub kind: String,
    pub text: String,
    pub plan: Option<String>,
}

pub fn reply(kind: &str, text: String) -> UiReply {
    UiReply { kind: kind.into(), text, plan: None }
}


pub fn handle_text(state: &mut UiState, input: &str) -> UiReply {
    handle_text_from(state, input, false)
}

/// `from_voice`: the text came from speech recognition. Every plan that does
/// anything then needs a fresh explicit confirmation, saved-action commands
/// cannot be dictated, and the words are never trusted as typed.
pub fn handle_text_from(state: &mut UiState, input: &str, from_voice: bool) -> UiReply {
    const VOICE_WHY: &str = "this text came from speech recognition, so check the words before you say yes";
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    if let Some(m) = pawline_core::actions::parse_meta(input) {
        if from_voice {
            return reply("message", "Saved-action commands can't be dictated. Type them instead.".into());
        }
        return handle_meta(state, &backend.inner, m);
    }
    // Parser first. The 35 MB model is loaded only when the parser has a
    // gap, so ordinary commands never wait for it.
    let why = if from_voice { Some(VOICE_WHY) } else { None };
    let mut r = state.engine.handle_with_force(input, &backend, false, None, why);
    if r.is_err() {
        if !state.assist_tried {
            state.assist_tried = true;
            state.assist = NeedleAssist::embedded().ok();
        }
        if let Some(a) = state.assist.as_ref() {
            r = state.engine.handle_with_force(input, &backend, false, Some(a as &dyn pawline_core::assist::Assist), why);
        }
    }
    match r {
        Ok(Response::Executed(ops)) => {
            let done: Vec<String> = ops.iter().map(|o| o.op.describe()).collect();
            let body = if ops.len() == 1
                && matches!(ops[0].op, pawline_core::plan::Operation::Answer { .. })
            {
                done.join("\n")
            } else {
                let lines: Vec<String> = ops
                    .iter()
                    .map(|o| match &o.op {
                        pawline_core::plan::Operation::Search { .. }
                        | pawline_core::plan::Operation::List { .. } => {
                            format!("{}\n{}", o.op.describe(), o.detail)
                        }
                        _ => o.op.describe(),
                    })
                    .collect();
                format!("Done:\n{}", lines.join("\n"))
            };
            reply("executed", body)
        }
        Ok(Response::Confirm(plan)) => {
            let summary = plan.summary();
            let why = if plan.assist_derived {
                "The AI assist proposed this. It only runs if you say so:"
            } else {
                "This needs your confirmation:"
            };
            let r = UiReply {
                kind: "confirm".into(),
                text: format!("{why}\n{summary}"),
                plan: Some(summary),
            };
            state.pending = Some(plan);
            r
        }
        Ok(Response::Message(m)) => reply("message", m),
        Ok(Response::Failed { completed, failed, error, remaining }) => reply(
            "failed",
            format!(
                "Stopped partway. Completed {} step(s). Failed: {failed} ({error}). Not attempted: {remaining}.",
                completed.len()
            ),
        ),
        Err(e) => reply("error", e.to_string()),
    }
}

/// Put any journal recovery findings in front of the reply, once.
pub fn with_notes(state: &mut UiState, mut r: UiReply) -> UiReply {
    if !state.notes.is_empty() {
        let n = state.notes.drain(..).collect::<Vec<_>>().join("\n");
        r.text = format!("After an interrupted run I checked the undo journal:\n{n}\n\n{}", r.text);
    }
    r
}

pub fn confirm_inner(state: &mut UiState) -> UiReply {
    let plan = match state.pending.take() {
        Some(p) => p,
        None => return reply("error", "Nothing waiting for confirmation.".into()),
    };
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    match state.engine.execute_plan(plan, &backend) {
        Ok(Response::Executed(ops)) => {
            let done: Vec<String> = ops.iter().map(|o| o.op.describe()).collect();
            let body = if ops.len() == 1
                && matches!(ops[0].op, pawline_core::plan::Operation::Answer { .. })
            {
                done.join("\n")
            } else {
                let lines: Vec<String> = ops
                    .iter()
                    .map(|o| match &o.op {
                        pawline_core::plan::Operation::Search { .. }
                        | pawline_core::plan::Operation::List { .. } => {
                            format!("{}\n{}", o.op.describe(), o.detail)
                        }
                        _ => o.op.describe(),
                    })
                    .collect();
                format!("Done:\n{}", lines.join("\n"))
            };
            reply("executed", body)
        }
        Ok(Response::Failed { completed, failed, error, remaining }) => reply(
            "failed",
            format!("Stopped partway. Completed {}. Failed: {failed} ({error}). Not attempted: {remaining}.", completed.len()),
        ),
        _ => reply("error", "unexpected state".into()),
    }
}


pub fn new_state(root: std::path::PathBuf) -> UiState {
    let notes = startup_notes_from(&root);
    UiState {
        engine: Engine::new(root.clone()),
        root,
        pending: None,
        assist: None,
        assist_tried: false,
        notes,
    }
}

pub fn cancel(state: &mut UiState) -> UiReply {
    state.pending = None;
    reply("message", "Cancelled. Nothing was done.".into())
}

/// Saved-action commands. Same engine and same confirmation as everything
/// else; the assist is never used here.
fn handle_meta(state: &mut UiState, b: &DirectFsBackend, m: pawline_core::actions::Meta) -> UiReply {
    use pawline_core::actions::{self, Meta};
    let mut all = match b.read_store_text(actions::ACTIONS) {
        Ok(t) => actions::parse_file(&t),
        Err(e) => return reply("error", format!("could not read saved actions: {e}")),
    };
    let write = |all: &Vec<actions::Saved>| b.write_store_text(actions::ACTIONS, &actions::render_file(all));
    match m {
        Meta::Save(name) => {
            let c = match state.engine.save_candidate() {
                Some(c) => c.clone(),
                None => {
                    return reply(
                        "error",
                        "Nothing to save. \"save that as NAME\" saves the last command that ran, and for now only plain make-folder, make-file and copy commands with full paths can be saved. Run one of those, then save it right away.".into(),
                    )
                }
            };
            if let Err(e) = actions::add(&mut all, &name, &c) {
                return reply("error", e.to_string());
            }
            match write(&all) {
                Ok(()) => reply(
                    "message",
                    format!(
                        "Saved \"{name}\":\n{}\nRun it with: run saved action {name}. It will be re-checked and ask for your confirmation every time.",
                        actions::describe_saved(all.last().unwrap())
                    ),
                ),
                Err(e) => reply("error", format!("could not save: {e}")),
            }
        }
        Meta::List => {
            if all.is_empty() {
                reply("message", "No saved actions.".into())
            } else {
                let names: Vec<String> = all.iter().map(|a| format!("{}: {}", a.name, a.command)).collect();
                reply("message", format!("Saved actions:\n{}", names.join("\n")))
            }
        }
        Meta::Show(name) => match actions::find(&all, &name) {
            Some(a) => reply("message", actions::describe_saved(a)),
            None => reply("error", format!("no saved action named \"{name}\"")),
        },
        Meta::Delete(name) => match actions::remove(&mut all, &name) {
            Ok(()) => match write(&all) {
                Ok(()) => reply("message", format!("Deleted the saved action \"{name}\". Files it made are untouched.")),
                Err(e) => reply("error", format!("could not update saved actions: {e}")),
            },
            Err(e) => reply("error", e.to_string()),
        },
        Meta::Run(name) => {
            let saved = match actions::find(&all, &name) {
                Some(a) => a.clone(),
                None => return reply("error", format!("no saved action named \"{name}\"")),
            };
            match state.engine.plan_saved(&saved) {
                Ok(plan) => {
                    let summary = plan.summary();
                    let r = UiReply {
                        kind: "confirm".into(),
                        text: format!(
                            "Saved action \"{}\" was re-checked just now and gives the same steps as when you saved it. It only runs if you say so:\n{summary}",
                            saved.name
                        ),
                        plan: Some(summary),
                    };
                    state.pending = Some(plan);
                    r
                }
                Err(e) => reply("error", e.to_string()),
            }
        }
    }
}
