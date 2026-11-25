//! Voice input and spoken replies, built only from local free tools that are already installed.
//! Privacy rules, enforced by the code below:
//!  - nothing records until the user presses the microphone button; the recorder is a visible child process
//!  - audio goes to one private temp file (mode 0600) that is deleted after transcription or cancel
//!  - the transcript only lands in the draft box; it is never sent without the user pressing Enter
//!  - no cloud: only local commands run; Pawline downloads nothing and uploads nothing
//!  - spoken replies are off until switched on in Settings

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_RECORD: Duration = Duration::from_secs(60);
pub const STT_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
}

fn split_env(s: &str) -> Option<Cmd> {
    let mut it = s.split_whitespace().map(String::from);
    let program = it.next()?;
    Some(Cmd { program, args: it.collect() })
}

/// The recorder command for writing a WAV to `out`. `PAWLINE_REC_CMD` overrides detection (program and fixed args;
/// the file path is appended). Detection order: parecord, pw-record, arecord.
pub fn recorder(env_override: Option<&str>, has: &dyn Fn(&str) -> bool, out: &Path) -> Result<Cmd, String> {
    let o = out.to_string_lossy().to_string();
    if let Some(mut c) = env_override.and_then(split_env) {
        c.args.push(o);
        return Ok(c);
    }
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if has("parecord") {
        let mut args = a(&["--channels=1", "--rate=16000", "--format=s16le", "--file-format=wav"]);
        args.push(o);
        return Ok(Cmd { program: "parecord".into(), args });
    }
    if has("pw-record") {
        let mut args = a(&["--rate", "16000", "--channels", "1"]);
        args.push(o);
        return Ok(Cmd { program: "pw-record".into(), args });
    }
    if has("arecord") {
        let mut args = a(&["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav"]);
        args.push(o);
        return Ok(Cmd { program: "arecord".into(), args });
    }
    Err("No recorder found. Install one of: parecord (PulseAudio), pw-record (PipeWire) or arecord (ALSA).".into())
}

/// The speech-to-text command for the WAV at `wav`. `PAWLINE_STT_CMD` overrides (file path appended; it must print the
/// text on stdout). Otherwise whisper.cpp (whisper-cli) with a model file the user already has.
pub fn stt(env_override: Option<&str>, has: &dyn Fn(&str) -> bool, model: Option<PathBuf>, wav: &Path) -> Result<Cmd, String> {
    let w = wav.to_string_lossy().to_string();
    if let Some(mut c) = env_override.and_then(split_env) {
        c.args.push(w);
        return Ok(c);
    }
    let bin = ["whisper-cli", "whisper-cpp"].into_iter().find(|b| has(b));
    match (bin, model) {
        (Some(b), Some(m)) => Ok(Cmd { program: b.into(), args: vec!["-m".into(), m.to_string_lossy().to_string(), "-f".into(), w, "-nt".into(), "-np".into()] }),
        (Some(_), None) => Err("whisper.cpp is installed but no model file was found. Put a ggml model (for example ggml-base.bin) in ~/.local/share/pawline/models/ or set PAWLINE_WHISPER_MODEL. Pawline does not download models.".into()),
        _ => Err("No speech-to-text engine found. Install whisper.cpp (whisper-cli) with a ggml model, or set PAWLINE_STT_CMD to a local command that prints the text for a WAV file. Pawline never uses an online service for this.".into()),
    }
}

/// First ggml model in the user's own models folder.
pub fn find_model(env: Option<&str>, home: &Path) -> Option<PathBuf> {
    if let Some(e) = env {
        let p = PathBuf::from(e);
        return p.is_file().then_some(p);
    }
    let dir = home.join(".local/share/pawline/models");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "bin") && p.is_file()).collect();
    v.sort();
    v.into_iter().next()
}

