//! Task-step memory (stage 2 of learning-through-use). Pure core, not yet wired into the UI.
//!
//! Remembers the reviewable text of the steps that worked (and any user correction), found again
//! through a non-reversible lookup key of the request wording. The wording itself is NOT stored.
//! Matching limits: only the same words (case and spacing ignored) find a routine; the key is a
//! 64-bit FNV-1a hash, so it hides the wording from a casual reader but is not encryption and a
//! guessed phrase can be checked against it. It only ever produces a *suggestion string* for the user to read. It never
//! stores or replays executable operations, so a remembered routine cannot run anything:
//! a new request is always re-parsed, re-validated and confirmed as usual.
//! Local 0600 JSON file, bounded, deletable. The secret check is a safeguard, not a guarantee.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 100;
const MAX_STEPS: usize = 10;
const MAX_LEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routine {
    /// Hex FNV-1a key of the normalized request wording. The wording is not kept.
    pub key: String,
    pub steps: Vec<String>,
    pub correction: Option<String>,
    pub saved: u64,
    pub uses: u32,
}

pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().chars().take(MAX_LEN).collect()
}

pub fn lookup_key(request: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in normalize(request).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn default_path() -> Option<PathBuf> {
    crate::searchmem::default_path().map(|p| p.with_file_name("task-memory.json"))
}

pub fn load(path: &Path) -> Vec<Routine> {
    let Some(Value::Array(a)) = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
        return vec![];
    };
    a.iter()
        .filter_map(|v| {
            Some(Routine {
                key: v.get("key")?.as_str()?.to_string(),
                steps: v.get("steps")?.as_array()?.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
                correction: v.get("correction").and_then(|c| c.as_str()).map(String::from),
                saved: v.get("saved")?.as_u64()?,
                uses: v.get("uses")?.as_u64()? as u32,
            })
        })
        .collect()
}

fn save(path: &Path, all: &[Routine]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    let v: Vec<Value> = all
        .iter()
        .map(|r| json!({"key": r.key, "steps": r.steps, "correction": r.correction, "saved": r.saved, "uses": r.uses}))
        .collect();
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path)
}

/// Record steps that ALL completed successfully. Returns false when nothing was saved
/// (no steps, secret-like text anywhere, or an empty request).
pub fn record_success(path: &Path, request: &str, steps: &[String], now: u64) -> bool {
    if normalize(request).is_empty() || steps.is_empty() {
        return false;
    }
    let steps: Vec<String> = steps.iter().take(MAX_STEPS).map(|s| s.chars().take(MAX_LEN).collect()).collect();
    if crate::textfallback::looks_like_secret(request) || steps.iter().any(|s| crate::textfallback::looks_like_secret(&s.replace('/', " "))) {
        return false;
    }
    let key = lookup_key(request);
    let mut all = load(path);
    let (uses, correction) = all.iter().find(|r| r.key == key).map_or((0, None), |r| (r.uses, r.correction.clone()));
    all.retain(|r| r.key != key);
    all.push(Routine { key, steps, correction, saved: now, uses });
    if all.len() > MAX_ENTRIES {
        let drop = all.len() - MAX_ENTRIES;
        all.drain(..drop);
    }
    save(path, &all).is_ok()
}

/// Attach the user's own correction text to an existing routine. Same secret rule.
pub fn record_correction(path: &Path, request: &str, correction: &str) -> bool {
    let key = lookup_key(request);
    let c: String = correction.trim().chars().take(MAX_LEN).collect();
    if c.is_empty() || crate::textfallback::looks_like_secret(&c) {
        return false;
    }
    let mut all = load(path);
    match all.iter_mut().find(|r| r.key == key) {
        Some(r) => r.correction = Some(c),
        None => return false,
    }
    save(path, &all).is_ok()
}

