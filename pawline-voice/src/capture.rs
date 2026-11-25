//! Bounded push-to-talk capture over the PulseAudio native protocol
//! (served by PulseAudio and by PipeWire-pulse). Pure Rust, no audio lib linked.
use crate::VoiceError;
use pulseaudio::protocol as p;
use std::ffi::CString;
use std::io::{BufReader, Read};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RATE: u32 = 16_000;
/// Hard sample cap, independent of the wall-clock cap.
pub const MAX_SECS: u32 = 15;
pub const MAX_SAMPLES: usize = (RATE * MAX_SECS) as usize;

pub struct Limits {
    pub wall: Duration,
    pub samples: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Limits { wall: Duration::from_secs(MAX_SECS as u64), samples: MAX_SAMPLES }
    }
}
impl Limits {
    fn clamped(&self) -> Limits {
        Limits { wall: self.wall.min(Duration::from_secs(MAX_SECS as u64)), samples: self.samples.min(MAX_SAMPLES) }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StopReason {
    Released,
    WallCap,
    SampleCap,
    DeviceError,
}

pub struct Recording {
    pub samples: Vec<f32>,
    pub reason: StopReason,
}

pub struct Capture {
    ctl: UnixStream,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    out: Arc<Mutex<Option<Recording>>>,
}

fn connect() -> Result<(BufReader<UnixStream>, u16, UnixStream), VoiceError> {
    let path = pulseaudio::socket_path_from_env().ok_or_else(|| VoiceError::NoAudioServer("no PulseAudio socket".into()))?;
    let raw = UnixStream::connect(path).map_err(|e| VoiceError::NoAudioServer(e.to_string()))?;
    let ctl = raw.try_clone().map_err(|e| VoiceError::Audio(e.to_string()))?;
    let mut s = BufReader::new(raw);
    let cookie = pulseaudio::cookie_path_from_env().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    let ae = |e: &dyn std::fmt::Display| VoiceError::Audio(e.to_string());
    p::write_command_message(
        s.get_mut(),
        0,
        &p::Command::Auth(p::AuthParams { version: p::MAX_VERSION, supports_shm: false, supports_memfd: false, cookie }),
        p::MAX_VERSION,
    )
    .map_err(|e| ae(&e))?;
    let (_, r) = p::read_reply_message::<p::AuthReply>(&mut s, p::MAX_VERSION).map_err(|e| ae(&e))?;
    let v = r.version.min(p::MAX_VERSION);
    let mut props = p::Props::new();
    props.set(p::Prop::ApplicationName, CString::new("pawline-voice").unwrap());
    p::write_command_message(s.get_mut(), 1, &p::Command::SetClientName(props), v).map_err(|e| ae(&e))?;
    p::read_reply_message::<p::SetClientNameReply>(&mut s, v).map_err(|e| ae(&e))?;
    Ok((s, v, ctl))
}

impl Capture {
    /// Opens the mic stream. The caller must only call this while the
    /// push-to-talk key is held.
    pub fn start(limits: Limits) -> Result<Capture, VoiceError> {
        let lim = limits.clamped();
        let (mut sock, v, ctl) = connect()?;
        let ae = |e: &dyn std::fmt::Display| VoiceError::Audio(e.to_string());
        p::write_command_message(
            sock.get_mut(),
            10,
            &p::Command::CreateRecordStream(p::RecordStreamParams {
                sample_spec: p::SampleSpec { format: p::SampleFormat::S16Le, channels: 1, sample_rate: RATE },
                channel_map: p::ChannelMap::mono(),
                ..Default::default()
            }),
            v,
        )
        .map_err(|e| ae(&e))?;
        let (_, rs) = p::read_reply_message::<p::CreateRecordStreamReply>(&mut sock, v).map_err(|e| ae(&e))?;
        if rs.sample_spec.format != p::SampleFormat::S16Le || rs.sample_spec.channels != 1 || rs.sample_spec.sample_rate != RATE {
            let _ = ctl.shutdown(Shutdown::Both);
            return Err(VoiceError::Audio("audio server returned an unexpected format".into()));
        }
        let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
        let reader = std::thread::spawn(move || loop {
            let Ok(d) = p::read_descriptor(&mut sock) else { let _ = tx.send(None); break };
            let mut b = vec![0u8; (d.length as usize).min(1 << 20)];
            if d.length as usize != b.len() || sock.read_exact(&mut b).is_err() {
                let _ = tx.send(None);
                break;
            }
            if d.channel != u32::MAX && tx.send(Some(b)).is_err() {
                break;
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let out = Arc::new(Mutex::new(None));
        let (stop2, out2, ctl2) = (stop.clone(), out.clone(), ctl.try_clone().map_err(|e| ae(&e))?);
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + lim.wall;
            let mut bytes: Vec<u8> = Vec::new();
            let max_bytes = lim.samples * 2;
            let reason = loop {
                let mut dead = false;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Some(b) => {
                            let room = max_bytes.saturating_sub(bytes.len());
                            bytes.extend_from_slice(&b[..b.len().min(room)]);
                        }
                        None => dead = true,
                    }
                }
                if stop2.load(Ordering::SeqCst) {
                    break StopReason::Released;
                }
                if bytes.len() >= max_bytes {
                    break StopReason::SampleCap;
                }
                if Instant::now() >= deadline {
                    break StopReason::WallCap;
                }
                if dead {
                    break StopReason::DeviceError;
                }
                std::thread::sleep(Duration::from_millis(15));
            };
            let _ = ctl2.shutdown(Shutdown::Both); // mic stream closed here
            let _ = reader.join();
            let samples = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
            *out2.lock().unwrap() = Some(Recording { samples, reason });
        });
        Ok(Capture { ctl, stop, worker: Some(worker), out })
    }

    /// Key released, focus lost or UI error: close the mic and return audio.
    pub fn stop(mut self) -> Recording {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        self.out.lock().unwrap().take().unwrap_or(Recording { samples: vec![], reason: StopReason::DeviceError })
    }
}

impl Drop for Capture {
    /// Dropping without `stop` (panic, window closed) still closes the mic.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.ctl.shutdown(Shutdown::Both);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
