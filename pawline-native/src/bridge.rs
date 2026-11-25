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

/// Run one provider request on its own thread so Esc can end the wait now.
/// Cancel and commit (write/return) arbitrate on one atomic: exactly one wins, and the
/// provider slot stays held until the thread really ends, even if abandoned.
fn run_cancellable<T: Send + 'static>(
    provider: pawline_core::imagegen::ImageProvider,
    job: impl FnOnce(&dyn Fn() -> bool, &dyn Fn() -> bool) -> Result<T, PawError> + Send + 'static,
) -> Result<T, PawError> {
    use std::sync::atomic::{AtomicU8, Ordering::SeqCst};
    use std::sync::Arc;
    let ticket = pawline_core::imagegen::GUARD.begin(provider, std::time::Instant::now())?;
    let my = GEN.fetch_add(1, SeqCst) + 1;
    // 0 running, 1 cancelled, 2 committed.
    let state = Arc::new(AtomicU8::new(0));
    let (tx, rx) = std::sync::mpsc::channel();
    let st = state.clone();
    std::thread::spawn(move || {
        let _ticket = ticket;
        let r = job(&|| st.load(SeqCst) == 1, &|| st.compare_exchange(0, 2, SeqCst, SeqCst).is_ok());
        let _ = tx.send(r);
    });
    loop {
        if CANCEL_UPTO.load(SeqCst) >= my && state.compare_exchange(0, 1, SeqCst, SeqCst).is_ok() {
            return Err(PawError::Exec("Cancelled. Nothing was saved, and a late answer will be discarded.".into()));
        }
        // If the commit already happened, cancel lost the race: the real result is reported below.
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(r) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(PawError::Exec("The online request stopped unexpectedly; nothing was kept.".into())),
        }
    }
}

fn ask_online(text: &str) -> Result<String, PawError> {
    let q = text.to_string();
    // Same Pollinations slot as pictures: one request at a time, 45 s apart.
    run_cancellable(pawline_core::imagegen::ImageProvider::Pollinations, move |cancelled, commit| {
        let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(60));
        pawline_core::textfallback::ask(&http, &q, cancelled, commit)
    })
    .map(|a| format!("Pollinations (online, not checked, may be wrong):\n{a}"))
}

static FORCE_REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Explicit search with a small local memory of past answers (see pawline_core::searchmem).
/// A fresh saved answer is shown labelled with its date. "refresh: <request>" fetches again.
/// If the source can't be reached, a saved answer (any age) is shown and labelled as such.
fn cached_search(engine: SearchEngine, query: &str) -> Result<String, PawError> {
    use pawline_core::searchmem as sm;
    let Some(path) = sm::default_path() else { return pawline_core::search::run(engine, query) };
    let now = sm::now_secs();
    let force = FORCE_REFRESH.swap(false, std::sync::atomic::Ordering::SeqCst);
    sm::answer(&path, engine, query, now, force, || pawline_core::search::run(engine, query))
}

struct UiBackend {
    inner: DirectFsBackend,
}

impl ExecutionBackend for UiBackend {
    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError> {
        if let Operation::Search { engine, query } = op {
            let text = cached_search(*engine, query)?;
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
            let (provider, prompt) = (*provider, prompt.clone());
            let path = run_cancellable(provider, move |cancelled, commit| {
                let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(120));
                pawline_core::imagegen::generate(
                    &http,
                    provider,
                    &prompt,
                    &dir,
                    std::time::Duration::from_secs(4),
                    std::time::Duration::from_secs(240),
                    cancelled,
                    commit,
                )
            })?;
            return Ok(OpOutcome { op: op.clone(), detail: format!("Saved {}", path.display()) });
        }
        if let Operation::AskOnline { text } = op {
            return Ok(OpOutcome { op: op.clone(), detail: ask_online(text)? });
        }
        if let Operation::Shrink { from, to } = op {
            // Only reached after the card was confirmed. Esc/Cancel ends ffmpeg and removes the temporary file.
            use std::sync::atomic::Ordering::SeqCst;
            let my = GEN.fetch_add(1, SeqCst) + 1;
            let detail = self.inner.shrink(from, to, &|| CANCEL_UPTO.load(SeqCst) >= my)?;
            return Ok(OpOutcome { op: op.clone(), detail });
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
        cached_search(engine, query)
    }
}

