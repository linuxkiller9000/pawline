//! FFI to the Cactus runtime with Needle3 weights compiled into the binary.
//! No model file exists anywhere at runtime: the bytes are a static blob.
//!
//! These proposals are ADVISORY. Callers must route them through
//! pawline-core validation before anything executes.

pub mod assist;

use std::ffi::{CStr, CString};

#[allow(non_camel_case_types)]
type c_char = std::os::raw::c_char;

#[cfg(not(windows))]
extern "C" {
    fn cactus_load(cact: *const u8, n: u64) -> i32;
    fn cactus_init(system_prompt: *const c_char, tools_json: *const c_char, tool_index_path: *const c_char) -> i32;
    fn cactus_complete(input: *const c_char, pcm: *const f32, samples: i32, max_new_tokens: i32, out: *mut c_char, out_capacity: i32) -> i32;
    fn cactus_last_error() -> *const c_char;
    fn cactus_reset();
}

/// The weights, baked in. Apache-2.0 (Cactus-Compute/needle3); see NOTICE.
#[cfg(not(windows))]
pub static NEEDLE3_CACT: &[u8] = include_bytes!("../needle3.cact");
/// Windows: no embedded assist (the runtime is Linux-only for now), so there are no weights to carry.
#[cfg(windows)]
pub static NEEDLE3_CACT: &[u8] = &[];

#[cfg(windows)]
unsafe fn cactus_load(_c: *const u8, _n: u64) -> i32 {
    -1
}
#[cfg(windows)]
unsafe fn cactus_init(_s: *const c_char, _t: *const c_char, _i: *const c_char) -> i32 {
    -1
}
#[cfg(windows)]
unsafe fn cactus_complete(_i: *const c_char, _p: *const f32, _s: i32, _m: i32, _o: *mut c_char, _c: i32) -> i32 {
    -1
}
#[cfg(windows)]
unsafe fn cactus_last_error() -> *const c_char {
    b"the embedded assist is not available on Windows\0".as_ptr() as *const c_char
}
#[cfg(windows)]
unsafe fn cactus_reset() {}

/// The cactus runtime keeps ONE process-global session: cactus_complete/cactus_reset take no handle,
/// and a second cactus_load frees the first session's arena (observed under strace: a second load
/// munmaps the 35 MB weights copy and remaps it). Two concurrent Needle users therefore corrupt each
/// other: a load frees the arena another thread's inference is reading (SIGSEGV observed in the
/// bridge test suite at --test-threads 4), an init vanishes under a caller that just made it
/// ("cactus_init not called"), and the corruption can surface as a foreign C++ exception Rust cannot
/// catch (abort). Every FFI call is serialized on this lock and the weights load at most once per
/// process; later sessions reuse them after a reset.
static CACTUS: std::sync::Mutex<()> = std::sync::Mutex::new(());
static CACTUS_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// try_lock wrapper: a still-running (possibly abandoned, see TimedAssist) inference or load holds
/// the lock, and loading under it would free the arena it reads. Busy is a graceful error, never a wait.
fn cactus_lock() -> Result<std::sync::MutexGuard<'static, ()>, NeedleError> {
    CACTUS.try_lock().map_err(|_| NeedleError("the local model is busy with another request".into()))
}

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
    /// Start a session on the embedded weights. The weights load on the first call and are reused
    /// after that (a second cactus_load would free the arena under any inference still reading it);
    /// a reused session is reset first, so no earlier conversation leaks into the new one.
    pub fn embedded(system_prompt: &str, tools_json: &str) -> Result<Self, NeedleError> {
        let _g = cactus_lock()?;
        let mut handle = 0;
        if !CACTUS_LOADED.load(std::sync::atomic::Ordering::SeqCst) {
            handle = unsafe { cactus_load(NEEDLE3_CACT.as_ptr(), NEEDLE3_CACT.len() as u64) };
            if handle < 0 {
                return Err(NeedleError(format!("cactus_load: {}", last_error())));
            }
            CACTUS_LOADED.store(true, std::sync::atomic::Ordering::SeqCst);
        } else {
            unsafe { cactus_reset() }
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

    /// Test-only: try to load ARBITRARY bytes as if they were the weights, so the corruption
    /// test can probe the runtime's validation without touching the known-good embedded bytes.
    #[cfg(not(windows))]
    #[doc(hidden)]
    pub fn probe_load_for_test(bytes: &[u8]) -> Result<(), NeedleError> {
        let _g = cactus_lock()?;
        let h = unsafe { cactus_load(bytes.as_ptr(), bytes.len() as u64) };
        if h < 0 {
            return Err(NeedleError(format!("cactus_load: {}", last_error())));
        }
        // Subprocess-isolated probes only: this replaced the process-global session, so mark it loaded
        // to keep a later embedded() in the same process from loading over an in-flight call.
        CACTUS_LOADED.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// Propose tool calls for an input. Returns the runtime's JSON reply.
    pub fn propose(&self, input: &str, max_tokens: i32) -> Result<String, NeedleError> {
        let _g = cactus_lock()?;
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
        if let Ok(_g) = cactus_lock() {
            unsafe { cactus_reset() }
        }
    }

    pub fn handle(&self) -> i32 {
        self.handle
    }
}
