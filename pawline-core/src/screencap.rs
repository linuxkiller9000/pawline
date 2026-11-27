//! Hybrid "what's on my screen" flow (user ask, 2026-11-27).
//!
//! Route: text-content questions go to OCR (tesseract) + the text model -
//! measured faster AND more accurate than either SmolVLM size on real
//! screen text (docs/EXPERIMENTS.md 4e). Visual questions (color, layout,
//! position) go to the vision model, because OCR carries no pixels. When
//! the needed route is unavailable, Pawline REFUSES instead of guessing:
//! the measured failure mode of answering anyway is fluent nonsense.
//!
//! Privacy: a capture only happens after the user confirms the card from
//! [`screen_confirm_card`], which names that the screen is captured and
//! where the pixels go. The screenshot lives in a temp file deleted when
//! the [`ScreenShot`] guard drops. Nothing is stored beyond that.

use crate::error::PawError;
use std::path::{Path, PathBuf};

/// OCR text shorter than this means "the screen has little readable text":
/// route to the vision model instead of letting the text model free-associate.
pub const MIN_OCR_CHARS: usize = 40;

/// The screen-vision model gets the capture downscaled to this width
/// (measured 4d: 960px reads almost nothing, 1920px is slow; 1280 is the
/// floor we expect to stay readable). The default image path is untouched.
pub const SCREEN_VISION_MAX_WIDTH: u32 = 1280;

/// What the question needs from the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenQuestionKind {
    /// "what does it say", "what's written", anything not in VISUAL_WORDS.
    Text,
    /// Color, layout, position, z-order, appearance: OCR cannot answer these.
    Visual,
}

/// Words that make a screen question visual. Substring match on the
/// lowercased question; a question with any of these is Visual.
const VISUAL_WORDS: &[&str] = &[
    "color", "colour", "where is", "where's", "position", "layout", "on top",
    "behind", "in front", "highlighted", "selected", "cursor", "pointing",
    "icon look", "look like", "looks like", "how big", "size of", "arrange",
    "aligned", "overlap", "visible", "showing", "displayed",
];

pub fn classify_screen_question(question: &str) -> ScreenQuestionKind {
    let q = question.to_lowercase();
    if VISUAL_WORDS.iter().any(|w| q.contains(w)) {
        ScreenQuestionKind::Visual
    } else {
        ScreenQuestionKind::Text
    }
}

/// Capture backends in order, first one present wins. `has` answers "is
/// this binary on PATH". Wayland only: on niri (and any wlr-screencopy
/// compositor) grim works; wayshot is the common alternative; niri's own
/// action is the last resort. No X11 path: honest error instead of a
/// black image.
pub fn screen_capture_command(has: &dyn Fn(&str) -> bool, dest: &Path) -> Result<Vec<String>, PawError> {
    let d = dest.to_string_lossy().to_string();
    if has("grim") {
        return Ok(vec!["grim".into(), d]);
    }
    if has("wayshot") {
        return Ok(vec!["wayshot".into(), "--file".into(), d]);
    }
    if has("niri") {
        // `--path` writes straight to `dest`; niri does it asynchronously after
        // the command returns, so the caller waits for a non-empty file.
        return Ok(vec![
            "niri".into(),
            "msg".into(),
            "action".into(),
            "screenshot-screen".into(),
            "--show-pointer".into(),
            "false".into(),
            "--path".into(),
            d,
        ]);
    }
    Err(PawError::Exec(
        "I could not find a way to take a screenshot: none of grim, wayshot or niri are installed. On Arch, `sudo pacman -S grim` is the simple one. Nothing was captured.".into(),
    ))
}


/// niri writes a screenshot file *after* its command returns, so an immediate
/// `exists()` check races it. Poll for a non-empty file instead of failing.
pub fn wait_for_capture(path: &Path, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(m) = std::fs::metadata(path) {
            if m.len() > 0 {
                return true;
            }
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A captured screenshot that deletes itself. The only copy of the pixels.
#[derive(Debug)]
pub struct ScreenShot {
    pub path: PathBuf,
}

impl ScreenShot {
    pub fn new(path: PathBuf) -> Self { Self { path } }
}

impl Drop for ScreenShot {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); }
}

/// The card the user confirms BEFORE every capture. It names exactly what
/// happens: the screen is captured, where the pixels go, and that the file
/// is deleted after. Typing never approves it (separate confirm control).
pub fn screen_confirm_card(destination: &str) -> String {
    format!(
        "Take a screenshot of your screen?\n  the whole screen is captured, including whatever is on it\n  the picture goes to: {destination}\n  the file is deleted right after the answer\nNothing is captured before you confirm."
    )
}

/// Where a question is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenRoute {
    /// OCR text to the text model.
    Ocr,
    /// Downscaled capture to the vision model.
    Vision,
    /// Honest refusal; the string is shown to the user.
    Refuse,
}

/// The routing decision, separated so it is unit-testable without any
/// capture, OCR or network.
pub fn route_question(kind: ScreenQuestionKind, ocr_chars: usize, vision_available: bool) -> (ScreenRoute, Option<String>) {
    match kind {
        ScreenQuestionKind::Visual => {
            if vision_available {
                (ScreenRoute::Vision, None)
            } else {
                (ScreenRoute::Refuse, Some(
                    "That question is about how the screen looks, and reading pixels needs a vision model, which is not set up. Load a vision model on your runner (free one: SmolVLM-500M GGUF from ggml-org) and ask again. I did not guess.".into()))
            }
        }
        ScreenQuestionKind::Text => {
            if ocr_chars >= MIN_OCR_CHARS {
                (ScreenRoute::Ocr, None)
            } else if vision_available {
                (ScreenRoute::Vision, None)
            } else {
                (ScreenRoute::Refuse, Some(
                    "I found almost no readable text on the screen, and without a vision model I cannot look at it another way. Load a vision model on your runner and ask again. I did not guess.".into()))
            }
        }
    }
}

// ---------- compositor metadata route: no capture, no model ----------

/// One window as the compositor reports it (`niri msg --json windows`).
///
/// This is the same information the synthetic eval harness in
/// experiments/screen-fork derives from its scene graph, and it is exact:
/// app, title, focus and the window's place in the layout, with no pixels
/// read and nothing sent anywhere. On the fork's strict battery
/// (docs/EXPERIMENTS.md 4g) metadata answers scored 84/86 against 36/86 for
/// a prompt- and grammar-tuned SmolVLM-256M, at a fraction of the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// niri's window id, when the list carried one. `niri msg action
    /// screenshot-window --id N` needs it to capture one window.
    pub id: Option<u64>,
    pub app_id: String,
    pub title: String,
    pub focused: bool,
    /// niri's scrolling-layout position: column is left-to-right, row is
    /// top-to-bottom. Stacked windows share a column and differ by row.
    pub column: i64,
    pub row: i64,
    /// Pixel size of the window's tile as niri lays it out, when niri reported
    /// it. Used to size the confirm card and to pick a crop; `None` means the
    /// window list did not carry `layout.tile_size`.
    pub tile_size: Option<(u32, u32)>,
}

