//! Frequent folders for the "zi" command. Display only: nothing here opens, moves or changes anything.
//!
//! Source 1: zoxide's own ranking, read by running `zoxide query --list --score` (fixed arguments,
//! no shell, read only; Pawline never opens or writes zoxide's database file and never runs `zoxide add`).
//! Source 2 (when zoxide is not installed or answers nothing): Pawline's own small list of folders its
//! file steps touched, kept in a local 0600 JSON file (paths, a use count and a last-used time only).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_OWN: usize = 200;
pub const SHOWN: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub score: f64,
    pub path: PathBuf,
}

pub fn own_path() -> Option<PathBuf> {
    crate::searchmem::default_path().map(|p| p.with_file_name("frequent-folders.json"))
}

/// Parse `zoxide query --list --score` output: lines of "<score> <path>".
pub fn parse_zoxide(out: &str) -> Vec<Ranked> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (sc, rest) = l.split_once(char::is_whitespace)?;
            let score: f64 = sc.parse().ok()?;
            let path = rest.trim_start();
            if !path.starts_with('/') {
                return None;
            }
            Some(Ranked { score, path: PathBuf::from(path) })
        })
        .collect()
}

const ZOXIDE_WAIT_MS: u64 = 2000;
const ZOXIDE_MAX_BYTES: u64 = 256 * 1024;

/// zoxide's ranking, or None when it is not installed, fails or takes longer than 2 seconds.
/// Output is capped at 256 KB. Read only: no db access, no `zoxide add`.
pub fn zoxide_ranked() -> Option<Vec<Ranked>> {
    use std::io::Read;
    let mut child = std::process::Command::new("zoxide")
        .args(["query", "--list", "--score"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(ZOXIDE_MAX_BYTES).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let start = std::time::Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.success(),
            Ok(None) if start.elapsed().as_millis() as u64 > ZOXIDE_WAIT_MS => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => break false,
        }
    };
    if !ok {
        return None; // the reader thread is left to finish on its own; we never wait for it
    }
    let buf = rx.recv_timeout(std::time::Duration::from_millis(500)).ok()?;
    Some(parse_zoxide(&String::from_utf8_lossy(&buf)))
}

fn load_own(path: &Path) -> Vec<(PathBuf, u32, u64)> {
    let Some(Value::Array(a)) = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
        return vec![];
    };
    a.iter()
        .filter_map(|v| Some((PathBuf::from(v.get("path")?.as_str()?), v.get("uses")?.as_u64()? as u32, v.get("last")?.as_u64()?)))
        .collect()
}

fn save_own(path: &Path, all: &[(PathBuf, u32, u64)]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    let v: Vec<Value> = all.iter().map(|(p, u, l)| json!({"path": p.to_string_lossy(), "uses": u, "last": l})).collect();
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default())?;
    crate::platform::set_private_file(&tmp)?;
    std::fs::rename(&tmp, path)
}

/// Note folders Pawline just worked in (absolute paths only).
pub fn record_own(path: &Path, dirs: &[PathBuf], now: u64) {
    let mut all = load_own(path);
    for d in dirs.iter().filter(|d| d.is_absolute()) {
        match all.iter_mut().find(|e| &e.0 == d) {
            Some(e) => {
                e.1 += 1;
                e.2 = now;
            }
            None => all.push((d.clone(), 1, now)),
        }
    }
    if all.len() > MAX_OWN {
        all.sort_by_key(|e| std::cmp::Reverse(e.2));
        all.truncate(MAX_OWN);
    }
    let _ = save_own(path, &all);
}

fn own_ranked(path: &Path, now: u64) -> Vec<Ranked> {
    let mut v: Vec<Ranked> = load_own(path)
        .into_iter()
        .map(|(p, uses, last)| {
            let age = now.saturating_sub(last);
            let w = if age < 3600 { 4.0 } else if age < 86_400 { 2.0 } else if age < 604_800 { 0.5 } else { 0.25 };
            Ranked { score: uses as f64 * w, path: p }
        })
        .collect();
    v.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    v
}

/// Which source answered, and the ranked existing folders matching every keyword.
pub fn ranked(zoxide: Option<Vec<Ranked>>, own: &Path, now: u64, query: &str) -> (&'static str, Vec<Ranked>) {
    let (src, mut list) = match zoxide {
        Some(z) if !z.is_empty() => ("zoxide", z),
        _ => ("Pawline's own list", own_ranked(own, now)),
    };
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    list.retain(|r| {
        let p = r.path.to_string_lossy().to_lowercase();
        r.path.is_dir() && words.iter().all(|w| p.contains(w))
    });
    (src, list)
}