/// Engine state owned by the worker thread.
pub struct UiState {
    pub engine: Engine,
    pub root: std::path::PathBuf,
    pub pending: Option<Plan>,
    /// Request wording of the pending plan, kept in memory only to build the lookup key if the plan completes. Never written to disk.
    pub pending_request: Option<String>,
    pub assist: Option<NeedleAssist>,
    pub assist_tried: bool,
    /// Journal recovery findings not yet shown to the user.
    pub notes: Vec<String>,
    /// Working time already spent on the pending plan (planning/validation), not counting any wait for the user.
    pub carried: std::time::Duration,
    /// Commands a chooser pick may have filled in; the next input equal to one of them always shows a confirmation.
    pub picks: Vec<String>,
    /// Steps of an organize plan waiting for a yes, and of the last organize that ran (for "undo organize"). Memory only.
    pub organize_pending: Option<usize>,
    pub undo_batch: Option<usize>,
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


/// "zi" / "zi <keywords>": list frequent folders (zoxide if present, else Pawline's own list). Display only.
pub fn dirs_command(
    input: &str,
    own: Option<&std::path::Path>,
    home: &std::path::Path,
    now: u64,
    zoxide: impl FnOnce() -> Option<Vec<pawline_core::dirs::Ranked>>,
) -> Option<String> {
    dirs_command_rows(input, own, home, now, zoxide).map(|r| r.0)
}

/// Same as dirs_command, plus the folders inside the home area (the ones a click may use), for a chooser.
pub fn dirs_command_rows(
    input: &str,
    own: Option<&std::path::Path>,
    home: &std::path::Path,
    now: u64,
    zoxide: impl FnOnce() -> Option<Vec<pawline_core::dirs::Ranked>>,
) -> Option<(String, Vec<String>)> {
    let t = input.trim().trim_end_matches(['.', '!']);
    let low = t.to_lowercase();
    if matches!(low.as_str(), "folders memory" | "show folders memory" | "folder memory" | "forget folders" | "clear folders") {
        let Some(own) = own else { return Some(("No data folder.".into(), vec![])) };
        return Some((
            if low.starts_with("forget") || low.starts_with("clear") {
                format!("Deleted {} folder(s) from Pawline's own list. zoxide was not touched.", pawline_core::dirs::forget_own(own))
            } else {
                pawline_core::dirs::describe_own(own)
            },
            vec![],
        ));
    }
    let query = if low == "zi" { "" } else { low.strip_prefix("zi ")? };
    let Some(own) = own else { return Some(("No data folder.".into(), vec![])) };
    let (src, list) = pawline_core::dirs::ranked(zoxide(), own, now, query);
    let rows: Vec<String> = list.iter().take(pawline_core::dirs::SHOWN).filter(|r| r.path.starts_with(home)).map(|r| r.path.to_string_lossy().to_string()).collect();
    Some((pawline_core::dirs::render(src, &list, home), rows))
}

/// Folders worth remembering from completed steps: where files were made, moved, copied or renamed to, or listed.
pub fn folders_of(ops: &[Operation]) -> Vec<std::path::PathBuf> {
    use pawline_core::plan::Operation as O;
    let mut v = vec![];
    for o in ops {
        match o {
            O::CreateDir { path } | O::List { path } => v.push(path.clone()),
            O::CreateFile { path } => v.extend(path.parent().map(|p| p.to_path_buf())),
            O::Move { to, .. } | O::Copy { to, .. } | O::Rename { to, .. } => v.extend(to.parent().map(|p| p.to_path_buf())),
            _ => {}
        }
    }
    v
}

/// Local task-step memory commands. None when the input is not one of them. Display/delete only.
pub fn task_memory_command(input: &str, path: Option<&std::path::Path>) -> Option<String> {
    use pawline_core::planmem as pm;
    let low = input.trim().to_lowercase();
    let low = low.trim_end_matches(['.', '!']);
    let listing = matches!(low, "task steps" | "show task steps" | "task memory");
    let forget = matches!(low, "forget task steps" | "clear task steps" | "forget task memory");
    let ask = low.strip_prefix("what did you do for ");
    if !listing && !forget && ask.is_none() {
        return None;
    }
    let Some(p) = path else { return Some("No data folder.".into()) };
    Some(if listing {
        pm::describe_all(p)
    } else if forget {
        format!("Deleted {} saved task routine(s).", pm::forget_all(p))
    } else {
        pm::suggest(p, ask.unwrap_or("")).unwrap_or_else(|| "I have no saved steps for that request.".into())
    })
}

/// Save completed steps as readable text, only when every step is a local file/app step.
/// Answers, searches, online asks, pictures, undo and list are never remembered. Returns whether it saved.
pub fn remember_steps(path: &std::path::Path, request: &str, ops: &[Operation], now: u64) -> bool {
    use pawline_core::plan::Operation as O;
    if ops.is_empty() || ops.iter().any(|o| matches!(o, O::Answer { .. } | O::Search { .. } | O::AskOnline { .. } | O::GenerateImage { .. } | O::Undo { .. } | O::List { .. })) {
        return false;
    }
    let steps: Vec<String> = ops.iter().map(|o| o.describe()).collect();
    pawline_core::planmem::record_success(path, request, &steps, now)
}

fn handle_text_inner(state: &mut UiState, input: &str) -> UiReply {
    // Local, offline controls for the search memory (Pawline's own file, nothing leaves the machine).
    {
        use pawline_core::searchmem as sm;
        let low = input.trim().to_lowercase();
        let low = low.trim_end_matches(['.', '!']);
        if matches!(low, "search memory" | "show search memory" | "what do you remember" | "what have you saved") {
            return reply("message", sm::default_path().map_or("No data folder.".into(), |p| sm::describe_all(&p)));
        }
        if matches!(low, "forget searches" | "forget search memory" | "clear search memory") {
            return reply(
                "message",
                match sm::default_path() {
                    Some(p) => format!("Deleted {} saved search answer(s).", sm::forget_all(&p)),
                    None => "No data folder.".into(),
                },
            );
        }
    }
    if let Some((m, rows)) = dirs_command_rows(input, pawline_core::dirs::own_path().as_deref(), &state.root, pawline_core::searchmem::now_secs(), pawline_core::dirs::zoxide_ranked) {
        let mut r = reply("message", m);
        if !rows.is_empty() {
            r.plan = Some(rows.join("\n")); // folders a click may use; shown as a chooser by the window
        }
        return r;
    }
    if let Some(r) = shrink_command(state, input) {
        return r;
    }
    if let Some(r) = organize_command(state, input) {
        return r;
    }
    if let Some((text_search, q)) = pawline_core::finder::parse(input) {
        // Pawline's own data folder is never searched, besides the journal store the scope always leaves out.
        let own_dir: Vec<std::path::PathBuf> = pawline_core::searchmem::default_path().and_then(|p| p.parent().map(|d| d.to_path_buf())).into_iter().collect();
        let Some(sc) = pawline_core::finder::Scope::new(&state.root, own_dir) else { return reply("failed", "I can't open your home area to search it.".into()) };
        let f = if text_search {
            pawline_core::finder::find_text(&sc, &q)
        } else {
            pawline_core::finder::find_names(&sc, &q.split_whitespace().map(String::from).collect::<Vec<_>>())
        };
        let mut r = reply("message", pawline_core::finder::render(if text_search { "rg" } else { "fd" }, &q, &sc.root, &f));
        let rows = pawline_core::finder::dir_rows(&f);
        if !rows.is_empty() {
            r.plan = Some(rows.join("\n"));
        }
        return r;
    }
    if let Some(m) = task_memory_command(input, pawline_core::planmem::default_path().as_deref()) {
        return reply("message", m);
    }
    let refreshed;
    let input = if let Some(rest) = input.trim().strip_prefix("refresh:") {
        FORCE_REFRESH.store(true, std::sync::atomic::Ordering::SeqCst);
        refreshed = rest.trim().to_string();
        refreshed.as_str()
    } else {
        input
    };
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    state.engine.set_force_confirm(state.picks.iter().any(|p| p == input));
    state.picks.clear();
    // A destination named in words ("... in models") may be found among the user's frequent folders.
    {
        let low = input.to_lowercase();
        let hints = if [" in ", " inside ", " under ", " within "].iter().any(|k| low.contains(k)) {
            match pawline_core::dirs::own_path() {
                Some(own) => pawline_core::dirs::ranked(pawline_core::dirs::zoxide_ranked(), &own, pawline_core::searchmem::now_secs(), "").1.into_iter().map(|r| r.path).collect(),
                None => vec![],
            }
        } else {
            vec![]
        };
        state.engine.set_dir_hints(hints);
    }
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
    // Several folders fit the destination words: a normal question with the choices clickable.
    // A pick only fills the input with the same request and the full path; Enter then shows the confirmation.
    if let Err(PawError::Ambiguous { words, choices }) = &r {
        let low = input.to_lowercase();
        if let Some(at) = low.find(&words.to_lowercase()) {
            if input.is_char_boundary(at) && input.is_char_boundary(at + words.len()) {
                let rows: Vec<String> = choices
                    .iter()
                    .map(|c| format!("{}\t{}{}{}", c.display(), &input[..at], c.display(), &input[at + words.len()..]))
                    .collect();
                let list = choices.iter().enumerate().map(|(k, c)| format!("{}. {}", k + 1, c.display())).collect::<Vec<_>>().join("\n");
                let mut out = reply("message", format!("Which folder do you mean by \"{words}\"?\n{list}\nPick one (click, or Up/Down then Tab), then press Enter. You will see the full path and can say yes or no. Nothing is made yet."));
                state.picks = rows.iter().filter_map(|r| r.split_once('\t').map(|x| x.1.to_string())).collect();
                out.plan = Some(rows.join("\n"));
                return out;
            }
        }
    }
    // Online answer for a plain question the parser and the assist could not handle.
    // Automatic only for clearly general questions; anything else asks first.
    if matches!(&r, Err(PawError::Parse(_))) {
        use pawline_core::textfallback::{classify, Class};
        match classify(input) {
            Class::Auto => {
                let q = input.trim().to_string();
                return match ask_online(&q) {
                    Ok(a) => reply("message", a),
                    Err(e) => {
                        let t = e.to_string();
                        let t = t.strip_prefix("Execution failed: ").unwrap_or(&t).to_string();
                        reply(if t.contains("Cancelled.") { "message" } else { "failed" }, t)
                    }
                };
            }
            Class::Ask => {
                let op = Operation::AskOnline { text: input.trim().to_string() };
                let summary = format!("1. {}", op.describe());
                let plan = Plan {
                    ops: vec![op],
                    needs_confirmation: true,
                    confirmation_reasons: vec!["This would send your text to an online service.".into()],
                    assist_derived: false,
                    notes: vec![],
                };
                state.pending = Some(plan);
                state.pending_request = None;
                return UiReply { kind: "confirm".into(), text: format!("This needs your confirmation:\n{summary}"), plan: Some(summary) };
            }
            Class::Block => {
                return reply("message", "That looks like it contains a password, key, token or other secret. I won't send it to an online service, and there is no way to approve that. Nothing was sent.".into());
            }
            Class::No => {}
        }
    }
    match r {
        Ok(Response::Executed(ops)) => {
            state.undo_batch = None; // anything else that ran since an organize means "undo organize" is no longer the last thing
            let done: Vec<String> = ops.iter().map(|o| o.op.describe()).collect();
            if let Some(p) = pawline_core::dirs::own_path() {
                pawline_core::dirs::record_own(&p, &folders_of(&ops.iter().map(|o| o.op.clone()).collect::<Vec<_>>()), pawline_core::searchmem::now_secs());
            }
            if let Some(p) = pawline_core::planmem::default_path() {
                remember_steps(&p, input, &ops.iter().map(|o| o.op.clone()).collect::<Vec<_>>(), pawline_core::searchmem::now_secs());
            }
            let body = if ops.len() == 1
                && matches!(ops[0].op, pawline_core::plan::Operation::Answer { .. })
            {
                done.join("\n")
            } else {
                let lines: Vec<String> = ops
                    .iter()
                    .map(|o| match &o.op {
                        pawline_core::plan::Operation::GenerateImage { provider, .. } => {
                            format!("Picture ready ({})\n{}", provider.label(), o.detail)
                        }
                        pawline_core::plan::Operation::Shrink { .. } => o.detail.clone(),
                        pawline_core::plan::Operation::Search { .. }
                        | pawline_core::plan::Operation::AskOnline { .. }
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
            state.pending_request = Some(input.trim().to_string());
            r
        }
        Ok(Response::Message(m)) => reply("message", m),
        Ok(Response::Failed { completed, failed, error, remaining }) => reply(if error.to_string().contains("Cancelled.") { "message" } else { "failed" }, fail_text(completed.len(), &failed.to_string(), &error.to_string(), remaining.to_string())),
        Err(e) => reply("error", e.to_string()),
    }
}

/// A single step that failed with nothing else done reads as one plain sentence.
fn fail_text(done: usize, failed: &str, error: &str, remaining: String) -> String {
    let plain = error.strip_prefix("Execution failed: ").unwrap_or(error);
    if plain.starts_with("Cancelled.") {
        return plain.to_string();
    }
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
    let request = state.pending_request.take();
    let organize_n = state.organize_pending.take();
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    match state.engine.execute_plan(plan, &backend) {
        Ok(Response::Executed(ops)) => {
            state.undo_batch = organize_n;
            // Recorded only after the confirmed plan fully succeeded (cancelled and failed plans arrive as Failed).
            if let Some(p) = pawline_core::dirs::own_path() {
                pawline_core::dirs::record_own(&p, &folders_of(&ops.iter().map(|o| o.op.clone()).collect::<Vec<_>>()), pawline_core::searchmem::now_secs());
            }
            if let (Some(req), Some(p)) = (request.as_deref(), pawline_core::planmem::default_path()) {
                remember_steps(&p, req, &ops.iter().map(|o| o.op.clone()).collect::<Vec<_>>(), pawline_core::searchmem::now_secs());
            }
            let done: Vec<String> = ops.iter().map(|o| o.op.describe()).collect();
            let body = if ops.len() == 1
                && matches!(ops[0].op, pawline_core::plan::Operation::Answer { .. })
            {
                done.join("\n")
            } else {
                let lines: Vec<String> = ops
                    .iter()
                    .map(|o| match &o.op {
                        pawline_core::plan::Operation::GenerateImage { provider, .. } => {
                            format!("Picture ready ({})\n{}", provider.label(), o.detail)
                        }
                        pawline_core::plan::Operation::Shrink { .. } => o.detail.clone(),
                        pawline_core::plan::Operation::Search { .. }
                        | pawline_core::plan::Operation::AskOnline { .. }
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
        Ok(Response::Failed { completed, failed, error, remaining }) => {
            let mut text = fail_text(completed.len(), &failed.to_string(), &error.to_string(), remaining.to_string());
            if organize_n.is_some() && !completed.is_empty() {
                // The steps that did happen are in the journal; the failed and unrun ones are not.
                state.undo_batch = Some(completed.len());
                text.push_str(&format!("\nThe {} completed step(s) stay done. Say \"undo organize\" to put them back.", completed.len()));
            }
            reply(if error.to_string().contains("Cancelled.") { "message" } else { "failed" }, text)
        }
        _ => reply("error", "unexpected state".into()),
    }
}


pub fn new_state(root: std::path::PathBuf) -> UiState {
    let notes = startup_notes_from(&root);
    UiState {
        picks: vec![],
        organize_pending: None,
        undo_batch: None,
        carried: std::time::Duration::ZERO,
        engine: Engine::new(root.clone()),
        root,
        pending: None,
        pending_request: None,
        assist: None,
        assist_tried: false,
        notes,
    }
}

/// "organize <place>": plan moves of the loose files into type folders and ask. "undo organize" reverses the last one that ran.
/// "shrink/compress <video>": one reviewed plan, never touches the original, new copy next to it.
fn shrink_command(state: &mut UiState, input: &str) -> Option<UiReply> {
    let words = pawline_core::shrink::parse(input)?;
    let home = std::fs::canonicalize(&state.root).unwrap_or_else(|_| state.root.clone());
    let mut found = pawline_core::shrink::find(&home, &words);
    found.truncate(12);
    match found.len() {
        0 => Some(reply("message", format!("I found no video for \"{words}\" in Videos, Downloads, Desktop or your home folder. Say the full path, for example \"shrink video ~/Videos/trip.mp4\". I shrink mp4, mkv, mov, webm, avi, m4v, wmv and flv. Nothing was changed."))),
        1 => {
            let from = found.remove(0);
            let to = pawline_core::shrink::output_for(&from);
            let size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
            let op = Operation::Shrink { from: from.clone(), to };
            let summary = format!("1. {}", op.describe());
            let plan = Plan {
                ops: vec![op],
                needs_confirmation: true,
                confirmation_reasons: vec![format!("This runs ffmpeg on a {} video and saves a new smaller file next to it. It can take several minutes (Cancel stops it). The new file is only kept if it decodes without errors, has the same length, keeps the video and audio streams listed above and is really smaller; otherwise nothing is saved.", pawline_core::shrink::mb(size))],
                assist_derived: false,
                notes: vec![],
            };
            state.pending = Some(plan);
            state.pending_request = None; // never remembered as a reusable plan
            state.organize_pending = None;
            Some(UiReply { kind: "confirm".into(), text: format!("This needs your confirmation:\n{summary}\nIt runs ffmpeg on a {} video and saves a new smaller file next to it. It can take several minutes (Cancel stops it). The new file is only kept if it decodes without errors, has the same length, keeps the video and audio streams listed above and is really smaller; otherwise nothing is saved.", pawline_core::shrink::mb(size)), plan: Some(summary) })
        }
        _ => {
            let rows: Vec<String> = found.iter().map(|c| format!("{}\tshrink video {}", c.display(), c.display())).collect();
            let list = found.iter().enumerate().map(|(k, c)| format!("{}. {}", k + 1, c.display())).collect::<Vec<_>>().join("\n");
            state.picks = rows.iter().filter_map(|r| r.split_once('\t').map(|x| x.1.to_string())).collect();
            let mut out = reply("message", format!("Which video do you mean by \"{words}\"?\n{list}\nPick one (click, or Up/Down then Tab), then press Enter. You will see the plan first. Nothing is changed yet."));
            out.plan = Some(rows.join("\n"));
            Some(out)
        }
    }
}

fn organize_command(state: &mut UiState, input: &str) -> Option<UiReply> {
    let low = input.trim().trim_end_matches(['.', '!']).to_lowercase();
    if matches!(low.as_str(), "undo organize" | "undo the organize" | "undo organizing" | "undo all") {
        let Some(n) = state.undo_batch.take() else {
            return Some(reply("message", "I only remember the last organize in this session, and there is none to undo. Say \"undo\" to undo the last single step.".into()));
        };
        let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
            Ok(b) => b,
            Err(e) => return Some(reply("error", format!("backend: {e}"))),
        };
        let mut done = 0;
        for _ in 0..n {
            // Each step is the journal's own reverse of one organize step; the whole batch was reviewed when it ran.
            match state.engine.handle_with("undo", &backend, false, None) {
                Ok(Response::Executed(_)) => done += 1,
                Ok(Response::Confirm(plan)) => match state.engine.execute_plan(plan, &backend) {
                    Ok(Response::Executed(_)) => done += 1,
                    _ => break,
                },
                _ => break,
            }
        }
        return Some(if done == n {
            reply("message", format!("Undid the organize: {done} step(s) put back."))
        } else {
            reply("failed", format!("Undid {done} of {n} steps, then stopped. Say \"undo\" to continue one step at a time."))
        });
    }
    if low == "undo" || low.starts_with("undo ") {
        state.undo_batch = None; // a single undo or any other undo changes what "the last organize" would reverse
    }
    let words = pawline_core::organize::parse(input)?;
    {
        let hints = match pawline_core::dirs::own_path() {
            Some(own) => pawline_core::dirs::ranked(pawline_core::dirs::zoxide_ranked(), &own, pawline_core::searchmem::now_secs(), "").1.into_iter().map(|r| r.path).collect(),
            None => vec![],
        };
        state.engine.set_dir_hints(hints);
    }
    let folder = match state.engine.resolve_place(&words) {
        Ok(Some(f)) => f,
        Ok(None) => return Some(reply("message", format!("I found no folder for \"{words}\". Say the full path, for example \"organize /path/to/folder\". Nothing was changed."))),
        Err(PawError::Ambiguous { words: w, choices }) => {
            let rows: Vec<String> = choices.iter().map(|c| format!("{}\torganize {}", c.display(), c.display())).collect();
            let list = choices.iter().enumerate().map(|(k, c)| format!("{}. {}", k + 1, c.display())).collect::<Vec<_>>().join("\n");
            state.picks = rows.iter().filter_map(|r| r.split_once('\t').map(|x| x.1.to_string())).collect();
            let mut out = reply("message", format!("Which folder do you mean by \"{w}\"?\n{list}\nPick one (click, or Up/Down then Tab), then press Enter. You will see every move first. Nothing is changed yet."));
            out.plan = Some(rows.join("\n"));
            return Some(out);
        }
        Err(e) => return Some(reply("failed", e.to_string())),
    };
    let o = match pawline_core::organize::plan(&folder) {
        Ok(o) => o,
        Err(e) => return Some(reply("failed", e)),
    };
    let moves = o.ops.iter().filter(|p| matches!(p, Operation::Move { .. })).count();
    if moves == 0 {
        let extra = if o.notes.is_empty() { String::new() } else { format!("\n{}", o.notes.join("\n")) };
        return Some(reply("message", format!("Nothing to organize in {}: no loose files of a known type.{extra}", folder.display())));
    }
    let n = o.ops.len();
    let summary = o.ops.iter().enumerate().map(|(i, p)| format!("{}. {}", i + 1, p.describe())).collect::<Vec<_>>().join("\n");
    let plan = Plan {
        ops: o.ops,
        needs_confirmation: true,
        confirmation_reasons: vec![format!("This moves {moves} file(s) inside {}. Files are moved, never copied, deleted or overwritten. If a move cannot be done I stop and say which. Check the list before saying yes.", folder.display())],
        assist_derived: false,
        notes: o.notes.clone(),
    };
    state.pending = Some(plan);
    state.pending_request = Some(input.trim().to_string());
    state.organize_pending = Some(n);
    let notes = if o.notes.is_empty() { String::new() } else { format!("\n{}", o.notes.join("\n")) };
    Some(UiReply { kind: "confirm".into(), text: format!("This needs your confirmation:\nOrganize {}: move {moves} file(s) into type folders.\n{summary}{notes}", folder.display()), plan: Some(summary) })
}

pub fn cancel(state: &mut UiState) -> UiReply {
    state.carried = std::time::Duration::ZERO;
    state.pending = None;
    state.pending_request = None;
    state.organize_pending = None;
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
    FORCE_REFRESH.store(false, std::sync::atomic::Ordering::SeqCst);
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
mod memory_tests {
    use super::*;
    #[test]
    fn zi_command_uses_zoxide_then_own_list_and_ignores_other_text() {
        use pawline_core::dirs::Ranked;
        let d = std::env::temp_dir().join(format!("pawline-zi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let proj = d.join("projects");
        std::fs::create_dir_all(&proj).unwrap();
        let own = d.join("ff.json");
        assert!(dirs_command("make a folder zi", Some(&own), &d, 1, || panic!("must not run")).is_none());
        assert!(dirs_command("ziggy", Some(&own), &d, 1, || panic!("must not run")).is_none());
        let z = dirs_command("zi", Some(&own), &d, 1, || Some(vec![Ranked { score: 5.0, path: proj.clone() }])).unwrap();
        assert!(z.contains("source: zoxide") && z.contains("projects"));
        let (_, rows) = dirs_command_rows("zi", Some(&own), &d, 1, || Some(vec![Ranked { score: 5.0, path: proj.clone() }, Ranked { score: 4.0, path: "/etc".into() }])).unwrap();
        assert_eq!(rows, vec![proj.to_string_lossy().to_string()], "only folders inside home are clickable");
        let none = dirs_command("zi proj", Some(&own), &d, 1, || None).unwrap();
        assert!(none.contains("No frequent folders") && none.contains("Pawline's own list"));
        pawline_core::dirs::record_own(&own, &folders_of(&[pawline_core::plan::Operation::CreateFile { path: proj.join("a.txt") }]), 1);
        let o = dirs_command("ZI proj", Some(&own), &d, 1, || None).unwrap();
        assert!(o.contains("Pawline's own list") && o.contains("projects"));
        assert!(dirs_command("folders memory", Some(&own), &d, 1, || panic!("no zoxide")).unwrap().contains("1 folder(s)"));
        assert!(dirs_command("forget folders", Some(&own), &d, 1, || panic!("no zoxide")).unwrap().contains("Deleted 1"));
        assert!(!own.exists());
        assert!(folders_of(&[pawline_core::plan::Operation::Answer { text: "x".into() }]).is_empty());
    }

    fn tdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-bridge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("task-memory.json")
    }

    #[test]
    fn only_local_file_steps_are_remembered() {
        use pawline_core::plan::{Operation as O, SearchEngine};
        let p = tdir("rem");
        let dir = O::CreateDir { path: "/tmp/x".into() };
        assert!(remember_steps(&p, "make x", &[dir.clone()], 1));
        assert_eq!(pawline_core::planmem::load(&p).len(), 1);
        for bad in [
            O::Answer { text: "42".into() },
            O::Search { engine: SearchEngine::Web, query: "q".into() },
            O::AskOnline { text: "q".into() },
            O::Undo { entry: 1, summary: "u".into() },
            O::List { path: "/tmp".into() },
        ] {
            assert!(!remember_steps(&p, "other", &[bad.clone()], 1));
            assert!(!remember_steps(&p, "mixed", &[dir.clone(), bad], 1));
        }
        assert!(!remember_steps(&p, "empty", &[], 1));
        assert_eq!(pawline_core::planmem::load(&p).len(), 1);
        assert!(!remember_steps(&p, "my password is abc make x", &[dir], 1));
    }

    fn org_root(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-org-bridge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Downloads")).unwrap();
        for f in ["a.png", "b.pdf", "c.mp4", "x.unknown"] {
            std::fs::write(d.join("Downloads").join(f), f).unwrap();
        }
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn organize_previews_every_move_cancels_cleanly_and_runs_then_undoes() {
        let root = org_root("flow");
        let dl = root.join("Downloads");
        let mut st = new_state(root.clone());
        // preview: every move listed, nothing touched
        let r = handle_text(&mut st, "organize my downloads");
        assert_eq!(r.kind, "confirm", "{}", r.text);
        for want in ["Create folder", "Images/a.png", "Documents/b.pdf", "Videos/c.mp4"] {
            assert!(r.text.contains(want), "{want}: {}", r.text);
        }
        assert!(!r.text.contains("x.unknown") || r.text.contains("stay where they are"));
        assert!(dl.join("a.png").exists() && !dl.join("Images").exists());
        // cancel: nothing happens and nothing is pending
        let c = cancel(&mut st);
        assert!(c.text.contains("Nothing was done") && st.pending.is_none() && st.organize_pending.is_none());
        assert!(dl.join("a.png").exists() && !dl.join("Images").exists());
        // yes: runs
        let r = handle_text(&mut st, "organize my downloads");
        assert_eq!(r.kind, "confirm");
        let done = confirm_exec(&mut st);
        assert!(done.text.contains("Done"), "{}", done.text);
        assert!(dl.join("Images/a.png").is_file() && dl.join("Documents/b.pdf").is_file() && dl.join("Videos/c.mp4").is_file());
        assert!(dl.join("x.unknown").is_file() && !dl.join("a.png").exists());
        assert!(st.undo_batch.is_some());
        // undo organize: everything back, originals intact
        let u = handle_text(&mut st, "undo organize");
        assert!(u.text.contains("Undid the organize"), "{}", u.text);
        for f in ["a.png", "b.pdf", "c.mp4", "x.unknown"] {
            assert_eq!(std::fs::read_to_string(dl.join(f)).unwrap(), f);
        }
        assert!(!dl.join("Images").exists(), "folders it made are removed again");
    }

    #[test]
    fn organize_collision_and_unknown_folder_and_ambiguity() {
        let root = org_root("edge");
        let dl = root.join("Downloads");
        std::fs::create_dir_all(dl.join("Images")).unwrap();
        std::fs::write(dl.join("Images/a.png"), "OLD").unwrap();
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "organize downloads");
        assert_eq!(r.kind, "confirm");
        assert!(r.text.contains("a.png stays") && !r.text.contains("Move") || !r.text.contains("Images/a.png to"), "{}", r.text);
        let _ = confirm_exec(&mut st);
        assert_eq!(std::fs::read_to_string(dl.join("Images/a.png")).unwrap(), "OLD", "never overwritten");
        assert!(dl.join("a.png").exists(), "the colliding original stays");
        // a folder that does not exist: a plain message, nothing planned
        let r = handle_text(&mut st, "organize nonesuch");
        assert!(r.kind == "message" && r.text.contains("no folder") && st.pending.is_none(), "{}", r.text);
        // two folders with the same name: ask, offer both, never pick
        std::fs::create_dir_all(root.join("a/Pics")).unwrap();
        std::fs::create_dir_all(root.join("b/Pics")).unwrap();
        std::fs::write(root.join("a/Pics/z.png"), "z").unwrap();
        std::fs::write(root.join("b/Pics/y.png"), "y").unwrap();
        st.engine.set_dir_hints(vec![root.join("a/Pics"), root.join("b/Pics")]);
        // hints are rebuilt per request from zoxide/own data, so exercise the engine call directly
        assert!(matches!(st.engine.resolve_place("pics"), Err(PawError::Ambiguous { .. })));
        assert!(root.join("a/Pics/z.png").exists() && root.join("b/Pics/y.png").exists());
    }

    #[test]
    fn organize_failure_midway_keeps_completed_moves_journaled_and_undoable() {
        let root = org_root("fail");
        let dl = root.join("Downloads");
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "organize downloads");
        assert_eq!(r.kind, "confirm");
        // after the preview: one source vanishes
        std::fs::remove_file(dl.join("b.pdf")).unwrap();
        let done = confirm_exec(&mut st);
        assert_eq!(done.kind, "failed", "{}", done.text);
        assert!(done.text.contains("Stopped partway") && done.text.contains("undo organize"), "{}", done.text);
        // completed: Images created and a.png moved. failed one not recorded as success, the other untouched
        assert!(dl.join("Images/a.png").is_file());
        assert!(!dl.join("Videos/c.mp4").exists());
        assert!(dl.join("c.mp4").is_file(), "the original of an unrun move is untouched");
        // the completed steps are recoverable
        let u = handle_text(&mut st, "undo organize");
        assert!(u.text.contains("Undid the organize"), "{}", u.text);
        assert!(dl.join("a.png").is_file() && !dl.join("Images").exists());
        // nothing was remembered as a successful plan
        assert!(st.organize_pending.is_none());
    }

    #[test]
    fn organize_refuses_symlinks_outside_and_protects_undo_from_overwrites() {
        let root = org_root("sym");
        let dl = root.join("Downloads");
        let outside = std::env::temp_dir().join(format!("pawline-org-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        // a symlinked category folder and a symlinked file are never followed
        std::os::unix::fs::symlink(&outside, dl.join("Images")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.pdf"), dl.join("link.pdf")).unwrap();
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "organize downloads");
        assert_eq!(r.kind, "confirm", "{}", r.text);
        assert!(!r.text.contains("link.pdf") && !r.text.contains("Images/a.png"), "{}", r.text);
        let _ = confirm_exec(&mut st);
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "nothing written outside");
        assert!(dl.join("a.png").exists());
        // a folder outside home is refused outright
        let r = handle_text(&mut st, &format!("organize {}", outside.display()));
        assert!(r.kind != "confirm" && st.pending.is_none(), "{}", r.text);
        // undo never overwrites: a file that took the original name stays
        let _ = handle_text(&mut st, "undo organize");
        let root2 = org_root("undo");
        let dl2 = root2.join("Downloads");
        let mut st2 = new_state(root2.clone());
        let _ = handle_text(&mut st2, "organize downloads");
        let _ = confirm_exec(&mut st2);
        std::fs::write(dl2.join("a.png"), "SOMETHING ELSE").unwrap();
        let u = handle_text(&mut st2, "undo organize");
        assert_eq!(u.kind, "failed", "{}", u.text);
        assert_eq!(std::fs::read_to_string(dl2.join("a.png")).unwrap(), "SOMETHING ELSE");
        assert_eq!(std::fs::read_to_string(dl2.join("Images/a.png")).unwrap(), "a.png", "the moved file is kept, not lost");
    }

    #[test]
    fn organize_with_many_files_lists_every_move_and_the_count() {
        let root = org_root("many");
        let dl = root.join("Downloads");
        for i in 0..230 {
            std::fs::write(dl.join(format!("p{i:03}.png")), "x").unwrap();
        }
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "organize downloads");
        assert_eq!(r.kind, "confirm");
        let moves = r.text.lines().filter(|l| l.contains(" Move ")).count();
        assert_eq!(moves, 200, "every planned move is on the card");
        assert!(r.text.contains("move 200 file(s)") && r.text.contains("only the first 200"), "{}", &r.text[..300]);
    }

    #[test]
    fn organize_destination_taken_between_preview_and_confirm_is_not_overwritten() {
        let root = org_root("race");
        let dl = root.join("Downloads");
        std::fs::create_dir_all(dl.join("Images")).unwrap();
        let mut st = new_state(root.clone());
        assert_eq!(handle_text(&mut st, "organize downloads").kind, "confirm");
        std::fs::write(dl.join("Images/a.png"), "TAKEN").unwrap();
        let done = confirm_exec(&mut st);
        assert_eq!(done.kind, "failed", "{}", done.text);
        assert_eq!(std::fs::read_to_string(dl.join("Images/a.png")).unwrap(), "TAKEN");
        assert_eq!(std::fs::read_to_string(dl.join("a.png")).unwrap(), "a.png", "original untouched");
        assert!(!dl.join("Documents/b.pdf").exists(), "later moves were not attempted");
    }

    #[test]
    fn undo_organize_keeps_a_moved_file_that_was_edited_since() {
        let root = org_root("edit");
        let dl = root.join("Downloads");
        let mut st = new_state(root.clone());
        let _ = handle_text(&mut st, "organize downloads");
        let _ = confirm_exec(&mut st);
        std::fs::write(dl.join("Images/a.png"), "MY EDITS, MUCH LONGER NOW").unwrap();
        let u = handle_text(&mut st, "undo organize");
        assert_eq!(u.kind, "failed", "{}", u.text);
        assert_eq!(std::fs::read_to_string(dl.join("Images/a.png")).unwrap(), "MY EDITS, MUCH LONGER NOW");
        assert!(!dl.join("a.png").exists(), "not restored over anything");
        assert!(u.text.contains("Undid") , "{}", u.text);
    }

    static SHRINK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn mkvideo(path: &std::path::Path, secs: u32, size: &str, codec_args: &[&str]) -> bool {
        std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=duration={secs}:size={size}:rate=25"))
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(codec_args)
            .args(["-shortest"])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn leftovers(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.contains(".pawline-shrink")).collect()
    }

    #[test]
    fn shrink_publishes_a_checked_smaller_copy_keeps_the_original_and_undoes() {
        let _g = SHRINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = org_root("shrink");
        let dl = root.join("Downloads");
        let v = dl.join("trip.mp4");
        if !mkvideo(&v, 3, "640x480", &["-c:v", "mpeg4", "-q:v", "1", "-c:a", "aac"]) {
            eprintln!("ffmpeg not available here; skipped");
            return;
        }
        let orig = std::fs::read(&v).unwrap();
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "shrink video trip");
        assert_eq!(r.kind, "confirm", "{}", r.text);
        assert!(r.text.contains("trip-small.mp4") && r.text.contains("original is not changed"), "{}", r.text);
        assert!(r.text.contains("lossy re-encode") && r.text.contains("quality can drop") && r.text.contains("all audio streams") && r.text.contains("Drops subtitles"), "{}", r.text);
        assert!(!dl.join("trip-small.mp4").exists());
        let done = confirm_exec(&mut st);
        assert_eq!(done.kind, "executed", "{}", done.text);
        let small = std::fs::metadata(dl.join("trip-small.mp4")).unwrap().len();
        assert!(small > 0 && small < orig.len() as u64, "{small} vs {}", orig.len());
        assert_eq!(std::fs::read(&v).unwrap(), orig, "original byte for byte");
        assert!(leftovers(&dl).is_empty());
        assert!(done.text.contains("smaller") && done.text.contains("original is untouched"), "{}", done.text);
        let u = handle_text(&mut st, "undo");
        assert_eq!(u.kind, "confirm", "{}", u.text);
        let u = confirm_exec(&mut st);
        assert!(!dl.join("trip-small.mp4").exists() && v.is_file(), "{}", u.text);
    }

    #[test]
    fn shrink_that_does_not_reduce_size_saves_nothing_and_a_taken_name_is_refused() {
        let _g = SHRINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = org_root("shrink2");
        let dl = root.join("Downloads");
        let v = dl.join("tiny.mp4");
        if !mkvideo(&v, 2, "64x64", &["-c:v", "libx264", "-crf", "51", "-preset", "veryslow", "-c:a", "aac", "-b:a", "8k"]) {
            eprintln!("ffmpeg/libx264 not available here; skipped");
            return;
        }
        let mut st = new_state(root.clone());
        assert_eq!(handle_text(&mut st, "compress tiny.mp4").kind, "confirm");
        let done = confirm_exec(&mut st);
        assert!(done.kind == "failed" || done.kind == "error", "{}", done.text);
        assert!(done.text.contains("would not make it smaller"), "{}", done.text);
        assert!(!dl.join("tiny-small.mp4").exists() && leftovers(&dl).is_empty());
        // a name taken between preview and confirm is not replaced
        assert_eq!(handle_text(&mut st, "compress tiny.mp4").kind, "confirm");
        std::fs::write(dl.join("tiny-small.mp4"), "MINE").unwrap();
        let done = confirm_exec(&mut st);
        assert_ne!(done.kind, "executed");
        assert_eq!(std::fs::read_to_string(dl.join("tiny-small.mp4")).unwrap(), "MINE");
        assert!(leftovers(&dl).is_empty());
        // not a video / unknown name: plain message, nothing planned
        assert!(handle_text(&mut st, "shrink notes.txt").kind == "message" && st.pending.is_none());
    }

    #[test]
    fn cancelling_a_running_shrink_removes_the_temporary_file_and_keeps_the_original() {
        let _g = SHRINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = org_root("shrink3");
        let dl = root.join("Downloads");
        let v = dl.join("long.mp4");
        if !mkvideo(&v, 60, "1920x1080", &["-c:v", "mpeg4", "-q:v", "1", "-c:a", "aac"]) {
            eprintln!("ffmpeg not available here; skipped");
            return;
        }
        let orig = std::fs::metadata(&v).unwrap().len();
        let mut st = new_state(root.clone());
        assert_eq!(handle_text(&mut st, "shrink long.mp4").kind, "confirm");
        let h = std::thread::spawn(move || confirm_exec(&mut st));
        for _ in 0..100 {
            if !leftovers(&dl).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!leftovers(&dl).is_empty(), "ffmpeg run did not start");
        std::thread::sleep(std::time::Duration::from_millis(300));
        request_cancel();
        let r = h.join().unwrap();
        assert!(r.text.contains("Cancelled"), "{}", r.text);
        assert!(!dl.join("long-small.mp4").exists() && leftovers(&dl).is_empty());
        assert_eq!(std::fs::metadata(&v).unwrap().len(), orig);
    }

    #[test]
    fn shrink_keeps_every_audio_stream_drops_subtitles_and_says_so_and_missing_tools_change_nothing() {
        let _g = SHRINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = org_root("shrink4");
        let dl = root.join("Downloads");
        let v = dl.join("multi.mkv");
        let ok = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=duration=3:size=640x480:rate=25", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-f", "lavfi", "-i", "sine=frequency=880:duration=3", "-map", "0", "-map", "1", "-map", "2", "-c:v", "mpeg4", "-q:v", "1", "-c:a", "aac"])
            .arg(&v)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("ffmpeg not available here; skipped");
            return;
        }
        let mut st = new_state(root.clone());
        assert_eq!(handle_text(&mut st, "shrink multi.mkv").kind, "confirm");
        // tools missing: nothing changes, says how to fix it
        let old = std::env::var_os("PATH");
        let empty = root.join("emptybin");
        std::fs::create_dir_all(&empty).unwrap();
        std::env::set_var("PATH", &empty);
        let r = confirm_exec(&mut st);
        if let Some(o) = old.clone() {
            std::env::set_var("PATH", o);
        }
        assert!(r.text.contains("ffmpeg is not installed") && r.text.contains("Nothing was changed"), "{}", r.text);
        assert!(!dl.join("multi-small.mp4").exists() && leftovers(&dl).is_empty());
        // real run
        assert_eq!(handle_text(&mut st, "shrink multi.mkv").kind, "confirm");
        let r = confirm_exec(&mut st);
        assert_eq!(r.kind, "executed", "{}", r.text);
        let out = std::process::Command::new("ffprobe").args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"]).arg(dl.join("multi-small.mp4")).output().unwrap();
        let t = String::from_utf8_lossy(&out.stdout).to_string();
        assert_eq!(t.lines().filter(|l| l.trim() == "audio").count(), 2, "{t}");
        assert_eq!(t.lines().filter(|l| l.trim() == "video").count(), 1, "{t}");
        assert!(r.text.contains("decodes end to end"), "{}", r.text);
    }

    #[test]
    fn pending_request_is_cleared_on_cancel_and_never_recorded_before_success() {
        let root = std::env::temp_dir().join(format!("pawline-bridge-state-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let mut st = new_state(root);
        st.pending_request = Some("delete x".into());
        let _ = cancel(&mut st);
        assert!(st.pending_request.is_none() && st.pending.is_none());
        // a confirm with nothing pending records nothing and reports an error
        let r = confirm_exec(&mut st);
        assert_eq!(r.kind, "error");
        // a confirmable step is eligible once (and only after) it completed
        let p = tdir("conf");
        assert!(remember_steps(&p, "delete x", &[pawline_core::plan::Operation::Delete { path: "/tmp/x".into() }], 1));
    }

    #[test]
    fn task_memory_commands_list_suggest_forget() {
        let p = tdir("cmd");
        assert!(task_memory_command("make a folder", Some(&p)).is_none());
        assert_eq!(task_memory_command("Task steps.", Some(&p)).unwrap(), "No task steps saved.");
        assert!(task_memory_command("what did you do for zzz", Some(&p)).unwrap().contains("no saved steps"));
        remember_steps(&p, "make x", &[pawline_core::plan::Operation::CreateDir { path: "/tmp/x".into() }], 1);
        assert!(task_memory_command("task steps", Some(&p)).unwrap().contains("Create folder /tmp/x"));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("make x"));
        let s = task_memory_command("what did you do for make x", Some(&p)).unwrap();
        assert!(s.contains("suggestion only") && s.contains("Create folder /tmp/x"));
        assert!(task_memory_command("forget task steps", Some(&p)).unwrap().contains("Deleted 1"));
        assert!(!p.exists());
        assert_eq!(task_memory_command("task steps", None).unwrap(), "No data folder.");
    }
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