impl WindowInfo {
    /// "org.gnome.Nautilus" -> "Nautilus", "kitty" -> "Kitty".
    pub fn app_name(&self) -> String { app_name_from_id(&self.app_id) }
}

pub fn app_name_from_id(app_id: &str) -> String {
    let last = app_id.rsplit(['.', '/']).next().unwrap_or(app_id);
    let mut c = last.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => app_id.to_string(),
    }
}

/// Parse `niri msg --json windows`. Unknown fields are ignored and missing
/// ones degrade to empty/zero, so a niri version change costs one answer
/// instead of the whole feature.
pub fn parse_niri_windows(json: &str) -> Result<Vec<WindowInfo>, PawError> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| PawError::Validation(format!("Could not read the window list from niri ({e}).")))?;
    let arr = v.as_array().ok_or_else(|| {
        PawError::Validation("The window list from niri was not a list.".into())
    })?;
    Ok(arr
        .iter()
        .map(|w| {
            let pos = w.get("layout").and_then(|l| l.get("pos_in_scrolling_layout"));
            let tile = w
                .get("layout")
                .and_then(|l| l.get("tile_size"))
                .and_then(|t| t.as_array())
                .and_then(|t| {
                    let w0 = t.first()?.as_u64()?;
                    let h0 = t.get(1)?.as_u64()?;
                    Some((u32::try_from(w0).ok()?, u32::try_from(h0).ok()?))
                });
            WindowInfo {
                id: w.get("id").and_then(|x| x.as_u64()),
                app_id: w.get("app_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                title: w.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                focused: w.get("is_focused").and_then(|x| x.as_bool()).unwrap_or(false),
                column: pos.and_then(|p| p.get(0)).and_then(|x| x.as_i64()).unwrap_or(0),
                row: pos.and_then(|p| p.get(1)).and_then(|x| x.as_i64()).unwrap_or(0),
                tile_size: tile,
            }
        })
        .collect())
}

/// The window list from niri's IPC. `None` means "niri is not installed, not
/// running, or its output did not parse" and the caller falls back to the
/// pixel path; it never means "no windows are open".
pub fn niri_windows() -> Option<Vec<WindowInfo>> {
    if !binary_on_path("niri") {
        return None;
    }
    let out = std::process::Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    parse_niri_windows(&text).ok().filter(|w| !w.is_empty())
}

/// A question the compositor answers exactly, with no pixels read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataQuestion {
    Count,
    Focus,
    Leftmost,
    Rightmost,
    LeftmostApp,
    RightmostApp,
    /// Is a window of this app open? Carries the lowercased name as asked.
    Presence(String),
}

/// App names people say, mapped to the app_ids they mean. A name outside
/// this table only ever produces a "yes" (when it matches a real window);
/// it never produces a confident "no", because a wrong "no" is worse than
/// falling through to the pixel path.
const APP_ALIASES: &[(&str, &[&str])] = &[
    ("terminal", &["kitty", "alacritty", "foot", "wezterm", "terminal", "konsole", "xterm"]),
    ("browser", &["firefox", "chromium", "chrome", "brave", "epiphany", "librewolf"]),
    ("editor", &["neovim", "vim", "code", "code-oss", "helix", "emacs", "zed"]),
    ("files", &["nautilus", "dolphin", "thunar", "nemo", "pcmanfm"]),
    ("mail", &["thunderbird", "evolution", "geary"]),
    ("chat", &["signal", "telegram", "discord", "element", "fractal"]),
];

fn presence_target(q: &str) -> Option<String> {
    let rest = ["is there a ", "is there an ", "do i have a ", "do i have "]
        .iter()
        .find_map(|p| q.strip_prefix(p))?;
    let rest = rest.trim().trim_end_matches('?').trim();
    let name = ["window open", "window", "open"]
        .iter()
        .find_map(|s| rest.strip_suffix(s))
        .unwrap_or(rest)
        .trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

pub fn classify_metadata_question(question: &str) -> Option<MetadataQuestion> {
    let q = question.to_lowercase();
    let q = q.trim().trim_end_matches(['?', '.', '!']);
    if q.contains("how many windows") || q.contains("how many app windows")
        || q.contains("count the windows") || q.contains("count the open windows")
    {
        return Some(MetadataQuestion::Count);
    }
    if (q.contains("which window") || q.contains("what window") || q.contains("name the window"))
        && (q.contains("focused") || q.contains("active"))
    {
        return Some(MetadataQuestion::Focus);
    }
    if q.contains("focused window") && q.contains("title") {
        return Some(MetadataQuestion::Focus);
    }
    let wants_app = q.contains("app") || q.contains("program") || q.contains("application");
    if q.contains("leftmost") || q.contains("far left") || q.contains("on the left") {
        return Some(if wants_app { MetadataQuestion::LeftmostApp } else { MetadataQuestion::Leftmost });
    }
    if q.contains("rightmost") || q.contains("far right") || q.contains("on the right") {
        return Some(if wants_app { MetadataQuestion::RightmostApp } else { MetadataQuestion::Rightmost });
    }
    presence_target(q).map(MetadataQuestion::Presence)
}

fn windows_in_layout_order(wins: &[WindowInfo]) -> Vec<&WindowInfo> {
    let mut v: Vec<&WindowInfo> = wins.iter().collect();
    v.sort_by_key(|w| (w.column, w.row));
    v
}

/// Answer from the window list, or `None` when the question needs pixels or
/// the list cannot settle it. `None` is a fall-through, not a failure: the
/// caller then runs the OCR/vision path and refuses if neither is possible.
pub fn answer_from_metadata(question: &str, wins: &[WindowInfo]) -> Option<(String, &'static str)> {
    let kind = classify_metadata_question(question)?;
    if wins.is_empty() {
        return None;
    }
    match kind {
        MetadataQuestion::Count => Some((wins.len().to_string(), "open windows")),
        MetadataQuestion::Focus => wins
            .iter()
            .find(|w| w.focused)
            .map(|w| (w.title.clone(), "focused window")),
        MetadataQuestion::Leftmost => {
            let v = windows_in_layout_order(wins);
            Some((v.first()?.title.clone(), "leftmost window"))
        }
        MetadataQuestion::Rightmost => {
            let v = windows_in_layout_order(wins);
            Some((v.last()?.title.clone(), "rightmost window"))
        }
        MetadataQuestion::LeftmostApp => {
            let v = windows_in_layout_order(wins);
            Some((v.first()?.app_name(), "leftmost app"))
        }
        MetadataQuestion::RightmostApp => {
            let v = windows_in_layout_order(wins);
            Some((v.last()?.app_name(), "rightmost app"))
        }
        MetadataQuestion::Presence(name) => {
            let alias = APP_ALIASES.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
            let hit = wins.iter().any(|w| {
                let id = w.app_id.to_lowercase();
                let title = w.title.to_lowercase();
                match alias {
                    Some(names) => names.iter().any(|n| id.contains(n) || title.contains(n)),
                    None => id.contains(&name) || title.contains(&name),
                }
            });
            if hit || alias.is_some() {
                Some((if hit { "yes".into() } else { "no".into() }, "open windows"))
            } else {
                None
            }
        }
    }
}

/// Decode a PNG to 8-bit RGB(A)/gray pixels. Palette and 16-bit inputs are
/// expanded/stripped by the decoder.
fn decode_png(png: &[u8]) -> Result<(Vec<u8>, u32, u32, png::ColorType), PawError> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut rdr = dec.read_info().map_err(|e| PawError::Validation(format!("That screenshot file did not decode as a PNG ({e}). Nothing was done.")))?;
    let mut buf = vec![0u8; rdr.output_buffer_size()];
    let info = rdr.next_frame(&mut buf).map_err(|e| PawError::Validation(format!("That screenshot file did not decode as a PNG ({e}). Nothing was done.")))?;
    buf.truncate(info.buffer_size());
    Ok((buf, info.width, info.height, info.color_type))
}

