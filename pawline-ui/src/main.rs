#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use glib::object::ObjectType;
use gtk::prelude::WidgetExt;
use pawline_core::plan::Plan;
use pawline_core::{Engine, Response};
use pawline_core::exec::{DirectFsBackend, ExecutionBackend, OpOutcome};
use pawline_core::error::PawError;
use pawline_core::plan::{Operation, SearchEngine};

/// UI backend: confined fs execution plus the explicit online search the
/// user asked for by name.
struct UiBackend {
    inner: DirectFsBackend,
}

impl ExecutionBackend for UiBackend {
    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError> {
        if let Operation::Weather { place } = op {
            let http = pawline_core::imagegen::UreqHttp::new(std::time::Duration::from_secs(20));
            return Ok(OpOutcome { op: op.clone(), detail: pawline_core::weather::weather(&http, place)? });
        }
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
use pawline_needle::assist::NeedleAssist;
use serde::Serialize;
use tauri::Manager;
use std::os::raw::c_void;
use std::sync::Mutex;

#[allow(non_snake_case)]
extern "C" {
    fn gtk_layer_init_for_window(window: *mut c_void);
    fn gtk_layer_set_layer(window: *mut c_void, layer: i32);
    fn gtk_layer_set_anchor(window: *mut c_void, edge: i32, anchored: i32);
    fn gtk_layer_set_keyboard_mode(window: *mut c_void, mode: i32);
    fn gtk_layer_set_margin(window: *mut c_void, edge: i32, margin: i32);
    fn gtk_layer_set_namespace(window: *mut c_void, ns: *const std::os::raw::c_char);
}

const EDGE_LEFT: i32 = 0;
const EDGE_RIGHT: i32 = 1;
const EDGE_TOP: i32 = 2;
const LAYER_TOP: i32 = 2;
const KB_NONE: i32 = 0;
const KB_ON_DEMAND: i32 = 1;
const COMPACT_H: f64 = 68.0;
const COMPACT_W: f64 = 68.0;
const EXPANDED_H: f64 = 470.0;
const WIDTH: f64 = 440.0;

struct UiState {
    engine: Engine,
    root: std::path::PathBuf,
    pending: Option<Plan>,
    assist: Option<NeedleAssist>,
    assist_tried: bool,
    /// Journal recovery findings not yet shown to the user.
    notes: Vec<String>,
}

fn startup_notes_from(root: &std::path::Path) -> Vec<String> {
    DirectFsBackend::new(root).map(|b| b.recovery_notes().to_vec()).unwrap_or_default()
}

#[derive(Serialize)]
struct UiReply {
    kind: String,
    text: String,
    plan: Option<String>,
}

fn reply(kind: &str, text: String) -> UiReply {
    UiReply { kind: kind.into(), text, plan: None }
}

fn ui_state<'a>(app: &'a tauri::AppHandle) -> tauri::State<'a, Mutex<UiState>> {
    app.state::<Mutex<UiState>>()
}

