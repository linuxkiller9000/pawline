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
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    })
}

/// Fixed ffmpeg arguments (no shell, no user text except two absolute paths after -i / at the end).
pub fn ffmpeg_args(src: &Path, tmp: &Path) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"].iter().map(|s| (*s).into()).collect();
    a.push(src.as_os_str().to_owned());
    for s in [
        "-map", "0:v:0", "-map", "0:a?", "-sn", "-dn", "-c:v", "libx264", "-crf", "28", "-preset", "medium", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart", "-f", "mp4",
    ] {
        a.push(s.into());
    }
    a.push(tmp.as_os_str().to_owned());
    a
}

/// Duration in seconds via ffprobe, None if it cannot be read.
pub fn probe_duration(ffprobe: &Path, f: &Path) -> Option<f64> {
    let o = std::process::Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(f)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok().filter(|d| *d > 0.0)
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
        let a = ffmpeg_args(Path::new("/h/a.mp4"), Path::new("/h/.t"));
        assert_eq!(a[a.len() - 1], std::ffi::OsString::from("/h/.t"));
    }
}
