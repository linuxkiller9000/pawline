//! FFI to the Cactus runtime with Needle3 weights compiled into the binary.
//! No model file exists anywhere at runtime: the bytes are a static blob.
//!
//! These proposals are ADVISORY. Callers must route them through
//! pawline-core validation before anything executes.

pub mod assist;

use std::ffi::{CStr, CString};

#[allow(non_camel_case_types)]
type c_char = std::os::raw::c_char;

extern "C" {
    fn cactus_load(cact: *const u8, n: u64) -> i32;
    fn cactus_init(system_prompt: *const c_char, tools_json: *const c_char, tool_index_path: *const c_char) -> i32;
    fn cactus_complete(input: *const c_char, pcm: *const f32, samples: i32, max_new_tokens: i32, out: *mut c_char, out_capacity: i32) -> i32;
    fn cactus_last_error() -> *const c_char;
    fn cactus_reset();
}

/// The weights, baked in. Apache-2.0 (Cactus-Compute/needle3); see NOTICE.
pub static NEEDLE3_CACT: &[u8] = include_bytes!("../needle3.cact");

pub struct Needle {
    handle: i32,
}

#[derive(Debug)]
pub struct NeedleError(pub String);

fn last_error() -> String {
    unsafe {
        let p = cactus_last_error();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

impl Needle {
    /// Load the embedded weights from memory and start a session.
    pub fn embedded(system_prompt: &str, tools_json: &str) -> Result<Self, NeedleError> {
        let handle = unsafe { cactus_load(NEEDLE3_CACT.as_ptr(), NEEDLE3_CACT.len() as u64) };
        if handle < 0 {
            return Err(NeedleError(format!("cactus_load: {}", last_error())));
        }
        let sp = CString::new(system_prompt).map_err(|e| NeedleError(e.to_string()))?;
        let tj = CString::new(tools_json).map_err(|e| NeedleError(e.to_string()))?;
        let tip = CString::new("").unwrap();
        let r = unsafe { cactus_init(sp.as_ptr(), tj.as_ptr(), tip.as_ptr()) };
        if r < 0 {
            return Err(NeedleError(format!("cactus_init: {}", last_error())));
        }
        Ok(Needle { handle })
    }

    /// Propose tool calls for an input. Returns the runtime's JSON reply.
    pub fn propose(&self, input: &str, max_tokens: i32) -> Result<String, NeedleError> {
        let inp = CString::new(input).map_err(|e| NeedleError(e.to_string()))?;
        let mut buf = vec![0u8; 8192];
        let r = unsafe {
            cactus_complete(
                inp.as_ptr(),
                std::ptr::null(),
                0,
                max_tokens,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as i32,
            )
        };
        if r < 0 {
            return Err(NeedleError(format!("cactus_complete: {}", last_error())));
        }
        let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..nul]).into_owned())
    }

    pub fn reset(&self) {
        unsafe { cactus_reset() }
    }

    pub fn handle(&self) -> i32 {
        self.handle
    }
}