fn handle_text(state: &mut UiState, input: &str) -> UiReply {
    let backend = match DirectFsBackend::new(&state.root).map(|inner| UiBackend { inner }) {
        Ok(b) => b,
        Err(e) => return reply("error", format!("backend: {e}")),
    };
    state.notes.extend(backend.inner.recovery_notes().iter().cloned());
    // Lazily load the embedded model on first parser gap; if it fails,
    // Pawline stays parser-only.
    if !state.assist_tried {
        state.assist_tried = true;
        state.assist = NeedleAssist::embedded().ok();
    }
    let r = state.engine.handle_with(
        input,
        &backend,
        false,
        state.assist.as_ref().map(|a| a as &dyn pawline_core::assist::Assist),
    );
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
                        | pawline_core::plan::Operation::Weather { .. }
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

#[tauri::command]
fn send(app: tauri::AppHandle, text: String) -> UiReply {
    let st = ui_state(&app);
    let mut state = st.lock().unwrap();
    state.pending = None;
    let r = handle_text(&mut state, &text);
    with_notes(&mut state, r)
}

/// Put any journal recovery findings in front of the reply, once.
fn with_notes(state: &mut UiState, mut r: UiReply) -> UiReply {
    if !state.notes.is_empty() {
        let n = state.notes.drain(..).collect::<Vec<_>>().join("\n");
        r.text = format!("After an interrupted run I checked the undo journal:\n{n}\n\n{}", r.text);
    }
    r
}

/// Recovery findings from startup, shown when the window first loads.
#[tauri::command]
fn startup_notes(app: tauri::AppHandle) -> Vec<String> {
    let st = ui_state(&app);
    let mut state = st.lock().unwrap();
    state.notes.drain(..).collect()
}

#[tauri::command]
fn confirm(app: tauri::AppHandle) -> UiReply {
    let st = ui_state(&app);
    let mut state = st.lock().unwrap();
    let r = confirm_inner(&mut state);
    with_notes(&mut state, r)
}

fn confirm_inner(state: &mut UiState) -> UiReply {
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
                        | pawline_core::plan::Operation::Weather { .. }
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

#[tauri::command]
fn cancel(app: tauri::AppHandle) -> UiReply {
    let st = ui_state(&app);
    let mut state = st.lock().unwrap();
    state.pending = None;
    reply("message", "Cancelled. Nothing was done.".into())
}

#[tauri::command]
fn set_mode(app: tauri::AppHandle, expanded: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let (w, h, kb) = if expanded { (WIDTH, EXPANDED_H, KB_ON_DEMAND) } else { (COMPACT_W, COMPACT_H, KB_NONE) };
        if let Ok(gtk_win) = win.gtk_window() {
            unsafe { gtk_layer_set_keyboard_mode(gtk_win.as_ptr() as *mut c_void, kb) };
        }
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
    }
}

fn main() {
    let root = std::env::temp_dir().join("pawline-ui-home");
    std::fs::create_dir_all(&root).unwrap();

    let start_expanded = std::env::var("PAWLINE_START_EXPANDED").is_ok();
    let (start_w, start_h, start_kb) = if start_expanded {
        (WIDTH, EXPANDED_H, KB_ON_DEMAND)
    } else {
        (COMPACT_W, COMPACT_H, KB_NONE)
    };

    let notes_root = root.clone();
    tauri::Builder::default()
        .manage(Mutex::new(UiState {
            engine: Engine::new(root.clone()),
            root,
            pending: None,
            assist: None,
            assist_tried: false,
            notes: startup_notes_from(&notes_root),
        }))
        .invoke_handler(tauri::generate_handler![send, confirm, cancel, set_mode, startup_notes])
        .setup(move |app| {
            let url = if start_expanded { "index.html#expanded" } else { "index.html" };
            let win = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App(url.into()))
                .title("Pawline")
                .initialization_script(if start_expanded { "window.__pawline_start_expanded = true;" } else { "" })
                .inner_size(start_w, start_h)
                .visible(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .build()?;
            let gtk_win = win.gtk_window()?;
            let ptr = gtk_win.as_ptr() as *mut c_void;
            let ns = std::ffi::CString::new("pawline").unwrap();
            // Default: plain xdg-toplevel window (the path verified to paint). layer-shell is opt-in
            // (PAWLINE_LAYERSHELL=1) because it rendered blank in the sandbox sway test.
            let use_layer = std::env::var("PAWLINE_LAYERSHELL").is_ok();
            unsafe {
                if use_layer {
                gtk_layer_init_for_window(ptr);
                gtk_layer_set_layer(ptr, LAYER_TOP);
                gtk_layer_set_anchor(ptr, EDGE_TOP, 1);
                gtk_layer_set_anchor(ptr, EDGE_RIGHT, 1);
                gtk_layer_set_anchor(ptr, EDGE_LEFT, 0);
                gtk_layer_set_margin(ptr, EDGE_TOP, 12);
                gtk_layer_set_margin(ptr, EDGE_RIGHT, 12);
                gtk_layer_set_namespace(ptr, ns.as_ptr());
                gtk_layer_set_keyboard_mode(ptr, start_kb); // compact releases keyboard
                }
            }
            win.show()?;
            // wry bug: a webview whose window starts hidden never gets its
            // internal show_all(), and then never paints. Toggle the actual
            // WebKit widget (not just the window) to force it visible.
            let _ = win.with_webview(|wv| {
                use gtk::prelude::WidgetExt;
                let webview = wv.inner();
                webview.hide();
                webview.show_all();
                webview.hide();
                webview.show_all();
            });
            gtk_win.show_all();
            if use_layer {
                // layer-shell: GTK3 never schedules a first frame (surface gets
                // configure/ack but no frame clock kick), so defer an explicit
                // redraw + webview re-toggle until after the first configure.
                let _ = win.with_webview(|wv| {
                    use gtk::prelude::WidgetExt;
                    let webview = wv.inner();
                    let w2 = webview.clone();
                    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                        w2.hide();
                        w2.show_all();
                        if let Some(toplevel) = w2.toplevel() {
                            toplevel.queue_draw();
                        }
                    });
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running pawline");
}
