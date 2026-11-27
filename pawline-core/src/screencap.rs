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
        // niri writes its own file; the caller moves it to `dest`.
        return Ok(vec!["niri".into(), "msg".into(), "action".into(), "screenshot-screen".into()]);
    }
    Err(PawError::Exec(
        "I could not find a way to take a screenshot: none of grim, wayshot or niri are installed. On Arch, `sudo pacman -S grim` is the simple one. Nothing was captured.".into(),
    ))
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