fn to_luma(px: &[u8], ct: png::ColorType) -> Vec<u8> {
    match ct {
        png::ColorType::Rgb => px.chunks_exact(3).map(|c| ((c[0] as u32 * 77 + c[1] as u32 * 150 + c[2] as u32 * 29) >> 8) as u8).collect(),
        png::ColorType::Rgba => px.chunks_exact(4).map(|c| ((c[0] as u32 * 77 + c[1] as u32 * 150 + c[2] as u32 * 29) >> 8) as u8).collect(),
        png::ColorType::Grayscale => px.to_vec(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).map(|c| c[0]).collect(),
        _ => Vec::new(),
    }
}

/// OCR preprocessing, measured load-bearing (4e): raw tesseract on a
/// desktop-size screenshot reads almost nothing; grayscale + a smooth 2x
/// upscale reads nearly everything. No contrast stretch: measured 2026-11-27
/// that the winning Python pipeline's autocontrast was a no-op (the resample
/// already spans 0-255), and a real stretch makes tesseract read WORSE.
/// Output is a binary PGM (P5), which tesseract reads natively.
pub fn prepare_for_ocr(png: &[u8]) -> Result<Vec<u8>, PawError> {
    let (px, w, h, ct) = decode_png(png)?;
    let stretched = to_luma(&px, ct);
    if stretched.is_empty() {
        return Err(PawError::Validation("That screenshot's pixel format is not one Pawline can read. Nothing was done.".into()));
    }
    // 2x Lanczos3 upscale. Measured (4e): bilinear blurs small glyphs and
    // tesseract reads them noticeably worse; Lanczos3 (what PIL LANCZOS does)
    // keeps the edges crisp enough to read. Center-aligned mapping like PIL.
    let (w, h) = (w as usize, h as usize);
    let (w2, h2) = (w * 2, h * 2);
    let stretched = &stretched;
    fn lanczos3(x: f64) -> f64 {
        if x.abs() >= 3.0 { return 0.0; }
        if x.abs() < 1e-9 { return 1.0; }
        let px = std::f64::consts::PI * x;
        (px.sin() / px) * ((px / 3.0).sin() / (px / 3.0))
    }
    let sample = |fx: f64, fy: f64| -> u8 {
        let (cx, cy) = (fx.floor() as i64, fy.floor() as i64);
        let mut acc = 0.0;
        let mut norm = 0.0;
        for dy in -2..=3 {
            for dx in -2..=3 {
                let (sx, sy) = (cx + dx, cy + dy);
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 { continue; }
                let k = lanczos3(fx - sx as f64) * lanczos3(fy - sy as f64);
                acc += stretched[sy as usize * w + sx as usize] as f64 * k;
                norm += k;
            }
        }
        (acc / norm).round().clamp(0.0, 255.0) as u8
    };
    let mut up = vec![0u8; w2 * h2];
    for y in 0..h2 {
        for x in 0..w2 {
            let fx = (x as f64 + 0.5) / 2.0 - 0.5;
            let fy = (y as f64 + 0.5) / 2.0 - 0.5;
            up[y * w2 + x] = sample(fx, fy);
        }
    }
    let mut out = format!("P5\n{w2} {h2}\n255\n").into_bytes();
    out.extend_from_slice(&up);
    Ok(out)
}

/// Box-average downscale to at most `max_w` pixels wide, re-encoded as PNG.
/// Identity when already small enough.
pub fn downscale_png(png: &[u8], max_w: u32) -> Result<Vec<u8>, PawError> {
    let (px, w, h, ct) = decode_png(png)?;
    if w <= max_w {
        return Ok(png.to_vec());
    }
    let ch = match ct {
        png::ColorType::Rgb => 3usize,
        png::ColorType::Rgba => 4,
        _ => return Err(PawError::Validation("That screenshot's pixel format is not one Pawline can shrink. Nothing was done.".into())),
    };
    let scale = w as f64 / max_w as f64;
    let w2 = max_w as usize;
    let h2 = ((h as f64) / scale).round().max(1.0) as usize;
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w2 * h2 * ch];
    for y2 in 0..h2 {
        for x2 in 0..w2 {
            let (x0, y0) = ((x2 as f64 * scale) as usize, (y2 as f64 * scale) as usize);
            let x1 = (((x2 + 1) as f64 * scale) as usize).min(w).max(x0 + 1);
            let y1 = (((y2 + 1) as f64 * scale) as usize).min(h).max(y0 + 1);
            let mut acc = vec![0u64; ch];
            let mut n = 0u64;
            for y in y0..y1 { for x in x0..x1 {
                for c in 0..ch { acc[c] += px[(y * w + x) * ch + c] as u64; }
                n += 1;
            }}
            for c in 0..ch { out[(y2 * w2 + x2) * ch + c] = (acc[c] / n.max(1)) as u8; }
        }
    }
    let mut enc_buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut enc_buf, w2 as u32, h2 as u32);
        enc.set_color(ct);
        enc.set_depth(png::BitDepth::Eight);
        let mut wri = enc.write_header().map_err(|e| PawError::Exec(format!("Could not re-encode the screenshot ({e}). Nothing was done.")))?;
        wri.write_image_data(&out).map_err(|e| PawError::Exec(format!("Could not re-encode the screenshot ({e}). Nothing was done.")))?;
    }
    Ok(enc_buf)
}

