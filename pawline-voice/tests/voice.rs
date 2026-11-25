use pawline_voice::capture::*;
use pawline_voice::runtime::*;
use pawline_voice::VoiceError;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn hash_mismatch_fails_closed_before_loading_anything() {
    let libs = [PinnedLib { name: "libx.so", bytes: b"not a library", sha256_hex: &"0".repeat(64) }];
    match Runtime::load(&libs) {
        Err(VoiceError::HashMismatch { name }) => assert_eq!(name, "libx.so"),
        _ => panic!("expected HashMismatch"),
    }
}

#[test]
fn matching_hash_but_not_elf_fails_closed_with_error() {
    let b = b"hello";
    let h = sha256_hex(b);
    let libs = [PinnedLib { name: "libx.so", bytes: b, sha256_hex: &h }];
    assert!(matches!(Runtime::load(&libs), Err(VoiceError::LoaderUnavailable(_))));
}

#[test]
fn sha256_known_vector() {
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn caps_are_clamped() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let l = Limits { wall: Duration::from_secs(999), samples: usize::MAX };
    // starting without a server must fail cleanly, never hang
    std::env::set_var("PULSE_SERVER", "unix:/nonexistent/pulse");
    assert!(matches!(Capture::start(l), Err(VoiceError::NoAudioServer(_))));
}

struct Daemon(Child, String);
impl Drop for Daemon { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
fn daemon(tag: &str) -> Option<Daemon> {
    let dir = format!("/tmp/pv-{tag}-{}", std::process::id());
    std::fs::create_dir_all(&dir).ok()?;
    let c = Command::new("pulseaudio")
        .env("XDG_RUNTIME_DIR", &dir)
        .args(["--daemonize=no", "-n", "--exit-idle-time=-1", "-L", "module-native-protocol-unix", "-L", "module-sine-source frequency=440 source_name=synth"])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    let sock = format!("{dir}/pulse/native");
    for _ in 0..50 { if std::path::Path::new(&sock).exists() { break } std::thread::sleep(Duration::from_millis(100)); }
    std::env::set_var("PULSE_SERVER", format!("unix:{sock}"));
    std::env::set_var("PULSE_SOURCE", "synth");
    Some(Daemon(c, dir))
}
fn open_streams() -> usize {
    let o = Command::new("pactl").arg("list").arg("short").arg("source-outputs").output().unwrap();
    String::from_utf8_lossy(&o.stdout).lines().count()
}

// One test owns the daemon (env vars are process-global).
#[test]
fn capture_release_wallcap_samplecap_and_drop_all_close_the_mic() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_d) = daemon("cap") else { eprintln!("SKIP: no pulseaudio"); return };
    // release
    let c = Capture::start(Limits::default()).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let r = c.stop();
    assert_eq!(r.reason, StopReason::Released);
    assert!(!r.samples.is_empty() && r.samples.len() <= MAX_SAMPLES);
    assert_eq!(open_streams(), 0, "mic stream left open after release");
    // wall-clock cap (independent of samples)
    let t = Instant::now();
    let c = Capture::start(Limits { wall: Duration::from_millis(700), samples: MAX_SAMPLES }).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let r = c.stop();
    assert_eq!(r.reason, StopReason::WallCap);
    assert!(t.elapsed() < Duration::from_secs(4));
    assert_eq!(open_streams(), 0);
    // sample cap (independent of wall clock)
    let c = Capture::start(Limits { wall: Duration::from_secs(15), samples: 8000 }).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let r = c.stop();
    assert_eq!(r.reason, StopReason::SampleCap);
    assert_eq!(r.samples.len(), 8000);
    assert_eq!(open_streams(), 0);
    // drop without stop (window closed / panic) closes the mic
    let c = Capture::start(Limits::default()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(open_streams(), 1);
    drop(c);
    assert_eq!(open_streams(), 0, "mic stream left open after drop");
}

// Needs the pinned libs and a model; set PAWLINE_VOICE_TEST_LIBS and _MODEL and _PCM to run.
#[test]
fn transcribes_with_fresh_transcriber_each_time() {
    let (Ok(dir), Ok(model), Ok(pcm)) = (std::env::var("PAWLINE_VOICE_TEST_LIBS"), std::env::var("PAWLINE_VOICE_TEST_MODEL"), std::env::var("PAWLINE_VOICE_TEST_PCM")) else { eprintln!("SKIP: env not set"); return };
    let read = |n: &str| std::fs::read(format!("{dir}/{n}")).unwrap();
    let names = ["libgcc_s.so.1", "libstdc++.so.6", "libonnxruntime.so.1", "libmoonshine.so"];
    let data: Vec<Vec<u8>> = names.iter().map(|n| read(n)).collect();
    let hashes: Vec<String> = data.iter().map(|b| sha256_hex(b)).collect();
    let libs: Vec<PinnedLib> = (0..4).map(|i| PinnedLib { name: names[i], bytes: &data[i], sha256_hex: &hashes[i] }).collect();
    let rt = Runtime::load(&libs).unwrap();
    let raw = std::fs::read(pcm).unwrap();
    let s: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let a = rt.transcribe(&model, &s).unwrap();
    let b = rt.transcribe(&model, &s).unwrap();
    assert!(a.to_lowercase().contains("best of times"), "{a}");
    assert_eq!(a, b);
}

// Compares the mirrored struct with the real header via a C compiler.
// Set PAWLINE_VOICE_TEST_HEADER_DIR to the dir containing moonshine-c-api.h.
#[test]
fn line_layout_matches_pinned_header() {
    let Ok(dir) = std::env::var("PAWLINE_VOICE_TEST_HEADER_DIR") else { eprintln!("SKIP: header dir not set"); return };
    let src = "/tmp/pv_layout.c";
    std::fs::write(src, "#include <stdio.h>\n#include <stdint.h>\n#include <stddef.h>\n#include \"moonshine-c-api.h\"\nint main(){printf(\"%zu %zu %zu %zu %zu\", sizeof(struct transcript_line_t), offsetof(struct transcript_line_t,id), offsetof(struct transcript_line_t,speaker_spans), offsetof(struct transcript_line_t,words), (size_t)MOONSHINE_HEADER_VERSION);}").unwrap();
    let st = Command::new("gcc").args(["-I", &dir, src, "-o", "/tmp/pv_layout"]).status().unwrap();
    assert!(st.success());
    let o = Command::new("/tmp/pv_layout").output().unwrap();
    let want = format!("{} {} {} {} {}", LINE_SIZE, 32, 48, 72, HEADER_VERSION);
    assert_eq!(String::from_utf8_lossy(&o.stdout), want);
}
