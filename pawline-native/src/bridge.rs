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

/// Places offered by the weather chooser, waiting for a number. Memory only; any other input clears it.
static WEATHER_PICK: std::sync::Mutex<Option<Vec<pawline_core::weather::Place>>> = std::sync::Mutex::new(None);

/// A typed number while a weather chooser is open fetches that place's forecast; anything else cancels the chooser.
pub fn weather_pick(input: &str) -> Option<UiReply> {
    let opts = WEATHER_PICK.lock().unwrap().take()?;
    let n: usize = input.trim().trim_end_matches('.').parse().ok().filter(|n| (1..=opts.len()).contains(n))?;
    let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(20));
    Some(match pawline_core::weather::forecast(&http, &opts[n - 1]) {
        Ok(t) => reply("executed", t),
        Err(e) => {
            let t = e.to_string();
            reply("failed", t.strip_prefix("Execution failed: ").unwrap_or(&t).to_string())
        }
    })
}

pub fn weather_pick_open() -> bool {
    WEATHER_PICK.lock().unwrap().is_some()
}

/// Online prose (Wikipedia, web instant answer, Pollinations) is drawn as Markdown, with a plain heading.
/// Weather and anything with files or paths stays raw.
fn online_md(ops: &[OpOutcome]) -> Option<String> {
    use pawline_core::plan::Operation as O;
    if ops.is_empty() {
        return None;
    }
    let mut parts = vec![];
    for o in ops {
        let (head, body) = match &o.op {
            O::Search { engine: SearchEngine::Wiki, .. } => ("Wikipedia (online)".to_string(), o.detail.clone()),
            O::Search { .. } => ("Web instant answer (online, may be out of date)".to_string(), o.detail.clone()),
            O::AskOnline { .. } => ("Pollinations answer (online, not checked, may be wrong)".to_string(), o.detail.split_once('\n').map(|x| x.1.to_string()).unwrap_or_else(|| o.detail.clone())),
            _ => return None,
        };
        parts.push(format!("**{head}**\n\n{body}"));
    }
    Some(parts.join("\n\n"))
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
        if let Operation::Weather { place } = op {
            let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(20));
            let mut pend = None;
            let text = pawline_core::weather::weather(&http, place, &mut pend)?;
            *WEATHER_PICK.lock().unwrap() = pend;
            return Ok(OpOutcome { op: op.clone(), detail: text });
        }
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
    if ops.is_empty() || ops.iter().any(|o| matches!(o, O::Answer { .. } | O::Search { .. } | O::Weather { .. } | O::AskOnline { .. } | O::GenerateImage { .. } | O::Undo { .. } | O::List { .. })) {
        return false;
    }
    let steps: Vec<String> = ops.iter().map(|o| o.describe()).collect();
    pawline_core::planmem::record_success(path, request, &steps, now)
}

