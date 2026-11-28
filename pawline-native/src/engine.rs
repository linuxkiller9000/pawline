//! The in-process engine holder: one loaded model, kept for the whole session.
//!
//! Order of preference for a screen question is decided in `bridge::screen_qa`:
//! a llama-server the user already runs comes first (it uses every core), and
//! this engine is the no-install fallback. Without the `engine` feature the
//! crate compiles a stub, `available()` is false, and nothing here loads.
//!
//! Threads: one per core, not one, because the vision encoder is the whole cost
//! of a first look (docs/INDEPENDENT-RUNTIME.md) and it parallelises.

use std::path::Path;
use std::sync::Mutex;

struct Loaded {
    vision: String,
    mmproj: String,
    model: pawline_llama::LocalModel,
}

static ENGINE: Mutex<Option<Loaded>> = Mutex::new(None);

/// True when this build really contains the engine.
pub fn available() -> bool {
    pawline_llama::available()
}

/// How many threads the engine should use: one per core, at least one.
pub fn threads() -> u16 {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8) as u16
}

/// One question about one image, loading the model on first use and reusing it
/// afterwards. A change of model or projector file reloads.
pub fn ask_image(
    vision: &Path,
    mmproj: &Path,
    image: &[u8],
    question: &str,
    max_tokens: u32,
) -> Result<String, String> {
    if !available() {
        return Err("this build has no built-in engine".into());
    }
    let v = vision.to_string_lossy().into_owned();
    let m = mmproj.to_string_lossy().into_owned();
    let mut guard = ENGINE
        .lock()
        .map_err(|_| "the engine was left in a bad state by an earlier crash".to_string())?;
    let reload = match guard.as_ref() {
        Some(l) => l.vision != v || l.mmproj != m,
        None => true,
    };
    if reload {
        let mut model = pawline_llama::LocalModel::load(&v, 4096, threads())
            .map_err(|e| format!("could not load the vision model {v}: {e}"))?;
        model
            .load_vision(&m)
            .map_err(|e| format!("could not load the projector {m}: {e}"))?;
        *guard = Some(Loaded { vision: v, mmproj: m, model });
    }
    let loaded = guard.as_mut().expect("just loaded");
    loaded
        .model
        .ask_image(image, question, max_tokens)
        .map_err(|e| format!("the built-in engine could not answer: {e}"))
}

/// Free the model now (used when the user points at different files).
pub fn unload() {
    if let Ok(mut guard) = ENGINE.lock() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_is_at_least_one_and_sane() {
        let t = threads();
        assert!((1..=8).contains(&t), "unexpected thread count {t}");
    }

    #[test]
    fn a_build_without_the_engine_says_so_instead_of_loading() {
        let r = ask_image(
            Path::new("/nonexistent/vision.gguf"),
            Path::new("/nonexistent/mmproj.gguf"),
            b"not an image",
            "what is this?",
            8,
        );
        assert!(r.is_err(), "a missing model must not answer");
        if !available() {
            assert!(r.unwrap_err().contains("no built-in engine"));
        }
    }
}
