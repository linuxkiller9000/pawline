//! Loads the Moonshine runtime libraries from memory (memfd), after SHA-256
//! pin checks, and transcribes with a fresh transcriber per utterance.
//! Fails closed: no temp-file extraction fallback.
use crate::VoiceError;
use sha2::{Digest, Sha256};
use std::ffi::{c_char, c_int, c_void, CStr, CString};

extern "C" {
    fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(h: *mut c_void, s: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn write(fd: c_int, b: *const c_void, n: usize) -> isize;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
}
const MFD_CLOEXEC: u32 = 1;
const MFD_ALLOW_SEALING: u32 = 2;
const F_ADD_SEALS: c_int = 1033;
const SEALS: c_int = 0x1 | 0x2 | 0x4 | 0x8; // seal, shrink, grow, write
const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

pub struct PinnedLib<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub sha256_hex: &'a str,
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Verify every pin first; load nothing unless all match.
fn load_one(l: &PinnedLib) -> Result<*mut c_void, VoiceError> {
    let cn = CString::new(l.name).map_err(|_| VoiceError::LoaderUnavailable("bad name".into()))?;
    unsafe {
        let fd = memfd_create(cn.as_ptr(), MFD_CLOEXEC | MFD_ALLOW_SEALING);
        if fd < 0 {
            return Err(VoiceError::LoaderUnavailable("memfd_create refused".into()));
        }
        let mut off = 0;
        while off < l.bytes.len() {
            let n = write(fd, l.bytes[off..].as_ptr() as *const c_void, l.bytes.len() - off);
            if n <= 0 {
                close(fd);
                return Err(VoiceError::LoaderUnavailable("memfd write failed".into()));
            }
            off += n as usize;
        }
        // Seal so nothing (including this process) can change the bytes after hashing.
        if fcntl(fd, F_ADD_SEALS, SEALS) < 0 {
            close(fd);
            return Err(VoiceError::LoaderUnavailable("memfd sealing refused".into()));
        }
        let path = CString::new(format!("/proc/self/fd/{fd}")).unwrap();
        let h = dlopen(path.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
        if h.is_null() {
            let e = CStr::from_ptr(dlerror()).to_string_lossy().into_owned();
            return Err(VoiceError::LoaderUnavailable(e));
        }
        Ok(h)
    }
}

/// Mirror of `struct transcript_line_t` from moonshine-c-api.h (header version 30000,
/// library v0.1.6). Layout is asserted at compile time and, in tests, against the
/// pinned header with a real C compiler. The libs are also hash-pinned and the
/// library's reported version is checked before use.
#[repr(C)]
pub struct Line {
    pub text: *const c_char,
    pub audio: *const f32,
    pub audio_n: usize,
    pub start: f32,
    pub dur: f32,
    pub id: u64,
    pub flags: [i8; 5],
    pub speaker_spans: *const c_void,
    pub speaker_span_count: u64,
    pub latency_ms: u32,
    pub words: *const c_void,
    pub word_count: u64,
}
pub const LINE_SIZE: usize = std::mem::size_of::<Line>();
const _: () = {
    assert!(LINE_SIZE == 88);
    assert!(std::mem::offset_of!(Line, id) == 32);
    assert!(std::mem::offset_of!(Line, flags) == 40);
    assert!(std::mem::offset_of!(Line, speaker_spans) == 48);
    assert!(std::mem::offset_of!(Line, latency_ms) == 64);
    assert!(std::mem::offset_of!(Line, words) == 72);
};
/// Header version this crate was written against (MOONSHINE_HEADER_VERSION).
pub const HEADER_VERSION: i32 = 30000;
#[repr(C)]
struct Transcript {
    lines: *mut Line,
    line_count: u64,
}
type LoadFn = extern "C" fn(*const c_char, u32, *const c_void, u64, i32) -> i32;
type TransFn = extern "C" fn(i32, *mut f32, u64, i32, u32, *mut *mut Transcript) -> i32;
type FreeFn = extern "C" fn(i32) -> ();

pub struct Runtime {
    load: LoadFn,
    trans: TransFn,
    free: Option<FreeFn>,
}

impl Runtime {
    /// `libs` in dependency order: gcc_s, stdc++, onnxruntime, moonshine (last).
    pub fn load(libs: &[PinnedLib]) -> Result<Runtime, VoiceError> {
        for l in libs {
            if sha256_hex(l.bytes) != l.sha256_hex {
                return Err(VoiceError::HashMismatch { name: l.name.into() });
            }
        }
        let mut last = std::ptr::null_mut();
        for l in libs {
            last = load_one(l)?;
        }
        unsafe {
            let sym = |n: &str| {
                let c = CString::new(n).unwrap();
                dlsym(last, c.as_ptr())
            };
            let (a, b) = (sym("moonshine_load_transcriber_from_files"), sym("moonshine_transcribe_without_streaming"));
            if a.is_null() || b.is_null() {
                return Err(VoiceError::LoaderUnavailable("moonshine entry points missing".into()));
            }
            let ver = sym("moonshine_get_version");
            if ver.is_null() {
                return Err(VoiceError::LoaderUnavailable("moonshine version entry point missing".into()));
            }
            let v: extern "C" fn() -> i32 = std::mem::transmute(ver);
            if v() != HEADER_VERSION {
                return Err(VoiceError::LoaderUnavailable(format!("moonshine ABI version {} != {}", v(), HEADER_VERSION)));
            }
            let f = sym("moonshine_free_transcriber");
            Ok(Runtime {
                load: std::mem::transmute(a),
                trans: std::mem::transmute(b),
                free: if f.is_null() { None } else { Some(std::mem::transmute(f)) },
            })
        }
    }

    /// Fresh transcriber per utterance (reusing a handle returned "Mm" in tests).
    pub fn transcribe(&self, model_dir: &str, samples: &[f32]) -> Result<String, VoiceError> {
        let cm = CString::new(model_dir).map_err(|_| VoiceError::Model("bad model path".into()))?;
        let h = (self.load)(cm.as_ptr(), 2, std::ptr::null(), 0, HEADER_VERSION);
        if h < 0 {
            return Err(VoiceError::Model(format!("model load failed ({h})")));
        }
        let mut pcm = samples.to_vec();
        let mut t: *mut Transcript = std::ptr::null_mut();
        let e = (self.trans)(h, pcm.as_mut_ptr(), pcm.len() as u64, 16000, 0, &mut t);
        let out = unsafe {
            if e != 0 {
                Err(VoiceError::Model(format!("transcribe failed ({e})")))
            } else if t.is_null() || (*t).line_count == 0 {
                Ok(String::new())
            } else {
                let mut s = String::new();
                for i in 0..(*t).line_count as usize {
                    // lines are laid out as an array of the C struct; stride is the full C size.
                    let base = (*t).lines as *const u8;
                    let line = base.add(i * LINE_SIZE) as *const Line;
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&CStr::from_ptr((*line).text).to_string_lossy());
                }
                Ok(s.trim().to_string())
            }
        };
        if let Some(f) = self.free {
            f(h);
        }
        out
    }
}