fn handle_text_inner(state: &mut UiState, input: &str) -> UiReply {
    if let Some(r) = weather_pick(input) {
        return r;
    }
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
    if let Some(r) = deck_command(state, input) {
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
            Class::Ask | Class::Auto => {
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
            if ops.len() == 1 && matches!(ops[0].op, pawline_core::plan::Operation::Weather { .. }) && weather_pick_open() {
                return reply("message", ops[0].detail.clone());
            }
            if let Some(md) = online_md(&ops) {
                return reply("md", md);
            }
            let answer_only = ops.len() == 1 && matches!(ops[0].op, pawline_core::plan::Operation::Answer { .. });
            let body = if answer_only {
                done.join("\n")
            } else {
                let lines: Vec<String> = ops
                    .iter()
                    .map(|o| match &o.op {
                        pawline_core::plan::Operation::GenerateImage { provider, .. } => {
                            format!("Picture ready ({})\n{}", provider.label(), o.detail)
                        }
                        pawline_core::plan::Operation::Shrink { .. } | pawline_core::plan::Operation::Deck { .. } => o.detail.clone(),
                        pawline_core::plan::Operation::Weather { .. } => o.detail.clone(),
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
            // Pawline's own plain answers are drawn as Markdown; everything that lists files or paths stays raw
            reply(if answer_only { "md" } else { "executed" }, body)
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
            if ops.len() == 1 && matches!(ops[0].op, pawline_core::plan::Operation::Weather { .. }) && weather_pick_open() {
                return reply("message", ops[0].detail.clone());
            }
            if let Some(md) = online_md(&ops) {
                return reply("md", md);
            }
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
                        pawline_core::plan::Operation::Weather { .. } => o.detail.clone(),
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
/// "make a presentation from <outline file>": local, deterministic, never changes the outline.
fn deck_command(state: &mut UiState, input: &str) -> Option<UiReply> {
    let words = pawline_core::deck::parse_command(input)?;
    let home = std::fs::canonicalize(&state.root).unwrap_or_else(|_| state.root.clone());
    let mut found = pawline_core::deck::find(&home, &words);
    found.truncate(12);
    match found.len() {
        0 => Some(reply("message", format!("I found no outline file for \"{words}\" (.md or .txt in Documents, Desktop, Downloads or home). Say the full path, for example \"make a presentation from ~/Documents/talk.md\". The outline uses \"# Title\", \"## Slide title\" and \"- bullet\" lines. Nothing was changed."))),
        1 => {
            let from = found.remove(0);
            let text = match std::fs::metadata(&from) {
                Ok(m) if m.len() <= pawline_core::deck::MAX_SOURCE => std::fs::read_to_string(&from).unwrap_or_default(),
                _ => return Some(reply("failed", format!("{} is too big or unreadable (limit 256 KB of text).", from.display()))),
            };
            let slides = match pawline_core::deck::parse_outline(&text) {
                Ok(s) => s,
                Err(e) => return Some(reply("failed", format!("I can't make slides from {}: {e}. Nothing was changed.", from.display()))),
            };
            let to = pawline_core::deck::output_for(&from);
            let op = Operation::Deck { from, to };
            let titles = slides.iter().enumerate().map(|(i, s)| format!("  {}. {} ({} line(s))", i + 1, s.title, s.bullets.len())).collect::<Vec<_>>().join("\n");
            let summary = format!("1. {}\n{} slide(s):\n{titles}", op.describe(), slides.len());
            state.pending = Some(Plan {
                ops: vec![op],
                needs_confirmation: true,
                confirmation_reasons: vec!["This saves one new .pptx file next to the outline.".into()],
                assist_derived: false,
                notes: vec![],
            });
            state.pending_request = None;
            state.organize_pending = None;
            Some(UiReply { kind: "confirm".into(), text: format!("This needs your confirmation:\n{summary}"), plan: Some(summary) })
        }
        _ => {
            let rows: Vec<String> = found.iter().map(|c| format!("{}\tmake a presentation from {}", c.display(), c.display())).collect();
            let list = found.iter().enumerate().map(|(k, c)| format!("{}. {}", k + 1, c.display())).collect::<Vec<_>>().join("\n");
            state.picks = rows.iter().filter_map(|r| r.split_once('\t').map(|x| x.1.to_string())).collect();
            let mut out = reply("message", format!("Which outline do you mean by \"{words}\"?\n{list}\nPick one (click, or Up/Down then Tab), then press Enter. You will see the plan first. Nothing is changed yet."));
            out.plan = Some(rows.join("\n"));
            Some(out)
        }
    }
}

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
    if matches!(r.kind.as_str(), "executed" | "failed" | "message" | "error" | "md") {
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

/// Why a request cannot run while a folder chip is set, or None when it is fine. With a chip, only requests whose
/// meaning does not depend on the chip folder being guessed are allowed: creating inside the chip, "here", listing
/// the chip, explicit paths (absolute or ~), and the fixed places. A bare name ("delete kp") is looked up from the
/// home root, not inside the chip, so it is refused before any planning instead of silently using another folder.
pub fn chip_conflict(input: &str, dir: &std::path::Path) -> Option<String> {
    use pawline_core::plan::{Directive as D, LocationRef as L};
    let Ok(ds) = pawline_core::intent::parse(input) else { return None };
    let bare = |l: &L| matches!(l, L::Named(_));
    let hit = ds.iter().any(|d| match d {
        D::Create { parent, .. } => bare(parent),
        D::Delete { target } | D::List { target } => bare(target),
        D::Rename { target, .. } => bare(target),
        D::Move { target, dest } | D::Copy { target, dest } => bare(target) || bare(dest),
        D::SetWallpaper { image, .. } => bare(image),
        _ => false,
    });
    hit.then(|| {
        format!(
            "A folder chip is set ({}). Names without a path are looked up from the home folder, not inside the chip, so nothing was run. Remove the chip (Backspace in an empty-start draft, or click its x), or write the full path.",
            dir.display()
        )
    })
}

/// A request typed with (or without) a folder chip. The chip folder goes to the engine first; a folder outside the
/// home area or gone is refused. Requests that would resolve names from a different folder are refused before planning.
pub fn handle_text_in(state: &mut UiState, input: &str, dir: Option<&std::path::Path>) -> UiReply {
    if let Err(e) = state.engine.set_working_dir(dir) {
        return reply("error", format!("The folder chip was not used, so nothing was run: {e}"));
    }
    if let Some(d) = dir {
        if let Some(m) = chip_conflict(input, d) {
            return reply("error", m);
        }
    }
    handle_text(state, input)
}

/// Folders for the /in chooser: remembered folders first (zoxide, else Pawline's own list) matching every keyword,
/// then a bounded name search under the home folder. Inside home only, no hidden folders, no symlinks, no repeats.
pub fn folder_candidates_with(
    home: &std::path::Path,
    own: Option<&std::path::Path>,
    now: u64,
    zox: Option<Vec<pawline_core::dirs::Ranked>>,
    query: &str,
    cap: usize,
    cancel: &dyn Fn() -> bool,
    budget: std::time::Duration,
) -> Vec<std::path::PathBuf> {
    let t0 = std::time::Instant::now();
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    let mut out: Vec<std::path::PathBuf> = vec![];
    if let Some(own) = own {
        for r in pawline_core::dirs::ranked(zox, own, now, query).1 {
            if r.path.starts_with(home) && r.path != home && !out.contains(&r.path) {
                out.push(r.path);
            }
        }
    }
    // fallback: walk the home folder (bounded), match on the full path below home
    let mut stack = vec![(home.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    let mut found: Vec<std::path::PathBuf> = vec![];
    while let Some((d, depth)) = stack.pop() {
        if cancel() || t0.elapsed() > budget {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut kids: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).collect();
        kids.sort();
        for p in kids.into_iter().rev() {
            seen += 1;
            if seen > 5000 {
                break;
            }
            let hidden = p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true);
            if hidden || p.is_symlink() || !p.is_dir() {
                continue;
            }
            let rel = p.strip_prefix(home).unwrap_or(&p).to_string_lossy().to_lowercase();
            if words.iter().all(|w| rel.contains(w)) {
                found.push(p.clone());
            }
            if depth < 5 {
                stack.push((p, depth + 1));
            }
        }
    }
    found.sort();
    for p in found {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out.truncate(cap);
    out
}

pub fn folder_candidates(home: &std::path::Path, query: &str, cancel: &dyn Fn() -> bool) -> Vec<std::path::PathBuf> {
    folder_candidates_with(home, pawline_core::dirs::own_path().as_deref(), pawline_core::searchmem::now_secs(), pawline_core::dirs::zoxide_ranked(), query, 200, cancel, std::time::Duration::from_millis(800))
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
            O::Weather { place: "Cairo".into() },
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

    #[test]
    fn every_online_ask_shows_a_disclosure_card_first_and_cancel_sends_nothing() {
        let mut st = new_state(org_root("online"));
        for (q, host) in [
            ("what's the weather in Cairo?", "geocoding-api.open-meteo.com"),
            ("search the web for niri compositor", "api.duckduckgo.com"),
            ("wiki tux the penguin", "en.wikipedia.org"),
            ("what is a strong password", "text.pollinations.ai"),
        ] {
            let r = handle_text(&mut st, q);
            assert_eq!(r.kind, "confirm", "{q}: {}", r.text);
            assert!(r.text.contains(host) && r.text.contains("IP address") && r.text.contains("does not send your files"), "{q}: {}", r.text);
            assert!(st.pending.is_some());
            let c = cancel(&mut st);
            assert!(c.text.contains("Nothing was done") && st.pending.is_none(), "{q}");
        }
        // no place: asks, no card, nothing sent
        let r = handle_text(&mut st, "what is the weather");
        assert!(r.text.contains("Which place") && st.pending.is_none());
    }

    #[test]
    fn weather_chooser_pick_and_cancel_state() {
        use pawline_core::weather::Place;
        let mut st = new_state(org_root("wpick"));
        let opts = vec![Place { label: "A, X".into(), lat: 1.0, lon: 2.0 }, Place { label: "A, Y".into(), lat: 3.0, lon: 4.0 }];
        // a non-number cancels the chooser and is then handled as normal input
        *WEATHER_PICK.lock().unwrap() = Some(opts.clone());
        assert!(weather_pick_open());
        assert!(weather_pick("never mind").is_none() && !weather_pick_open(), "any other text closes it");
        // an out-of-range number also closes it without any request
        *WEATHER_PICK.lock().unwrap() = Some(opts.clone());
        assert!(weather_pick("7").is_none() && !weather_pick_open());
        // with no chooser open a number is just text
        let r = handle_text(&mut st, "2");
        assert!(!r.text.contains("Weather for"), "a bare number is not a weather pick: {}", r.text);
        assert!(!weather_pick_open());
    }

    #[test]
    fn online_prose_is_markdown_and_weather_stays_raw() {
        use pawline_core::plan::{Operation as O, SearchEngine};
        let o = |op: O, d: &str| OpOutcome { op, detail: d.into() };
        let md = online_md(&[o(O::Search { engine: SearchEngine::Wiki, query: "q".into() }, "Wikipedia - Tux: a **penguin**\nSource: https://en.wikipedia.org/wiki/Tux")]).unwrap();
        assert!(md.starts_with("**Wikipedia (online)**\n\n") && md.contains("Source: https://en.wikipedia.org/wiki/Tux"));
        let md = online_md(&[o(O::AskOnline { text: "q".into() }, "Pollinations (online, not checked, may be wrong):\nHello")]).unwrap();
        assert!(md.contains("may be wrong)**\n\nHello") && !md.contains("Pollinations (online"));
        assert!(online_md(&[o(O::Weather { place: "x".into() }, "d")]).is_none());
        assert!(online_md(&[o(O::Answer { text: "x".into() }, "d")]).is_none());
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
    fn cancelling_a_running_shrink_with_an_unnamed_output_leaves_nothing_behind() {
        let _g = SHRINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = org_root("shrink4");
        let dl = root.join("Downloads");
        let v = dl.join("long.mp4");
        if !mkvideo(&v, 60, "1920x1080", &["-c:v", "mpeg4", "-q:v", "1", "-c:a", "aac"]) {
            eprintln!("ffmpeg not available here; skipped");
            return;
        }
        let mut st = new_state(root.clone());
        assert_eq!(handle_text(&mut st, "shrink long.mp4").kind, "confirm");
        let h = std::thread::spawn(move || confirm_exec(&mut st));
        std::thread::sleep(std::time::Duration::from_millis(100));
        request_cancel();
        let r = h.join().unwrap();
        assert!(r.text.contains("Cancelled"), "{}", r.text);
        assert!(!dl.join("long-small.mp4").exists() && leftovers(&dl).is_empty());
        assert!(v.exists());
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
    fn presentation_from_outline_previews_publishes_without_overwrite_and_undoes() {
        let root = org_root("deck");
        let dl = root.join("Downloads");
        let md = dl.join("talk.md");
        std::fs::write(&md, "# My talk\nsubtitle\n\n## One\n- a\n- b\n## Two\n- c\n").unwrap();
        let before = std::fs::read(&md).unwrap();
        let mut st = new_state(root.clone());
        let r = handle_text(&mut st, "make a presentation from talk");
        assert_eq!(r.kind, "confirm", "{}", r.text);
        assert!(r.text.contains("talk.pptx") && r.text.contains("3 slide(s)") && r.text.contains("1. My talk") && r.text.contains("Nothing is sent anywhere"), "{}", r.text);
        assert!(!dl.join("talk.pptx").exists());
        // a name taken after the preview is not replaced
        std::fs::write(dl.join("talk.pptx"), "MINE").unwrap();
        let d = confirm_exec(&mut st);
        assert_ne!(d.kind, "executed");
        assert_eq!(std::fs::read_to_string(dl.join("talk.pptx")).unwrap(), "MINE");
        std::fs::remove_file(dl.join("talk.pptx")).unwrap();
        assert_eq!(handle_text(&mut st, "make a presentation from talk").kind, "confirm");
        let d = confirm_exec(&mut st);
        assert_eq!(d.kind, "executed", "{}", d.text);
        let bytes = std::fs::read(dl.join("talk.pptx")).unwrap();
        assert_eq!(&bytes[0..2], b"PK");
        assert_eq!(std::fs::read(&md).unwrap(), before);
        assert!(std::fs::read_dir(&dl).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains(".part")));
        let u = handle_text(&mut st, "undo");
        assert_eq!(u.kind, "confirm");
        let _ = confirm_exec(&mut st);
        assert!(!dl.join("talk.pptx").exists() && md.is_file());
        // a bad outline and a non-outline file never plan anything
        std::fs::write(dl.join("bad.txt"), "just words").unwrap();
        let r = handle_text(&mut st, "make a presentation from bad.txt");
        assert!(r.kind == "failed" && r.text.contains("heading") && st.pending.is_none(), "{}", r.text);
        assert!(handle_text(&mut st, "make a presentation from a.png").kind == "message");
    }

    #[test]
    fn folder_chip_sets_the_create_folder_and_a_bad_chip_runs_nothing() {
        let root = std::env::temp_dir().join(format!("pawline-chip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("proj");
        std::fs::create_dir_all(&sub).unwrap();
        let mut st = new_state(root.canonicalize().unwrap());
        let r = handle_text_in(&mut st, "make a folder called chipdir", Some(&sub.canonicalize().unwrap()));
        assert_ne!(r.kind, "error", "{}", r.text);
        assert!(sub.join("chipdir").is_dir() && !root.join("chipdir").exists());
        // chip removed: back to the home folder
        let r = handle_text_in(&mut st, "make a folder called plain", None);
        assert_ne!(r.kind, "error", "{}", r.text);
        assert!(root.join("plain").is_dir());
        // a chip outside the home area is refused and nothing runs
        let r = handle_text_in(&mut st, "make a folder called nope", Some(std::path::Path::new("/tmp")));
        assert_eq!(r.kind, "error");
        assert!(!std::path::Path::new("/tmp/nope").exists() && !root.join("nope").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skill_chip_plus_folder_chip_dispatch_creates_in_the_chip_folder() {
        let root = std::env::temp_dir().join(format!("pawline-skchip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("proj");
        std::fs::create_dir_all(&sub).unwrap();
        let mut st = new_state(root.canonicalize().unwrap());
        let files = crate::SKILL_MENU.iter().position(|m| m.0 == "/files").unwrap();
        let text = crate::compose_request(Some(files), "kitten");
        let r = handle_text_in(&mut st, &text, Some(&sub.canonicalize().unwrap()));
        assert_ne!(r.kind, "error", "{}", r.text);
        assert!(sub.join("kitten").is_dir() && !root.join("kitten").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_search_is_cancellable_and_time_bounded() {
        let home = std::env::temp_dir().join(format!("pawline-fcb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        for i in 0..40 {
            for j in 0..40 {
                std::fs::create_dir_all(home.join(format!("d{i}/e{j}"))).unwrap();
            }
        }
        let own = home.join("ff.json");
        let t = std::time::Instant::now();
        let r = folder_candidates_with(&home, Some(&own), 1, None, "zzz", 200, &|| true, std::time::Duration::from_secs(5));
        assert!(r.is_empty() && t.elapsed() < std::time::Duration::from_millis(100), "cancelled scan returns at once");
        let t = std::time::Instant::now();
        let _ = folder_candidates_with(&home, Some(&own), 1, None, "zzz", 200, &|| false, std::time::Duration::from_millis(1));
        assert!(t.elapsed() < std::time::Duration::from_millis(500), "time budget stops the walk");
        let full = folder_candidates_with(&home, Some(&own), 1, None, "e39", 200, &|| false, std::time::Duration::from_secs(10));
        assert_eq!(full.len(), 40);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn chip_refuses_bare_names_before_planning_and_keeps_explicit_paths() {
        let d = std::path::Path::new("/h/proj");
        for bad in ["delete kp", "move kp to Documents", "copy a.txt to b", "rename kp to z", "make a folder called x inside pics"] {
            assert!(chip_conflict(bad, d).is_some(), "{bad}");
        }
        for ok in ["make a folder called x", "list", "delete /h/proj/kp", "move ~/a.txt to ~/b", "move /h/a.txt to Documents"] {
            let c = chip_conflict(ok, d);
            assert!(c.is_none(), "{ok}: {c:?}");
        }
    }

    #[test]
    fn folder_search_finds_beyond_first_8_duplicates_and_deep_folders() {
        let home = std::env::temp_dir().join(format!("pawline-fc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        for i in 0..12 {
            std::fs::create_dir_all(home.join(format!("area{i:02}/shared"))).unwrap();
        }
        std::fs::create_dir_all(home.join("a/b/c/d/deepgem")).unwrap();
        std::fs::create_dir_all(home.join(".hidden/shared")).unwrap();
        let own = home.join("ff.json");
        let all = folder_candidates_with(&home, Some(&own), 1, None, "", 200, &|| false, std::time::Duration::from_secs(5));
        assert!(all.len() > 8, "not limited to 8: {}", all.len());
        let shared = folder_candidates_with(&home, Some(&own), 1, None, "shared", 200, &|| false, std::time::Duration::from_secs(5));
        assert_eq!(shared.len(), 12, "duplicate basenames all listed with distinct full paths");
        assert!(shared.iter().all(|p| !p.to_string_lossy().contains(".hidden")));
        let deep = folder_candidates_with(&home, Some(&own), 1, None, "deepgem", 200, &|| false, std::time::Duration::from_secs(5));
        assert_eq!(deep, vec![home.join("a/b/c/d/deepgem")]);
        let two = folder_candidates_with(&home, Some(&own), 1, None, "area03 shared", 200, &|| false, std::time::Duration::from_secs(5));
        assert_eq!(two, vec![home.join("area03/shared")]);
        // remembered folders come first
        let z = vec![pawline_core::dirs::Ranked { score: 9.0, path: home.join("area07/shared") }];
        let first = folder_candidates_with(&home, Some(&own), 1, Some(z), "shared", 200, &|| false, std::time::Duration::from_secs(5));
        assert_eq!(first[0], home.join("area07/shared"));
        assert_eq!(first.len(), 12);
        let _ = std::fs::remove_dir_all(&home);
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
