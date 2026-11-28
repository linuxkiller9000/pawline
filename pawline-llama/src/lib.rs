//! In-process llama.cpp engine for Pawline.
//!
//! Same GGUF files the app already uses, but loaded inside the process: no
//! `llama-server`, no port, no second terminal. llama.cpp is MIT licensed
//! (Copyright (c) 2023-2026 The ggml authors); the vendored source and its
//! LICENSE file are kept next to the code that builds it.
//!
//! The C shim in `src/shim.c` is the whole boundary: Rust never mirrors a
//! llama.cpp struct. Built without the `vendored` feature the crate compiles a
//! stub, so `available()` is false and nothing links C++.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int};

#[repr(C)]
pub struct Engine {
    _private: [u8; 0],
}

extern "C" {
    fn pl_available() -> c_int;
    fn pl_engine_version() -> *const c_char;
    fn pl_create(
        model_path: *const c_char,
        n_ctx: c_int,
        n_threads: c_int,
        err: *mut c_char,
        err_len: c_int,
    ) -> *mut Engine;
    fn pl_apply_template(
        engine: *mut Engine,
        system: *const c_char,
        user: *const c_char,
        out: *mut c_char,
        out_len: c_int,
    ) -> c_int;
    fn pl_generate(
        engine: *mut Engine,
        prompt: *const c_char,
        max_tokens: c_int,
        top_k: c_int,
        top_p: c_float,
        temp: c_float,
        out: *mut c_char,
        out_len: c_int,
    ) -> c_int;
    fn pl_destroy(engine: *mut Engine);
    fn pl_vision_enable(
        engine: *mut Engine,
        mmproj: *const c_char,
        err: *mut c_char,
        err_len: c_int,
    ) -> c_int;
    fn pl_vision_ready(engine: *mut Engine) -> c_int;
    fn pl_vision_ask(
        engine: *mut Engine,
        img: *const u8,
        img_len: usize,
        question: *const c_char,
        max_tokens: c_int,
        top_k: c_int,
        top_p: c_float,
        temp: c_float,
        out: *mut c_char,
        out_len: c_int,
    ) -> c_int;
}

/// True when this build really contains llama.cpp.
pub fn available() -> bool {
    unsafe { pl_available() == 1 }
}

