//! "shrink/compress <video>": make a smaller copy of one video with ffmpeg, next to the original.
//! The original is never touched. Output is written to a unique hidden temporary file, checked (it must
//! play back, have about the same length and be smaller), and only then published under its final name
//! with an atomic no-replace rename. A cancelled, failed or no-gain run removes its temporary file.
use std::path::{Path, PathBuf};

pub const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "mov", "webm", "avi", "m4v", "wmv", "flv"];
pub const TIMEOUT_SECS: u64 = 3600;

pub fn is_video(p: &Path) -> bool {
    p.extension().map(|e| VIDEO_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

/// "shrink video X", "compress my video X", "shrink X", ... -> the words after the verb.
pub fn parse(input: &str) -> Option<String> {
    let t = input.trim().trim_end_matches(['.', '!']);
    let low = t.to_lowercase();
    for p in [
        "shrink my video ", "shrink the video ", "shrink video ", "compress my video ", "compress the video ", "compress video ",
        "shrink my ", "shrink the ", "shrink ", "compress my ", "compress the ", "compress ", "make smaller ", "make the video smaller ",
    ] {
        if low.starts_with(p) {
            let rest = t[p.len()..].trim();
            return if rest.is_empty() { None } else { Some(rest.to_string()) };
        }
    }
    None
}

/// Candidate video files for the words, looking only at the top level of a few well-known folders under `home`
/// (or at the explicit path). Never follows symlinks. Several matches means the caller must ask.
pub fn find(home: &Path, words: &str) -> Vec<PathBuf> {
    let w = words.trim().trim_matches(|c| c == '"' || c == '`');
    let direct = if let Some(rest) = w.strip_prefix("~/") { Some(home.join(rest)) } else if w.starts_with('/') { Some(PathBuf::from(w)) } else { None };
    if let Some(p) = direct {
        return match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() && is_video(&p) => std::fs::canonicalize(&p).ok().filter(|c| c.starts_with(home)).into_iter().collect(),
            _ => vec![],
        };
    }
    let want: Vec<String> = w.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
    if want.is_empty() {
        return vec![];
    }
    let mut out = vec![];
    for d in ["Videos", "Downloads", "Desktop", "Movies", ""] {
        let dir = if d.is_empty() { home.to_path_buf() } else { home.join(d) };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file() && !t.is_symlink()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| is_video(p) && !is_output_name(p))
            .filter(|p| {
                let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                let mut at = 0;
                want.iter().all(|k| match n[at..].find(k.as_str()) {
                    Some(i) => { at += i + k.len(); true }
                    None => false,
                })
            })
            .collect();
        found.sort();
        out.extend(found);
    }
    out.sort();
    out.dedup();
    // an exact file-name match beats partial ones
    let exact: Vec<PathBuf> = out.iter().filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase() == w.to_lowercase()).unwrap_or(false)).cloned().collect();
    if exact.len() == 1 { exact } else { out }
}

fn is_output_name(p: &Path) -> bool {
    p.file_stem().map(|s| s.to_string_lossy().ends_with("-small") || s.to_string_lossy().contains("-small-")).unwrap_or(false)
}

/// `<stem>-small.mp4` next to the original; `-small-2`, ... when taken.
pub fn output_for(from: &Path) -> PathBuf {
    let dir = from.parent().unwrap_or(Path::new("/"));
    let stem = from.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}-small.mp4") } else { format!("{stem}-small-{n}.mp4") };
        let p = dir.join(name);
        if std::fs::symlink_metadata(&p).is_err() {
            return p;
        }
        n += 1;
    }
}

pub fn find_exe(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| {
        std::fs::metadata(p).map(|m| crate::platform::is_executable(&m)).unwrap_or(false)
    })
}

/// The streams the copy keeps: the first video stream and EVERY audio stream. Subtitles, data/attachment streams
/// and any further video streams are dropped. Chapters/metadata are not carried over by this command line.
pub const KEEPS: &str = "Keeps the first video stream and all audio streams. Drops subtitles, extra video streams, attachments and chapters.";