/// `home` is the area Pawline may act in; folders outside it are listed but marked non-actionable.
pub fn render(src: &str, list: &[Ranked], home: &Path) -> String {
    if list.is_empty() {
        return format!("No frequent folders found (source: {src}).");
    }
    let mut out = format!("Frequent folders (source: {src}, read only):\n");
    for (i, r) in list.iter().take(SHOWN).enumerate() {
        let note = if r.path.starts_with(home) { "" } else { "  [outside your home area: Pawline can't act on it]" };
        out.push_str(&format!("{}. {}  ({:.1}){}\n", i + 1, r.path.display(), r.score, note));
    }
    out.push_str("This only lists them. In the window, pick a folder from the chooser (click, or Up/Down then Tab), then press Enter. In the terminal, type \"list <path>\".");
    out
}

pub fn describe_own(path: &Path) -> String {
    let all = load_own(path);
    if all.is_empty() {
        return "No folders saved in Pawline's own list.".into();
    }
    let mut out = format!("{} folder(s) in Pawline's own list ({}; local, private, max {MAX_OWN}; paths, use counts and last-used time only):\n", all.len(), path.display());
    for (p, u, l) in all.iter().take(SHOWN) {
        out.push_str(&format!("- {} (used {u}x, last {})\n", p.display(), crate::searchmem::date_of(*l)));
    }
    if all.len() > SHOWN {
        out.push_str(&format!("...and {} more.\n", all.len() - SHOWN));
    }
    out.push_str("Say \"forget folders\" to delete it. zoxide's own data is never changed by Pawline.");
    out
}

pub fn forget_own(path: &Path) -> usize {
    let n = load_own(path).len();
    let _ = std::fs::remove_file(path);
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-dirs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_zoxide_score_lines() {
        let r = parse_zoxide("  12.0 /home/u/projects\n3.5 /tmp/a b\nnot a line\n 1.0 relative\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].path, PathBuf::from("/tmp/a b"));
        assert!((r[0].score - 12.0).abs() < 1e-9);
    }

    #[test]
    fn zoxide_preferred_filtered_and_existing_only() {
        let d = tmp("z");
        let a = d.join("alpha");
        std::fs::create_dir_all(&a).unwrap();
        let z = vec![Ranked { score: 9.0, path: a.clone() }, Ranked { score: 8.0, path: d.join("gone") }];
        let (src, l) = ranked(Some(z.clone()), &d.join("own.json"), 1, "");
        assert_eq!((src, l.len()), ("zoxide", 1));
        let (_, l) = ranked(Some(z), &d.join("own.json"), 1, "ALP");
        assert_eq!(l.len(), 1);
        let (_, none) = ranked(Some(vec![Ranked { score: 1.0, path: a }]), &d.join("own.json"), 1, "nope");
        assert!(none.is_empty());
    }

    #[test]
    fn falls_back_to_own_list_when_zoxide_missing_or_empty() {
        let d = tmp("o");
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let own = d.join("ff.json");
        record_own(&own, &[a.clone(), b.clone(), b.clone(), PathBuf::from("rel")], 1000);
        for z in [None, Some(vec![])] {
            let (src, l) = ranked(z, &own, 1000, "");
            assert_eq!(src, "Pawline's own list");
            assert_eq!(l[0].path, b, "more uses ranks first");
            assert_eq!(l.len(), 2, "relative paths are never stored");
        }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(std::fs::metadata(&own).unwrap().permissions().mode() & 0o777, 0o600);
            }
        assert_eq!(forget_own(&own), 2);
        assert!(!own.exists());
    }

    #[test]
    fn own_list_is_bounded_and_render_labels_source() {
        let d = tmp("b");
        let own = d.join("ff.json");
        let many: Vec<PathBuf> = (0..250).map(|i| PathBuf::from(format!("/tmp/pawline-x{i}"))).collect();
        record_own(&own, &many, 5);
        assert_eq!(load_own(&own).len(), MAX_OWN);
        assert!(render("zoxide", &[], Path::new("/home/u")).contains("source: zoxide"));
        let t = render("zoxide", &[Ranked { score: 2.0, path: "/tmp".into() }], Path::new("/home/u"));
        assert!(t.contains("read only") && t.contains("1. /tmp") && t.contains("outside your home area"));
        let h = render("zoxide", &[Ranked { score: 2.0, path: "/home/u/x".into() }], Path::new("/home/u"));
        assert!(!h.contains("outside"));
        assert!(describe_own(&d.join("nofile.json")).contains("No folders"));
        assert!(describe_own(&own).contains("forget folders"));
    }
}
