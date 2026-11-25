//! Optional voice input for Pawline.
//!
//! Safety contract (enforced here, tested in tests/):
//! * Voice is never started implicitly. `Capture::start` is the only thing
//!   that opens a microphone stream, and the stream is closed on `stop`,
//!   on drop, on a server error, when the wall-clock cap passes or when the
//!   sample cap is reached, whichever comes first.
//! * Audio lives in memory only. Nothing is written to disk.
//! * The output is plain text for the caller to show in a preview. This crate
//!   has no knowledge of plans or actions and never executes anything.
//! * Runtime libraries are loaded only from memory, only after their SHA-256
//!   matches a pinned value, and the load fails closed with `VoiceError`
//!   (no extract-to-disk fallback).
pub mod capture;
pub mod download;
pub mod runtime;

#[derive(Debug)]
pub enum VoiceError {
    /// memfd_create / dlopen refused (e.g. noexec memfd policy, seccomp).
    LoaderUnavailable(String),
    /// A library did not match its pinned hash; nothing was loaded.
    HashMismatch { name: String },
    NoAudioServer(String),
    Audio(String),
    Model(String),
}
impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceError::LoaderUnavailable(m) => write!(f, "voice unavailable: runtime could not be loaded ({m})"),
            VoiceError::HashMismatch { name } => write!(f, "voice unavailable: {name} did not match its pinned hash"),
            VoiceError::NoAudioServer(m) => write!(f, "voice unavailable: no audio server ({m})"),
            VoiceError::Audio(m) => write!(f, "audio error: {m}"),
            VoiceError::Model(m) => write!(f, "voice model error: {m}"),
        }
    }
}
impl std::error::Error for VoiceError {}