/// The system prompt for the OCR route. Grounds the answer in the OCR text
/// and tells the model to say when something is not there.
pub const OCR_SYSTEM: &str = "You answer questions about the user's screen using ONLY the OCR text provided. If something is not in the OCR text, say that it is not readable from the screen text. Do not invent anything.";

/// The user message for the OCR route.
pub fn ocr_user_message(ocr_text: &str, question: &str) -> String {
    format!("OCR text from my screen:\n{ocr_text}\n\nQuestion: {question}")
}

/// Is this binary on PATH? (uses `which`; false when `which` itself is gone)
pub fn binary_on_path(name: &str) -> bool {
    std::process::Command::new("which").arg(name)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}

/// Run tesseract (Apache-2.0) on a prepared PGM and return its text.
/// A missing tesseract is an honest error naming the install command.
pub fn run_tesseract(pgm: &Path) -> Result<String, PawError> {
    if !binary_on_path("tesseract") {
        return Err(PawError::Exec(
            "tesseract is not installed, so I cannot read text off the screen. On Arch: `sudo pacman -S tesseract`. Nothing was captured.".into()));
    }
    let out = std::process::Command::new("tesseract")
        .arg(pgm).arg("stdout").arg("--psm").arg("3")
        .output().map_err(|e| PawError::Exec(format!("Could not run tesseract: {e}. Nothing was done.")))?;
    if !out.status.success() {
        return Err(PawError::Exec(format!(
            "tesseract failed on the capture ({}). Nothing was done.",
            String::from_utf8_lossy(&out.stderr).chars().take(120).collect::<String>())));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// One OCR-routed turn: the OCR text plus the question, to the configured
/// provider with the OCR grounding system line. No chat history goes along.
/// Mirrors textfallback::chat, which owns the shared answer handling.
pub fn chat_with_ocr_text(http: &dyn crate::imagegen::Http, provider: &crate::providers::ChatProvider, ocr_text: &str, question: &str, cancelled: &dyn Fn() -> bool, commit: &dyn Fn() -> bool) -> Result<String, PawError> {
    let msgs = serde_json::json!([
        {"role": "system", "content": OCR_SYSTEM},
        {"role": "user", "content": ocr_user_message(ocr_text, question)}
    ]);
    let body = serde_json::json!({"model": provider.model, "messages": msgs}).to_string();
    let owned = provider.headers();
    let headers: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let r = http.post(&provider.url, &headers, &body).map_err(|_| provider.unreachable_error())?;
    if cancelled() || !commit() {
        return Err(PawError::Exec("Cancelled. Nothing was kept, and a late answer will be discarded.".into()));
    }
    match r.status {
        200 => {
            let raw = String::from_utf8_lossy(&r.body).to_string();
            let content = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| v["choices"][0]["message"]["content"].as_str().map(|s| s.to_string()))
                .ok_or_else(|| PawError::Exec(format!("{} sent a reply Pawline could not read; no answer.", provider.name)))?;
            let clean: String = content.chars().filter(|c| !c.is_control() || *c == '\n').collect();
            let clean = clean.trim();
            if clean.is_empty() {
                return Err(PawError::Exec(format!("{} sent an empty answer.", provider.name)));
            }
            Ok(clean.to_string())
        }
        s => Err(PawError::Exec(format!("{} answered HTTP {s}; no answer.", provider.name))),
    }
}


/// Which pixels a visual screen question actually needs.
///
/// Measured (docs/EXPERIMENTS.md 4h, stock SmolVLM-256M): a full frame answers
/// 5 of 16 visual questions; the same model on a crop of the window the
/// question is about answers 15 of 40 window-scoped ones and 7 of 8 progress
/// bars; on a strip of the focused window's border it answers 5 of 8
/// border-colour questions where the full frame answers 0 of 8. The crop is
/// chosen from the compositor's window list, so no pointer position is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CropChoice {
    /// Capture just this window: it arrives at native resolution instead of
    /// being squeezed into the frame's image budget.
    Window { id: Option<u64>, title: String, app: String },
    /// Capture this window, then keep only its top strip: a border is a few
    /// pixels on the window's edge, so the strip is what the model can see.
    BorderStrip { id: Option<u64>, title: String, app: String },
    /// No window is involved, or none is known: send the whole frame.
    FullFrame,
}

impl CropChoice {
    /// The truth the confirm card shows: exactly what will be captured.
    pub fn describe(&self) -> String {
        match self {
            CropChoice::Window { title, app, .. } if title.is_empty() => {
                format!("It captures only the {app} window, not the whole screen, then deletes the capture.")
            }
            CropChoice::Window { title, app, .. } => format!(
                "It captures only the \"{title}\" window ({app}), not the whole screen, then deletes the capture."
            ),
            CropChoice::BorderStrip { title, app, .. } if title.is_empty() => format!(
                "It captures the top edge of the {app} window, where the border is, not the whole screen, then deletes the capture."
            ),
            CropChoice::BorderStrip { title, app, .. } => format!(
                "It captures the top edge of the \"{title}\" window ({app}), where the border is, not the whole screen, then deletes the capture."
            ),
            CropChoice::FullFrame => {
                "This would capture your whole screen, answer from it, then delete the capture.".to_string()
            }
        }
    }
}

/// The window a question is about: the one it names, else the focused one.
pub fn window_for_question<'a>(question: &str, wins: &'a [WindowInfo]) -> Option<&'a WindowInfo> {
    let q = question.to_lowercase();
    let named = wins.iter().find(|w| {
        let app = w.app_name().to_lowercase();
        (!app.is_empty() && q.contains(&app)) || (!w.title.is_empty() && q.contains(&w.title.to_lowercase()))
    });
    named
        .or_else(|| wins.iter().find(|w| w.focused))
        .or_else(|| wins.first())
}



/// What a picker phrase asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickRequest {
    /// Draw a rectangle (the caller runs slurp and then knows the rect).
    Region,
    /// A target that is already known from the window list.
    Target(ShareTarget),
    /// The user named a window that is not open; nothing is captured.
    UnknownWindow(String),
}