/// A readable suggestion for a request seen before, or None. Display only.
pub fn suggest(path: &Path, request: &str) -> Option<String> {
    let key = lookup_key(request);
    let mut all = load(path);
    let r = all.iter_mut().find(|r| r.key == key)?;
    r.uses += 1;
    let text = format!(
        "Last time this worked with these steps (a suggestion only; I will check and ask again before doing anything):\n{}{}",
        r.steps.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s)).collect::<Vec<_>>().join("\n"),
        r.correction.as_ref().map(|c| format!("\nYou corrected it: {c}")).unwrap_or_default()
    );
    let _ = save(path, &all);
    Some(text)
}

pub fn describe_all(path: &Path) -> String {
    let all = load(path);
    if all.is_empty() {
        return "No task steps saved.".into();
    }
    let mut out = format!("{} saved task routine(s) ({}):\n", all.len(), path.display());
    for r in &all {
        out.push_str(&format!("- {}{} (used {}x, saved {})\n", r.steps.join(" ; "), if r.correction.is_some() { " [corrected]" } else { "" }, r.uses, crate::searchmem::date_of(r.saved)));
    }
    out.push_str("Say \"forget task steps\" to delete them all.");
    out
}

pub fn forget_all(path: &Path) -> usize {
    let n = load(path).len();
    let _ = std::fs::remove_file(path);
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn long_paths_with_digits_are_not_mistaken_for_secrets() {
        let p = std::env::temp_dir().join(format!("pawline-pm-path-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        assert!(record_success(&p, "make a folder called c3 in ho co models", &["Create folder /tmp/xh/container/models/c3".to_string()], 1));
        let _ = std::fs::remove_file(&p);
        // a real key inside a path is still refused
        assert!(!record_success(&p, "make x", &["Create folder /tmp/sk-abcdefghijklmnopqrstuvwxyz0123456789".to_string()], 1));
        let _ = std::fs::remove_file(&p);
    }

    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-planmem-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("task-memory.json")
    }
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn records_suggests_and_counts_uses() {
        let p = tmp("a");
        assert!(record_success(&p, "Make a folder called Kitten  Paws", &s(&["Create folder /home/u/kitten paws"]), 1000));
        let t = suggest(&p, "make a folder called kitten paws").unwrap();
        assert!(t.contains("suggestion only") && t.contains("1. Create folder"));
        assert_eq!(load(&p)[0].uses, 1);
        assert!(suggest(&p, "something else").is_none());
    }

    #[test]
    fn corrections_attach_and_show() {
        let p = tmp("b");
        assert!(!record_correction(&p, "x", "y"));
        record_success(&p, "tidy downloads", &s(&["Move a to b"]), 1);
        assert!(record_correction(&p, "tidy downloads", "put pdfs in Documents"));
        assert!(suggest(&p, "tidy downloads").unwrap().contains("put pdfs in Documents"));
    }

    #[test]
    fn secrets_empty_and_caps_are_refused_or_bounded() {
        let p = tmp("c");
        assert!(!record_success(&p, "my password is hunter2 make a file", &s(&["Create file a"]), 1));
        assert!(!record_success(&p, "make file", &s(&["Create file sk-abcdef123456"]), 1));
        assert!(!record_success(&p, "make file", &[], 1));
        assert!(!record_success(&p, "  ", &s(&["x"]), 1));
        assert!(!record_correction(&p, "make file", "token: abc"));
        let many: Vec<String> = (0..30).map(|i| format!("step {i}")).collect();
        assert!(record_success(&p, "big", &many, 1));
        assert_eq!(load(&p)[0].steps.len(), MAX_STEPS);
        for i in 0..150 {
            record_success(&p, &format!("req {i}"), &s(&["a"]), i);
        }
        assert_eq!(load(&p).len(), MAX_ENTRIES);
    }

    #[test]
    fn file_is_private_and_forgettable() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("d");
        record_success(&p, "a b", &s(&["x"]), 1);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(describe_all(&p).contains("- x ("));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("a b"), "request wording must not be stored");
        assert_eq!(forget_all(&p), 1);
        assert!(!p.exists() && load(&p).is_empty());
        assert_eq!(describe_all(&p), "No task steps saved.");
    }
}
