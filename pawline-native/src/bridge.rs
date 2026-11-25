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
static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANCEL_UPTO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Esc while a picture is being made: the wait ends at once and a late result is thrown away.
pub fn request_cancel() {
    use std::sync::atomic::Ordering::SeqCst;
    CANCEL_UPTO.store(GEN.load(SeqCst), SeqCst);
}

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
        if let Operation::GenerateImage { provider, prompt } = op {
            // Only reached after the card naming this provider was confirmed.
            let dir = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| PawError::Exec("HOME is not set; can't pick the Pictures folder".into()))?
                .join("Pictures")
                .join("Pawline");
            use std::sync::atomic::Ordering::SeqCst;
            let my = GEN.fetch_add(1, SeqCst) + 1;
            let (provider, prompt) = (*provider, prompt.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            // The request runs on its own thread so Esc can end the wait now; a late
            // answer from that thread is dropped before anything is written.
            std::thread::spawn(move || {
                let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(120));
                let r = pawline_core::imagegen::generate(
                    &http,
                    provider,
                    &prompt,
                    &dir,
                    std::time::Duration::from_secs(4),
                    std::time::Duration::from_secs(240),
                    &|| CANCEL_UPTO.load(SeqCst) >= my,
                );
                let _ = tx.send(r);
            });
            let path = loop {
                if CANCEL_UPTO.load(SeqCst) >= my {
                    return Err(PawError::Exec("Cancelled. Nothing was saved, and a late answer will be discarded.".into()));
                }
                match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(r) => break r?,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Err(PawError::Exec("The picture request stopped unexpectedly; nothing was saved.".into())),
                }
            };
            return Ok(OpOutcome { op: op.clone(), detail: format!("Saved {}", path.display()) });
        }
        if let Operation::OpenApp { argv, .. } = op {
            // Only reached after the plan (with the exact command) was confirmed.
            let detail = pawline_core::apps::launch(argv)?;
            return Ok(OpOutcome { op: op.clone(), detail });
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
    /// Working time already spent on the pending plan (planning/validation), not counting any wait for the user.
    pub carried: std::time::Duration,
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


fn handle_text_inner(state: &mut UiState, input: &str) -> UiReply {
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    // Parser first. The 35 MB model is loaded only when the parser has a
    // gap, so ordinary commands never wait for it.
    let mut r = state.engine.handle_with(input, &backend, false, None);
    if r.is_err() {
        if !state.assist_tried {
            state.assist_tried = true;
            state.assist = NeedleAssist::embedded().ok();
        }
        if let Some(a) = state.assist.as_ref() {
            r = state.engine.handle_with(input, &backend, false, Some(a as &dyn pawline_core::assist::Assist));
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
                        | pawline_core::plan::Operation::GenerateImage { .. }
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
        Ok(Response::Failed { completed, failed, error, remaining }) => reply("failed", fail_text(completed.len(), &failed.to_string(), &error.to_string(), remaining.to_string())),
        Err(e) => reply("error", e.to_string()),
    }
}

/// A single step that failed with nothing else done reads as one plain sentence.
fn fail_text(done: usize, failed: &str, error: &str, remaining: String) -> String {
    let plain = error.strip_prefix("Execution failed: ").unwrap_or(error);
    if done == 0 && remaining == "0" {
        return plain.to_string();
    }
    format!("Stopped partway. Completed {done} step(s). Failed: {failed} ({error}). Not attempted: {remaining}.")
}

/// Put any journal recovery findings in front of the reply, once.
pub fn with_notes(state: &mut UiState, mut r: UiReply) -> UiReply {
    if !state.notes.is_empty() {
        let n = state.notes.drain(..).collect::<Vec<_>>().join("\n");
        r.text = format!("After an interrupted run I checked the undo journal:\n{n}\n\n{}", r.text);
    }
    r
}

fn confirm_exec(state: &mut UiState) -> UiReply {
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
                        | pawline_core::plan::Operation::GenerateImage { .. }
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
        Ok(Response::Failed { completed, failed, error, remaining }) => reply("failed", fail_text(completed.len(), &failed.to_string(), &error.to_string(), remaining.to_string())),
        _ => reply("error", "unexpected state".into()),
    }
}


pub fn new_state(root: std::path::PathBuf) -> UiState {
    let notes = startup_notes_from(&root);
    UiState {
        carried: std::time::Duration::ZERO,
        engine: Engine::new(root.clone()),
        root,
        pending: None,
        assist: None,
        assist_tried: false,
        notes,
    }
}

pub fn cancel(state: &mut UiState) -> UiReply {
    state.carried = std::time::Duration::ZERO;
    state.pending = None;
    reply("message", "Cancelled. Nothing was done.".into())
}

/// Seconds shown to the user: rounded UP to one decimal ("1.21" -> "1.3"), at least 0.1.
pub fn took(d: std::time::Duration) -> String {
    let tenths = ((d.as_micros() + 99_999) / 100_000).max(1);
    format!("{}.{}s", tenths / 10, tenths % 10)
}

/// Add "Took N.Ns" to a finished result. A plan waiting for confirmation is not a result, and the
/// time the user spends deciding is never included (the timer for a confirmed run starts at confirm).
fn timed(mut r: UiReply, d: std::time::Duration) -> UiReply {
    if matches!(r.kind.as_str(), "executed" | "failed" | "message" | "error") {
        r.text = format!("{}\nTook {}", r.text, took(d));
    }
    r
}

/// Request submitted -> result ready. Planning time before a confirmation is carried over and
/// added to the execution time after it; the wait for the user is never counted.
pub fn handle_text(state: &mut UiState, input: &str) -> UiReply {
    state.carried = std::time::Duration::ZERO;
    let t = std::time::Instant::now();
    let r = handle_text_inner(state, input);
    finish(state, r, t.elapsed())
}

/// Confirmation received -> result ready: carried planning time plus execution.
pub fn confirm_inner(state: &mut UiState) -> UiReply {
    let t = std::time::Instant::now();
    let r = confirm_exec(state);
    finish(state, r, t.elapsed())
}

fn finish(state: &mut UiState, r: UiReply, spent: std::time::Duration) -> UiReply {
    if r.kind == "confirm" {
        state.carried += spent;
        return r;
    }
    let total = state.carried + spent;
    state.carried = std::time::Duration::ZERO;
    timed(r, total)
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use std::time::Duration as D;
    #[test]
    fn rounds_up_to_one_decimal() {
        assert_eq!(took(D::from_millis(1210)), "1.3s");
        assert_eq!(took(D::from_millis(1200)), "1.2s");
        assert_eq!(took(D::from_millis(1)), "0.1s");
        assert_eq!(took(D::from_millis(0)), "0.1s");
        assert_eq!(took(D::from_millis(9951)), "10.0s");
        assert_eq!(took(D::from_secs(61)), "61.0s");
    }
    #[test]
    fn confirm_wait_is_not_counted_and_plans_carry_no_time() {
        let plan = reply("confirm", "p".into());
        assert_eq!(timed(plan, D::from_secs(3)).text, "p");
        let done = timed(reply("executed", "Done".into()), D::from_millis(50));
        assert_eq!(done.text, "Done\nTook 0.1s");
    }
    #[test]
    fn planning_plus_execution_excludes_wait() {
        let mut st = new_state(std::env::temp_dir());
        // planning 0.21s, then the user waits 10s (never measured), then execution 0.31s
        let plan = finish(&mut st, reply("confirm", "p".into()), D::from_millis(210));
        assert_eq!(plan.text, "p");
        let done = finish(&mut st, reply("executed", "Done".into()), D::from_millis(310));
        assert_eq!(done.text, "Done\nTook 0.6s");
        assert_eq!(st.carried, D::ZERO);
        // a later request does not inherit it
        let again = finish(&mut st, reply("executed", "Done".into()), D::from_millis(100));
        assert_eq!(again.text, "Done\nTook 0.1s");
    }
}