/// Parse a picker phrase into what to share. `None` means the input is not a
/// picker phrase at all, so the caller keeps its normal handling. The phrases
/// are the ones the confirm card offers, so what the card promises is what the
/// user can actually say.
pub fn share_target_from_phrase(low: &str, question: &str, wins: &[WindowInfo]) -> Option<PickRequest> {
    let low = low.trim().trim_end_matches(['.', '!']).trim();
    if matches!(low, "pick a region" | "share a region" | "draw a region" | "pick region" | "share region") {
        return Some(PickRequest::Region);
    }
    if matches!(low, "share the whole screen" | "share my whole screen" | "share the screen" | "share my screen") {
        return Some(PickRequest::Target(ShareTarget::WholeScreen));
    }
    if matches!(low, "share that window" | "share this window" | "share the focused window") {
        return Some(PickRequest::Target(match window_for_question(question, wins) {
            Some(w) => ShareTarget::Window { id: w.id, title: w.title.clone(), app: w.app_name(), strip_top: false },
            None => ShareTarget::WholeScreen,
        }));
    }
    let named = low
        .strip_prefix("share only the ")
        .or_else(|| low.strip_prefix("share the "))
        .and_then(|rest| rest.strip_suffix(" window"))
        .map(|app| app.trim().to_string())?;
    Some(match wins
        .iter()
        .find(|w| w.app_name().to_lowercase() == named || w.title.to_lowercase().contains(&named))
    {
        Some(w) => PickRequest::Target(ShareTarget::Window {
            id: w.id,
            title: w.title.clone(),
            app: w.app_name(),
            strip_top: false,
        }),
        None => PickRequest::UnknownWindow(named),
    })
}

/// True when the question itself points at one window: it names a window's app
/// or title, or says "focused window" / "this window". Only then does the route
/// narrow the capture to that window on its own; otherwise the card offers the
/// whole screen and names the window option.
pub fn names_a_window(question: &str, wins: &[WindowInfo]) -> bool {
    let q = question.to_lowercase();
    if q.contains("focused window") || q.contains("this window") || q.contains("that window") {
        return !wins.is_empty();
    }
    wins.iter().any(|w| {
        let app = w.app_name().to_lowercase();
        (!app.is_empty() && q.contains(&app)) || (!w.title.is_empty() && q.contains(&w.title.to_lowercase()))
    })
}

/// Which crop a visual question needs. Text questions do not come here: they
/// go to OCR on the frame, which is measured more accurate than any crop.
pub fn crop_for_question(question: &str, wins: &[WindowInfo]) -> CropChoice {
    let q = question.to_lowercase();
    let wants_border = ["border", "outline", "frame", "edge"]
        .iter()
        .any(|w| q.contains(w));
    let Some(w) = window_for_question(question, wins) else {
        return CropChoice::FullFrame;
    };
    let (id, title, app) = (w.id, w.title.clone(), w.app_name());
    if wants_border {
        return CropChoice::BorderStrip { id, title, app };
    }
    CropChoice::Window { id, title, app }
}

/// `niri msg action screenshot-window` writes one window straight to a file, so
/// a window crop needs no geometry maths at all. `None` when niri is missing.
///
/// Field names come from niri's IPC `Action::ScreenshotWindow` (`id`,
/// `path`, `show_pointer`); the kebab-case spelling below is niri's CLI
/// convention and is covered by a test here, but it has not been run against a
/// live niri from this sandbox (no niri in it).
pub fn window_capture_command(
    has: &dyn Fn(&str) -> bool,
    dest: &Path,
    id: Option<u64>,
) -> Option<Vec<String>> {
    if !has("niri") {
        return None;
    }
    let mut v: Vec<String> = vec![
        "niri".into(),
        "msg".into(),
        "action".into(),
        "screenshot-window".into(),
        "--path".into(),
        dest.to_string_lossy().to_string(),
        "--show-pointer".into(),
        "false".into(),
    ];
    if let Some(i) = id {
        v.push("--id".into());
        v.push(i.to_string());
    }
    Some(v)
}



/// Phrases a small vision model uses when it does not recognise what it is
/// looking at. Measured (4e, 4g): it hedges or names the wrong thing rather
/// than saying it does not know, so the hedge is the cue to offer a lookup.
const UNSURE_PHRASES: &[&str] = &[
    "i don't know", "i do not know", "i'm not sure", "i am not sure", "not sure what",
    "cannot tell", "can't tell", "unable to tell", "unable to identify", "cannot identify",
    "can't identify", "no idea", "not clear what", "unclear", "unknown",
];

pub fn answer_is_unsure(answer: &str) -> bool {
    let a = answer.to_lowercase();
    UNSURE_PHRASES.iter().any(|p| a.contains(p))
}

/// The short text to offer a web search when the screen answer was unsure.
///
/// Built from the question's own words plus the app name, never from the
/// screen's text and never from a window title: a title can carry a document
/// name or a message subject, and this string is the only thing that would
/// leave the machine.
pub fn fallback_search_query(question: &str, wins: &[WindowInfo]) -> Option<String> {
    const STOP: &[&str] = &[
        "what", "which", "who", "whose", "is", "are", "was", "were", "the", "a", "an", "on", "in",
        "my", "screen", "this", "that", "these", "those", "does", "do", "did", "it", "its", "say",
        "says", "said", "color", "colour", "of", "to", "for", "tell", "me", "there", "window",
        "focused", "app", "application", "look", "looks", "like", "how", "big", "where", "and",
        "or", "at", "with", "about", "can", "you", "please", "see", "seeing", "shown", "show",
    ];
    let mut words: Vec<String> = Vec::new();
    for raw in question.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '-' && c != '.' && c != '+') {
        let w = raw.trim_matches('.');
        if w.len() < 2 || STOP.contains(&w) {
            continue;
        }
        if !words.iter().any(|x| x == w) {
            words.push(w.to_string());
        }
        if words.len() == 8 {
            break;
        }
    }
    if words.is_empty() {
        return None;
    }
    if let Some(win) = window_for_question(question, wins) {
        let app = win.app_name().to_lowercase();
        // A title can be a document name or a message subject, so its words stay
        // behind even when the question repeated them; the app name is safe.
        let title = win.title.to_lowercase();
        words.retain(|w| w == &app || !title.contains(w.as_str()));
        if app.len() > 1 && !words.iter().any(|x| *x == app) {
            words.push(app);
        }
    }
    let q = words.join(" ");
    if q.chars().filter(|c| c.is_alphanumeric()).count() < 3 {
        return None;
    }
    Some(q)
}

/// What the user chose to share, the way a screen-share tool asks: one window,
/// a region they draw, or the whole screen. The confirm card names it before
/// anything is captured, so "exactly what will be sent" is checkable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareTarget {
    WholeScreen,
    /// One window, captured by niri itself. `strip_top` keeps only the top
    /// rows, which is what makes a border colour answerable (4h: 5/8 against
    /// 0/8 on the full frame).
    Window { id: Option<u64>, title: String, app: String, strip_top: bool },
    /// A rectangle the user drew with slurp, in screen coordinates.
    Region { x: i32, y: i32, w: u32, h: u32 },
}