/// The text-to-speech command; the text is written to its stdin. `PAWLINE_TTS_CMD` overrides detection.
pub fn tts(env_override: Option<&str>, has: &dyn Fn(&str) -> bool) -> Result<Cmd, String> {
    if let Some(c) = env_override.and_then(split_env) {
        return Ok(c);
    }
    if has("espeak-ng") {
        return Ok(Cmd { program: "espeak-ng".into(), args: vec!["--stdin".into()] });
    }
    if has("espeak") {
        return Ok(Cmd { program: "espeak".into(), args: vec!["--stdin".into()] });
    }
    if has("flite") {
        return Ok(Cmd { program: "flite".into(), args: vec!["-f".into(), "/dev/stdin".into()] });
    }
    Err("No local text-to-speech engine found. Install espeak-ng (or flite), or set PAWLINE_TTS_CMD to a local command that reads text on stdin and speaks it.".into())
}

/// Plain text for the speaker: Markdown marks, links and the timing line removed, shortened. Confirm cards and file
/// listings are never spoken by the caller (it only passes answers).
pub fn clean_for_speech(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || l.is_empty() || l.starts_with("Took ") {
            continue;
        }
        let l = l.trim_start_matches(['#', '>', '-', '*', '+', ' ']);
        let mut s = String::new();
        let mut skip_url = false;
        let mut after_bracket = false;
        for ch in l.chars() {
            let was = after_bracket;
            after_bracket = ch == ']';
            match ch {
                '*' | '`' | '_' | '#' => {}
                '(' if was => skip_url = true,
                ')' if skip_url => skip_url = false,
                '[' | ']' => {}
                _ if skip_url => {}
                c => s.push(c),
            }
        }
        out.push_str(s.trim());
        out.push(' ');
    }
    let out = out.trim().to_string();
    if out.chars().count() > 400 {
        format!("{}.", out.chars().take(400).collect::<String>().trim_end())
    } else {
        out
    }
}

pub fn has_in_path(bin: &str) -> bool {
    std::env::var_os("PATH").map_or(false, |p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// A private temp WAV path (not created yet): runtime dir if present, else temp dir.
pub fn temp_wav() -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    base.join(format!("pawline-voice-{}-{}.wav", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)))
}

/// Start the recorder. The file is created private (0600) before the recorder opens it.
pub fn start_recording(cmd: &Cmd, out: &Path) -> Result<Child, String> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(out).map_err(|e| format!("Could not create the private audio file: {e}"))?;
    Command::new(&cmd.program).args(&cmd.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| {
        let _ = std::fs::remove_file(out);
        format!("Could not start {}: {e}", cmd.program)
    })
}

/// Ask the recorder to finish the file (SIGINT, so the WAV header is completed) and wait for it.
pub fn stop_recording(mut child: Child) {
    let _ = Command::new("kill").args(["-INT", &child.id().to_string()]).status();
    let t = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if t.elapsed() < Duration::from_secs(3) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}

/// Stop at once and throw the audio away.
pub fn cancel_recording(mut child: Child, out: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(out);
}

/// Run the STT command on the WAV; the WAV is deleted whatever happens. Returns the cleaned transcript.
pub fn transcribe(cmd: &Cmd, wav: &Path, timeout: Duration) -> Result<String, String> {
    let r = run_capture(cmd, timeout);
    let _ = std::fs::remove_file(wav);
    let raw = r?;
    let text = clean_transcript(&raw);
    if text.is_empty() {
        Err("I did not hear any words. Nothing was added.".into())
    } else {
        Ok(text)
    }
}

fn run_capture(cmd: &Cmd, timeout: Duration) -> Result<String, String> {
    use std::io::Read;
    let mut child = Command::new(&cmd.program).args(&cmd.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("Could not start {}: {e}", cmd.program))?;
    let mut so = child.stdout.take().ok_or("no output pipe")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = (&mut so).take(64 * 1024).read_to_end(&mut b);
        let _ = tx.send(b);
    });
    let t = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let b = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
                if !st.success() {
                    return Err(format!("{} failed (exit {}). Nothing was added.", cmd.program, st.code().unwrap_or(-1)));
                }
                return Ok(String::from_utf8_lossy(&b).to_string());
            }
            Ok(None) if t.elapsed() < timeout => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} took too long and was stopped. Nothing was added.", cmd.program));
            }
        }
    }
}