/// Fixed ffmpeg arguments. No user text at all: the input is the source file handed over as stdin and the output is
/// the temporary file handed over as stdout, so the process works on exactly the two files this program opened.
pub fn ffmpeg_args() -> Vec<std::ffi::OsString> {
    ["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", "/dev/fd/0",
     "-map", "0:v:0", "-map", "0:a?", "-sn", "-dn", "-map_chapters", "-1", "-c:v", "libx264", "-crf", "28", "-preset", "medium", "-pix_fmt", "yuv420p",
     "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart", "-f", "mp4", "/dev/fd/1"]
        .iter().map(|s| (*s).into()).collect()
}

/// Run a helper program on an already-open file (as its stdin, read through /dev/fd/0), bounded in time and output.
/// `cancelled` is polled. Returns (success, stdout up to 64 KB), or None when stopped for time or cancel.
pub fn run_bounded(exe: &Path, args: &[&str], stdin: std::fs::File, stdout: Option<std::fs::File>, limit: std::time::Duration, cancelled: &dyn Fn() -> bool) -> Result<(bool, Vec<u8>), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let capture = stdout.is_none();
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::from(stdin))
        .stdout(match stdout { Some(f) => Stdio::from(f), None => Stdio::piped() })
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
    let mut reader = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    if capture {
        if let Some(mut r) = reader.take() {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = (&mut r).take(65536).read_to_end(&mut buf);
                let _ = std::io::copy(&mut r, &mut std::io::sink());
                let _ = tx.send(buf);
            });
        }
    }
    let t0 = std::time::Instant::now();
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        if t0.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timeout".into());
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                let out = if capture { rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default() } else { vec![] };
                return Ok((st.success(), out));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("lost track of {}: {e}", exe.display()));
            }
        }
    }
}

/// (duration seconds, video stream count, audio stream count) from ffprobe, reading the open file.
pub fn probe(ffprobe: &Path, f: std::fs::File, cancelled: &dyn Fn() -> bool) -> Option<(f64, usize, usize)> {
    let (ok, out) = run_bounded(ffprobe, &["-v", "error", "-show_entries", "format=duration:stream=codec_type", "-of", "csv=p=0", "/dev/fd/0"], f, None, std::time::Duration::from_secs(30), cancelled).ok()?;
    if !ok {
        return None;
    }
    let text = String::from_utf8_lossy(&out).to_string();
    let (mut v, mut a, mut dur) = (0, 0, None);
    for l in text.lines() {
        match l.trim() {
            "video" => v += 1,
            "audio" => a += 1,
            x => {
                if let Ok(d) = x.parse::<f64>() {
                    dur = Some(d);
                }
            }
        }
    }
    dur.filter(|d| *d > 0.0).map(|d| (d, v, a))
}

/// Decode the whole file; true only if ffmpeg reads every frame without an error.
pub fn decodes_fully(ffmpeg: &Path, f: std::fs::File, limit: std::time::Duration, cancelled: &dyn Fn() -> bool) -> Result<bool, String> {
    let nul = std::fs::OpenOptions::new().write(true).open("/dev/null").map_err(|e| e.to_string())?;
    run_bounded(ffmpeg, &["-nostdin", "-v", "error", "-xerror", "-i", "/dev/fd/0", "-f", "null", "-"], f, Some(nul), limit, cancelled).map(|r| r.0)
}

pub fn mb(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_048_576.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_names() {
        assert_eq!(parse("shrink video holiday.mp4").as_deref(), Some("holiday.mp4"));
        assert_eq!(parse("compress my trip").as_deref(), Some("trip"));
        assert_eq!(parse("compress"), None);
        assert!(is_video(Path::new("/a/B.MKV")) && !is_video(Path::new("/a/b.txt")));
        let a = ffmpeg_args();
        assert_eq!(a[a.len() - 1], std::ffi::OsString::from("/dev/fd/1"));
    }
}