impl ShareTarget {
    /// What the card says will be captured, in the user's words.
    pub fn describe(&self) -> String {
        match self {
            ShareTarget::WholeScreen => {
                "It captures your whole screen - everything on it is in that picture - then deletes the capture.".to_string()
            }
            ShareTarget::Window { title, app, strip_top, .. } => {
                let what = if *strip_top { "the top edge (the border) of the" } else { "only the" };
                let name = if title.is_empty() { app.clone() } else { format!("\"{title}\" window ({app})") };
                let name = if title.is_empty() { format!("{app} window") } else { name };
                format!("It captures {what} {name}, not the whole screen, then deletes the capture. niri also puts a copy on your clipboard.")
            }
            ShareTarget::Region { x, y, w, h } => format!(
                "It captures only the region you drew ({w}x{h} at {x},{y}), not the whole screen, then deletes the capture."
            ),
        }
    }
}

/// slurp prints the rectangle it drew as "x,y wxh" (for example "120,300
/// 400x260"). `None` means the user cancelled or the output was not that shape.
pub fn parse_slurp_geometry(out: &str) -> Option<(i32, i32, u32, u32)> {
    let t = out.trim();
    let (pos, size) = t.split_once(' ')?;
    let (xs, ys) = pos.split_once(',')?;
    let (ws, hs) = size.split_once('x')?;
    let x: i32 = xs.trim().parse().ok()?;
    let y: i32 = ys.trim().parse().ok()?;
    let w: u32 = ws.trim().parse().ok()?;
    let h: u32 = hs.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((x, y, w, h))
}

/// `slurp` asks the user to draw a rectangle and prints it; the caller then
/// captures it with [`region_capture_command`].
pub fn slurp_command(has: &dyn Fn(&str) -> bool) -> Option<Vec<String>> {
    if has("slurp") {
        Some(vec!["slurp".into()])
    } else {
        None
    }
}

/// grim -g "x,y wxh" captures exactly the drawn rectangle.
pub fn region_capture_command(
    has: &dyn Fn(&str) -> bool,
    dest: &Path,
    rect: (i32, i32, u32, u32),
) -> Option<Vec<String>> {
    if !has("grim") {
        return None;
    }
    let (x, y, w, h) = rect;
    Some(vec![
        "grim".into(),
        "-g".into(),
        format!("{x},{y} {w}x{h}"),
        dest.to_string_lossy().to_string(),
    ])
}