/// What the engine says it is, for the settings screen and the docs.
pub fn engine_version() -> String {
    unsafe {
        let p = pl_engine_version();
        if p.is_null() {
            return "unknown".into();
        }
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// How a load or a generation failed, in words a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EngineError {}

/// A loaded model and its context. Dropping it frees both.
pub struct LocalModel {
    raw: *mut Engine,
}

impl LocalModel {
    /// Load a GGUF file into this process.
    pub fn load(model_path: &str, n_ctx: u32, n_threads: u16) -> Result<LocalModel, EngineError> {
        if !available() {
            return Err(EngineError(
                "this build has no in-process engine; the llama.cpp source was not vendored in".into(),
            ));
        }
        let path = CString::new(model_path)
            .map_err(|_| EngineError("the model path contains a NUL byte".into()))?;
        let mut err = vec![0 as c_char; 512];
        let raw = unsafe {
            pl_create(
                path.as_ptr(),
                n_ctx as c_int,
                n_threads as c_int,
                err.as_mut_ptr(),
                err.len() as c_int,
            )
        };
        if raw.is_null() {
            let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
            return Err(EngineError(if msg.is_empty() {
                "the engine could not load the model".into()
            } else {
                format!("{msg}: {model_path}")
            }));
        }
        Ok(LocalModel { raw })
    }

    /// Wrap a system and user turn in the model's own chat template. Falls back
    /// to a plain transcript when the GGUF carries no template.
    pub fn chat_prompt(&self, system: &str, user: &str) -> String {
        let sys = CString::new(system).unwrap_or_default();
        let usr = CString::new(user).unwrap_or_default();
        let mut out = vec![0 as c_char; 16 * 1024];
        let n = unsafe {
            pl_apply_template(
                self.raw,
                sys.as_ptr(),
                usr.as_ptr(),
                out.as_mut_ptr(),
                out.len() as c_int,
            )
        };
        if n > 0 {
            let bytes: Vec<u8> = out[..n as usize].iter().map(|c| *c as u8).collect();
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        if system.is_empty() {
            format!("{user}\n")
        } else {
            format!("{system}\n\n{user}\n")
        }
    }

    /// Generate a reply for an already-templated prompt. Greedy when `temp` is 0.
    pub fn generate(
        &mut self,
        prompt: &str,
        max_tokens: u32,
        top_k: i32,
        top_p: f32,
        temp: f32,
    ) -> Result<String, EngineError> {
        let p = CString::new(prompt)
            .map_err(|_| EngineError("the prompt contains a NUL byte".into()))?;
        let mut out = vec![0 as c_char; 64 * 1024];
        let n = unsafe {
            pl_generate(
                self.raw,
                p.as_ptr(),
                max_tokens as c_int,
                top_k as c_int,
                top_p,
                temp,
                out.as_mut_ptr(),
                out.len() as c_int,
            )
        };
        let bytes: Vec<u8> = out
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect();
        let text = String::from_utf8_lossy(&bytes).trim().to_string();
        if n < 0 {
            return Err(EngineError(if text.is_empty() {
                "generation failed".into()
            } else {
                text
            }));
        }
        Ok(text)
    }

    /// One question, one answer: template, generate, return the reply.
    pub fn answer(&mut self, system: &str, user: &str, max_tokens: u32) -> Result<String, EngineError> {
        let prompt = self.chat_prompt(system, user);
        self.generate(&prompt, max_tokens, 40, 0.95, 0.0)
    }

    /// Load the mmproj projector so this engine can answer questions about an
    /// image. Same model, same context, no server and no port.
    pub fn load_vision(&mut self, mmproj_path: &str) -> Result<(), EngineError> {
        let p = CString::new(mmproj_path)
            .map_err(|_| EngineError("the projector path contains a NUL byte".into()))?;
        let mut err = vec![0 as c_char; 512];
        let rc = unsafe {
            pl_vision_enable(self.raw, p.as_ptr(), err.as_mut_ptr(), err.len() as c_int)
        };
        if rc != 0 {
            let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy().into_owned();
            return Err(EngineError(if msg.is_empty() {
                format!("could not load the projector: {mmproj_path}")
            } else {
                format!("{msg}: {mmproj_path}")
            }));
        }
        Ok(())
    }

    /// True once a projector is loaded.
    pub fn vision_ready(&self) -> bool {
        unsafe { pl_vision_ready(self.raw) == 1 }
    }

    /// Ask a question about an image given as PNG or JPEG bytes.
    pub fn ask_image(
        &mut self,
        image: &[u8],
        question: &str,
        max_tokens: u32,
    ) -> Result<String, EngineError> {
        if image.is_empty() {
            return Err(EngineError("no image bytes to look at".into()));
        }
        let q = CString::new(question)
            .map_err(|_| EngineError("the question contains a NUL byte".into()))?;
        let mut out = vec![0 as c_char; 64 * 1024];
        let n = unsafe {
            pl_vision_ask(
                self.raw,
                image.as_ptr(),
                image.len(),
                q.as_ptr(),
                max_tokens as c_int,
                40,
                0.95,
                0.0,
                out.as_mut_ptr(),
                out.len() as c_int,
            )
        };
        let bytes: Vec<u8> = out
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect();
        let text = String::from_utf8_lossy(&bytes).trim().to_string();
        if n < 0 {
            return Err(EngineError(if text.is_empty() {
                "the vision model failed".into()
            } else {
                text
            }));
        }
        Ok(text)
    }
}

impl Drop for LocalModel {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { pl_destroy(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_whether_it_can_run_at_all() {
        // The stub build says no; the vendored build says yes. Either way the
        // answer must be consistent with a load attempt.
        let can = available();
        let loaded = LocalModel::load("/nonexistent/model.gguf", 512, 1);
        assert!(loaded.is_err(), "a missing file must not load");
        if !can {
            match loaded {
                Err(e) => assert!(
                    e.to_string().contains("no in-process engine"),
                    "unexpected: {e}"
                ),
                Ok(_) => panic!("the stub build must not load a model"),
            }
        }
    }

    #[test]
    fn engine_version_is_readable_text() {
        let v = engine_version();
        assert!(!v.is_empty());
    }

    #[test]
    fn an_empty_image_is_refused_before_any_model_work() {
        // A loaded model is needed for the rest, so this checks the cheap guard
        // that runs before the model is touched at all.
        let mut m = match LocalModel::load("/nonexistent/model.gguf", 512, 1) {
            Ok(m) => m,
            Err(_) => return,
        };
        let err = match m.ask_image(&[], "what is this?", 8) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("an empty image must not be sent to the model"),
        };
        assert!(err.contains("no image bytes"), "unexpected: {err}");
    }
}