/// Engines print markers like [BLANK_AUDIO] or timestamps; keep just the words, on one line, shortened.
pub fn clean_transcript(raw: &str) -> String {
    let mut s = String::new();
    let mut depth = 0;
    for ch in raw.chars() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            c if depth == 0 => s.push(if c.is_control() { ' ' } else { c }),
            _ => {}
        }
    }
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    t.chars().take(500).collect()
}

/// Start speaking (text on stdin). The caller keeps the child so a Stop control can kill it.
pub fn speak(cmd: &Cmd, text: &str) -> Result<Child, String> {
    use std::io::Write;
    let mut child = Command::new(&cmd.program).args(&cmd.args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("Could not start {}: {e}", cmd.program))?;
    if let Some(mut si) = child.stdin.take() {
        let t = text.to_string();
        std::thread::spawn(move || {
            let _ = si.write_all(t.as_bytes());
        });
    }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_detection_order_and_missing_message() {
        let out = Path::new("/run/x.wav");
        let only = |name: &'static str| move |b: &str| b == name;
        assert_eq!(recorder(None, &only("parecord"), out).unwrap().program, "parecord");
        assert_eq!(recorder(None, &only("pw-record"), out).unwrap().program, "pw-record");
        let a = recorder(None, &only("arecord"), out).unwrap();
        assert_eq!((a.program.as_str(), a.args.last().unwrap().as_str()), ("arecord", "/run/x.wav"));
        let e = recorder(None, &|_| false, out).unwrap_err();
        assert!(e.contains("parecord") && e.contains("arecord"));
        let o = recorder(Some("myrec --fast"), &|_| false, out).unwrap();
        assert_eq!((o.program.as_str(), o.args), ("myrec", vec!["--fast".to_string(), "/run/x.wav".to_string()]));
    }

    #[test]
    fn stt_needs_engine_and_model_and_never_names_a_cloud_service() {
        let wav = Path::new("/run/x.wav");
        let e = stt(None, &|_| false, None, wav).unwrap_err();
        assert!(e.contains("whisper") && e.contains("PAWLINE_STT_CMD") && e.contains("never uses an online"));
        let e = stt(None, &|b| b == "whisper-cli", None, wav).unwrap_err();
        assert!(e.contains("no model file") && e.contains("does not download"));
        let c = stt(None, &|b| b == "whisper-cli", Some("/m/ggml-base.bin".into()), wav).unwrap();
        assert_eq!(c.args, ["-m", "/m/ggml-base.bin", "-f", "/run/x.wav", "-nt", "-np"]);
        let c = stt(Some("my-stt --lang en"), &|_| false, None, wav).unwrap();
        assert_eq!(c.args, ["--lang", "en", "/run/x.wav"]);
    }

    #[test]
    fn tts_detection_and_missing_message() {
        assert_eq!(tts(None, &|b| b == "espeak-ng").unwrap().program, "espeak-ng");
        assert_eq!(tts(None, &|b| b == "flite").unwrap().program, "flite");
        assert!(tts(None, &|_| false).unwrap_err().contains("espeak-ng"));
        assert_eq!(tts(Some("say-it"), &|_| false).unwrap().program, "say-it");
    }

    #[test]
    fn speech_text_is_plain_short_and_without_links_or_timing() {
        let s = clean_for_speech("## Title\n- **Files**: make `a folder` [docs](http://x.example/y)\n```\ncode\n```\nTook 0.1s");
        assert_eq!(s, "Title Files: make a folder docs");
        let long = "word ".repeat(300);
        assert!(clean_for_speech(&long).chars().count() <= 401);
    }

    #[test]
    fn transcript_cleanup_drops_markers_and_keeps_words() {
        assert_eq!(clean_transcript("[BLANK_AUDIO]\n"), "");
        assert_eq!(clean_transcript(" [00:00:00.000 --> 00:00:02.000]   make a folder\n called kitten (music) \n"), "make a folder called kitten");
    }

    fn script(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("pawline-vt-{}-{name}", std::process::id()));
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn record_stop_transcribe_end_to_end_with_fixture_tools_and_cleanup() {
        // fixture recorder writes known bytes to its file argument, then waits for SIGINT like parecord does
        let rec = script("rec", "echo RIFFfake > \"$1\"\nsleep 30 &\np=$!\ntrap 'kill $p; exit 0' INT\nwait");
        let stt_s = script("stt", "test -s \"$1\" && echo ' make a folder called kitten '");
        let wav = temp_wav();
        let c = recorder(Some(&rec.to_string_lossy()), &|_| false, &wav).unwrap();
        let child = start_recording(&c, &wav).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&wav).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "private audio file");
        }
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        stop_recording(child);
        assert!(t.elapsed() < Duration::from_secs(2), "stopped promptly");
        let sc = stt(Some(&stt_s.to_string_lossy()), &|_| false, None, &wav).unwrap();
        let text = transcribe(&sc, &wav, Duration::from_secs(5)).unwrap();
        assert_eq!(text, "make a folder called kitten");
        assert!(!wav.exists(), "audio deleted after transcription");
    }

    #[test]
    fn cancel_discards_audio_and_failures_leave_no_file() {
        let rec = script("rec2", "echo RIFFfake > \"$1\"\nsleep 30 &\np=$!\ntrap 'kill $p; exit 0' INT\nwait");
        let wav = temp_wav();
        let c = recorder(Some(&rec.to_string_lossy()), &|_| false, &wav).unwrap();
        let child = start_recording(&c, &wav).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        cancel_recording(child, &wav);
        assert!(!wav.exists());
        // missing recorder program: file is removed again
        let w2 = temp_wav();
        let bad = Cmd { program: "/nonexistent/recorder".into(), args: vec![w2.to_string_lossy().to_string()] };
        assert!(start_recording(&bad, &w2).is_err() && !w2.exists());
        // failing and silent engines: error, audio deleted
        let w3 = temp_wav();
        std::fs::write(&w3, b"x").unwrap();
        let fail = Cmd { program: script("fail", "exit 3").to_string_lossy().to_string(), args: vec![] };
        assert!(transcribe(&fail, &w3, Duration::from_secs(5)).unwrap_err().contains("failed"));
        assert!(!w3.exists());
        let w4 = temp_wav();
        std::fs::write(&w4, b"x").unwrap();
        let silent = Cmd { program: script("silent", "echo '[BLANK_AUDIO]'").to_string_lossy().to_string(), args: vec![] };
        assert!(transcribe(&silent, &w4, Duration::from_secs(5)).unwrap_err().contains("did not hear"));
        // a hung engine is stopped by the timeout
        let w5 = temp_wav();
        std::fs::write(&w5, b"x").unwrap();
        let hang = Cmd { program: script("hang", "sleep 30").to_string_lossy().to_string(), args: vec![] };
        let t = Instant::now();
        assert!(transcribe(&hang, &w5, Duration::from_millis(200)).unwrap_err().contains("too long"));
        assert!(t.elapsed() < Duration::from_secs(3) && !w5.exists());
    }

    #[test]
    fn speak_sends_text_on_stdin_and_can_be_killed() {
        let out = std::env::temp_dir().join(format!("pawline-spoken-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let s = script("spk", &format!("cat > {}", out.display()));
        let c = tts(Some(&s.to_string_lossy()), &|_| false).unwrap();
        let mut ch = speak(&c, "hello there").unwrap();
        ch.wait().unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello there");
        let long = script("spk2", "sleep 30");
        let mut ch = speak(&Cmd { program: long.to_string_lossy().to_string(), args: vec![] }, "x").unwrap();
        ch.kill().unwrap();
        ch.wait().unwrap();
    }
}