/// Keep only the top `height` rows of a PNG (the border strip).
pub fn crop_png_top(png: &[u8], height: u32) -> Result<Vec<u8>, PawError> {
    let (px, w, h, ct) = decode_png(png)?;
    let ch = match ct {
        png::ColorType::Rgb => 3usize,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        _ => {
            return Err(PawError::Validation(
                "That capture's pixel format is not one Pawline can crop. Nothing was done.".into(),
            ))
        }
    };
    let rows = height.min(h).max(1) as usize;
    let n = w as usize * rows * ch;
    if px.len() < n {
        return Err(PawError::Validation("That capture was shorter than its own size said. Nothing was done.".into()));
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, rows as u32);
        enc.set_color(ct);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc
            .write_header()
            .map_err(|e| PawError::Validation(format!("Could not write the cropped capture ({e}). Nothing was done.")))?;
        wr.write_image_data(&px[..n])
            .map_err(|e| PawError::Validation(format!("Could not write the cropped capture ({e}). Nothing was done.")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_png(w: u32, h: u32) -> Vec<u8> {
        // A real little PNG: red/green/blue/yellow quadrants, like the
        // synthetic desktop probes in docs/EXPERIMENTS.md 4d.
        let mut px = vec![0u8; (w * h * 3) as usize];
        for y in 0..h { for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            let (r, g, b) = match (x < w / 2, y < h / 2) {
                (true, true) => (200, 30, 30),
                (false, true) => (30, 30, 200),
                (true, false) => (30, 160, 30),
                (false, false) => (220, 200, 30),
            };
            px[i] = r; px[i + 1] = g; px[i + 2] = b;
        }}
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut wri = enc.write_header().unwrap();
            wri.write_image_data(&px).unwrap();
        }
        out
    }

    #[test]
    fn classify_routes_visual_words_to_vision_and_the_rest_to_ocr() {
        assert_eq!(classify_screen_question("what color is the terminal background"), ScreenQuestionKind::Visual);
        assert_eq!(classify_screen_question("where is the close button"), ScreenQuestionKind::Visual);
        assert_eq!(classify_screen_question("which window is on top"), ScreenQuestionKind::Visual);
        assert_eq!(classify_screen_question("what's on my screen"), ScreenQuestionKind::Text);
        assert_eq!(classify_screen_question("read me the error in the terminal"), ScreenQuestionKind::Text);
        assert_eq!(classify_screen_question("what does the browser tab say"), ScreenQuestionKind::Text);
    }

    #[test]
    fn routing_never_guesses() {
        // Visual question, no vision model: refuse, with the reason.
        let (r, why) = route_question(ScreenQuestionKind::Visual, 500, false);
        assert_eq!(r, ScreenRoute::Refuse);
        assert!(why.unwrap().contains("vision model"));
        // Visual question with vision: vision.
        assert_eq!(route_question(ScreenQuestionKind::Visual, 500, true).0, ScreenRoute::Vision);
        // Text question with enough OCR text: OCR, even without vision.
        assert_eq!(route_question(ScreenQuestionKind::Text, 500, false).0, ScreenRoute::Ocr);
        // Text question with too little text: vision if possible, else refuse.
        assert_eq!(route_question(ScreenQuestionKind::Text, 5, true).0, ScreenRoute::Vision);
        let (r, why) = route_question(ScreenQuestionKind::Text, 5, false);
        assert_eq!(r, ScreenRoute::Refuse);
        assert!(why.unwrap().contains("almost no readable text"));
    }

    #[test]
    fn capture_command_prefers_grim_then_wayshot_then_niri_then_honest_error() {
        let d = Path::new("/tmp/x.png");
        let cmd = screen_capture_command(&|b| b == "grim", d).unwrap();
        assert_eq!(cmd, vec!["grim", "/tmp/x.png"]);
        let cmd = screen_capture_command(&|b| b == "wayshot", d).unwrap();
        assert_eq!(cmd[0], "wayshot");
        let cmd = screen_capture_command(&|_| false, d);
        assert!(cmd.is_err());
        let cmd = screen_capture_command(&|b| b == "niri", d).unwrap();
        assert_eq!(cmd[..3], ["niri", "msg", "action"]);
    }

    #[test]
    fn confirm_card_names_capture_destination_and_deletion() {
        let card = screen_confirm_card("llama.cpp on this computer (nothing leaves it)");
        assert!(card.contains("screen is captured"));
        assert!(card.contains("llama.cpp on this computer"));
        assert!(card.contains("deleted"));
        assert!(card.contains("before you confirm"));
    }

    const SAMPLE_WINDOWS: &str = r#"[
      {"id":1,"title":"Home - Nautilus","app_id":"org.gnome.Nautilus","is_focused":false,"layout":{"pos_in_scrolling_layout":[0,1],"tile_size":[600,900],"window_size":[598,898]}},
      {"id":2,"title":"Inbox - Thunderbird","app_id":"thunderbird","is_focused":true,"layout":{"pos_in_scrolling_layout":[1,1]}},
      {"id":3,"title":"~","app_id":"kitty","is_focused":false,"layout":{"pos_in_scrolling_layout":[2,1]}}
    ]"#;

    #[test]
    fn parses_niri_window_list() {
        let w = parse_niri_windows(SAMPLE_WINDOWS).unwrap();
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].app_id, "org.gnome.Nautilus");
        assert_eq!(w[0].app_name(), "Nautilus");
        assert_eq!(w[2].column, 2);
        assert!(w[1].focused);
        assert_eq!(w[1].app_name(), "Thunderbird");
        // tile_size sizes the confirm card and the crop; absent means None
        assert_eq!(w[0].tile_size, Some((600, 900)));
        assert_eq!(w[1].tile_size, None);
        // a niri that answers with something else is an error, not an empty screen
        assert!(parse_niri_windows("not json").is_err());
        assert!(parse_niri_windows("{}").is_err());
        // missing fields degrade instead of failing the whole list
        let w = parse_niri_windows(r#"[{"title":"x"}]"#).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].app_id, "");
    }

    #[test]
    fn metadata_answers_window_questions_exactly() {
        let wins = parse_niri_windows(SAMPLE_WINDOWS).unwrap();
        let ask = |q: &str| answer_from_metadata(q, &wins);
        assert_eq!(ask("How many windows are open?").unwrap().0, "3");
        assert_eq!(ask("Which window is focused?").unwrap().0, "Inbox - Thunderbird");
        assert_eq!(ask("Which window is on the far left?").unwrap().0, "Home - Nautilus");
        assert_eq!(ask("What app is open in the far right window?").unwrap().0, "Kitty");
        assert_eq!(ask("Is there a Nautilus window open?").unwrap().0, "yes");
        assert_eq!(ask("Is there a browser window open?").unwrap().0, "no");
        // an app name outside the alias table must never produce a confident "no"
        assert_eq!(ask("Is there a Spotify window open?"), None);
        // questions the compositor cannot answer fall through to pixels/OCR
        assert_eq!(ask("What color is the border of the focused window?"), None);
        assert_eq!(ask("Read the top email subject."), None);
        // no windows at all is not an answer to "how many"
        assert_eq!(answer_from_metadata("How many windows are open?", &[]), None);
    }

    #[test]
    fn screenshot_guard_deletes_the_file() {
        let p = std::env::temp_dir().join("pawline-shot-test.png");
        std::fs::write(&p, b"x").unwrap();
        { let _s = ScreenShot::new(p.clone()); }
        assert!(!p.exists());
    }

    #[test]
    fn ocr_prep_upscales_stretches_and_writes_valid_pgm() {
        let png = test_png(8, 8);
        let pgm = prepare_for_ocr(&png).unwrap();
        assert!(pgm.starts_with(b"P5\n16 16\n255\n"));
        let px = &pgm[b"P5\n16 16\n255\n".len()..];
        assert_eq!(px.len(), 16 * 16);
        // no contrast stretch (measured no-op/harmful): luma values pass
        // through. red (200,30,30) -> 81, blue (30,30,200) -> 49 (blue is dark).
        let red = px[0];            // top-left quadrant
        let blue = px[15];          // top-right quadrant
        assert!(red > blue, "red {red} should beat blue {blue} in luma");
        assert_eq!(blue, 49);
    }

    #[test]
    fn downscale_halves_and_keeps_color_and_small_images_untouched() {
        let png = test_png(64, 32);
        let small = downscale_png(&png, 32).unwrap();
        let (px, w, h, ct) = decode_png(&small).unwrap();
        assert_eq!((w, h), (32, 16));
        assert_eq!(ct, png::ColorType::Rgb);
        // top-left pixel of the downscaled image is still the red quadrant.
        assert!(px[0] > 150 && px[1] < 80);
        // already small: returned byte-identical, no work.
        assert_eq!(downscale_png(&png, 64).unwrap(), png);
        assert_eq!(downscale_png(&png, 128).unwrap(), png);
    }
    #[test]
    fn crop_choice_follows_the_question_and_needs_no_pointer() {
        let wins = parse_niri_windows(SAMPLE_WINDOWS).unwrap();
        assert_eq!(
            crop_for_question("what does the kitty window say", &wins),
            CropChoice::Window { id: Some(3), title: "~".into(), app: "Kitty".into() }
        );
        assert_eq!(
            crop_for_question("what colour is that button", &wins),
            CropChoice::Window { id: Some(2), title: "Inbox - Thunderbird".into(), app: "Thunderbird".into() }
        );
        assert_eq!(
            crop_for_question("what color is the border of the focused window?", &wins),
            CropChoice::BorderStrip { id: Some(2), title: "Inbox - Thunderbird".into(), app: "Thunderbird".into() }
        );
        assert_eq!(crop_for_question("what colour is the border", &[]), CropChoice::FullFrame);
        let card = crop_for_question("what color is the border?", &wins).describe();
        assert!(card.contains("Inbox - Thunderbird") && card.contains("top edge"), "{card}");
    }

    #[test]
    fn window_capture_is_one_niri_call() {
        let has = |b: &str| b == "niri";
        let cmd = window_capture_command(&has, Path::new("/tmp/pawline-screen-1.png"), Some(2)).unwrap();
        assert_eq!(cmd[0], "niri");
        assert_eq!(&cmd[1..4], &["msg", "action", "screenshot-window"]);
        assert!(cmd.contains(&"/tmp/pawline-screen-1.png".to_string()));
        assert_eq!(cmd[cmd.len() - 2], "--id");
        assert_eq!(cmd[cmd.len() - 1], "2");
        let cmd = window_capture_command(&has, Path::new("/tmp/x.png"), None).unwrap();
        assert!(!cmd.contains(&"--id".to_string()));
        let none = |_: &str| false;
        assert!(window_capture_command(&none, Path::new("/tmp/x.png"), Some(1)).is_none());
    }

    #[test]
    fn top_strip_keeps_the_first_rows() {
        let (w, h) = (2u32, 3u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(y * 10 + x) as u8, 0, 0]);
            }
        }
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(&px).unwrap();
        }
        let strip = crop_png_top(&png_bytes, 1).unwrap();
        let (got, gw, gh, _) = decode_png(&strip).unwrap();
        assert_eq!((gw, gh), (2, 1));
        assert_eq!(got, vec![0, 0, 0, 1, 0, 0]); // row 0 is y*10+x for x in 0..2
        let all = crop_png_top(&png_bytes, 99).unwrap();
        let (_, _, ah, _) = decode_png(&all).unwrap();
        assert_eq!(ah, 3);
    }


    #[test]
    fn share_targets_say_exactly_what_is_captured() {
        let whole = ShareTarget::WholeScreen.describe();
        assert!(whole.contains("whole screen"), "{whole}");
        let win = ShareTarget::Window { id: Some(2), title: "Inbox - Thunderbird".into(), app: "Thunderbird".into(), strip_top: false };
        assert!(win.describe().contains("only the \"Inbox - Thunderbird\" window"), "{}", win.describe());
        assert!(win.describe().contains("not the whole screen"), "{}", win.describe());
        let strip = ShareTarget::Window { id: Some(2), title: "Inbox - Thunderbird".into(), app: "Thunderbird".into(), strip_top: true };
        assert!(strip.describe().contains("top edge"), "{}", strip.describe());
        let region = ShareTarget::Region { x: 120, y: 300, w: 400, h: 260 };
        assert!(region.describe().contains("400x260 at 120,300"), "{}", region.describe());
        // a window with no title still names the app, never an empty quote
        let untitled = ShareTarget::Window { id: None, title: String::new(), app: "Kitty".into(), strip_top: false };
        assert!(untitled.describe().contains("Kitty window"), "{}", untitled.describe());
    }

    #[test]
    fn region_pick_and_capture_are_two_plain_commands() {
        let has = |b: &str| b == "slurp" || b == "grim";
        assert_eq!(slurp_command(&has).unwrap(), vec!["slurp".to_string()]);
        let (x, y, w, h) = parse_slurp_geometry("120,300 400x260\n").unwrap();
        assert_eq!((x, y, w, h), (120, 300, 400, 260));
        assert!(parse_slurp_geometry("").is_none());
        assert!(parse_slurp_geometry("cancelled").is_none());
        assert!(parse_slurp_geometry("10,10 0x0").is_none(), "an empty drag is not a region");
        let cmd = region_capture_command(&has, Path::new("/tmp/r.png"), (x, y, w, h)).unwrap();
        assert_eq!(cmd[0], "grim");
        assert_eq!(cmd[1], "-g");
        assert_eq!(cmd[2], "120,300 400x260");
        let none = |_: &str| false;
        assert!(region_capture_command(&none, Path::new("/tmp/r.png"), (0, 0, 1, 1)).is_none());
    }


    #[test]
    fn unsure_answers_offer_a_short_lookup_text() {
        let wins = parse_niri_windows(SAMPLE_WINDOWS).unwrap();
        assert!(answer_is_unsure("I don't know what that icon is."));
        assert!(answer_is_unsure("It is unclear from this image."));
        assert!(!answer_is_unsure("The border is teal."));
        let q = fallback_search_query("what is the icon in the kitty window", &wins).unwrap();
        assert!(q.split_whitespace().count() <= 9, "{q}");
        assert!(q.contains("icon"), "{q}");
        assert!(q.contains("kitty"), "{q}");
        let q = fallback_search_query("what is that thing in the Inbox - Thunderbird window", &wins).unwrap();
        assert!(!q.contains("inbox"), "a title must not leave the machine: {q}");
        assert!(fallback_search_query("what is this", &[]).is_none());
        assert!(fallback_search_query("", &[]).is_none());
    }


    #[test]
    fn picker_phrases_match_what_the_card_offers() {
        let wins = parse_niri_windows(SAMPLE_WINDOWS).unwrap();
        let q = "what colour is that button";
        assert_eq!(share_target_from_phrase("pick a region", q, &wins), Some(PickRequest::Region));
        assert_eq!(
            share_target_from_phrase("share the whole screen", q, &wins),
            Some(PickRequest::Target(ShareTarget::WholeScreen))
        );
        assert_eq!(
            share_target_from_phrase("share only the kitty window", q, &wins),
            Some(PickRequest::Target(ShareTarget::Window {
                id: Some(3), title: "~".into(), app: "Kitty".into(), strip_top: false
            }))
        );
        assert_eq!(
            share_target_from_phrase("share that window", q, &wins),
            Some(PickRequest::Target(ShareTarget::Window {
                id: Some(2), title: "Inbox - Thunderbird".into(), app: "Thunderbird".into(), strip_top: false
            }))
        );
        assert_eq!(
            share_target_from_phrase("share only the nosuchapp window", q, &wins),
            Some(PickRequest::UnknownWindow("nosuchapp".into()))
        );
        assert_eq!(share_target_from_phrase("what is on my screen", q, &wins), None);
    }


    #[test]
    fn capture_wait_covers_niris_async_write() {
        let dir = std::env::temp_dir().join(format!("pawline-wait-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("late.png");
        let _ = std::fs::remove_file(&p);
        assert!(!wait_for_capture(&p, std::time::Duration::from_millis(60)));
        let p2 = p.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            std::fs::write(&p2, b"png").unwrap();
        });
        assert!(wait_for_capture(&p, std::time::Duration::from_millis(2000)));
        t.join().unwrap();
        let e = dir.join("empty.png");
        std::fs::write(&e, b"").unwrap();
        assert!(!wait_for_capture(&e, std::time::Duration::from_millis(40)));
        let _ = std::fs::remove_dir_all(&dir);
    }

}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    fn binary_on_path_finds_sh_and_misses_nonsense() {
        assert!(binary_on_path("sh"));
        assert!(!binary_on_path("pawline-no-such-binary-xyz"));
    }

    #[test]
    fn tesseract_reads_a_pgm_we_write() {
        if !binary_on_path("tesseract") {
            return; // sandbox without tesseract: the error path is covered by unit tests
        }
        // 40x12 white PGM with a black rectangle - garbage text is fine, the
        // contract is "runs and returns a string", quality is covered live.
        let mut pgm = b"P5\n40 12\n255\n".to_vec();
        pgm.extend(std::iter::repeat(255u8).take(40 * 12));
        let f = std::env::temp_dir().join("pawline-ocr-test.pgm");
        std::fs::write(&f, &pgm).unwrap();
        let out = run_tesseract(&f).unwrap();
        assert!(out.len() < 200);
        let _ = std::fs::remove_file(&f);
    }
}
