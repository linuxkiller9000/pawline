//! Pending confirm cards that survive a crash or closing the app (item 15b).
//!
//! When a card for a LOCAL file plan (create/delete/rename/move/copy) is shown,
//! a small record is written next to the undo journal:
//! `<root>/.local/share/pawline/pending-confirm.json`:
//!   {"request": "...", "fingerprints": [{"path", "exists", "mtime_s", "mtime_ns", "size", "ino"}]}
//! At startup the record is re-validated: every fingerprint must match the disk
//! exactly, otherwise the card is dropped with a note instead of offering a
//! stale plan. The record is deleted the moment the card is approved, cancelled,
//! replaced by another card, or dropped by a new message.
//!
//! Only local file plans are persisted: no online text, no chat, no secrets
//! (secret-looking text never reaches a card at all). The record holds the same
//! class of data as the undo journal (the request and the paths it touched), in
//! the same private store, and it is deleted on resolution.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::plan::{Operation, Plan};

pub const PENDING: &str = ".local/share/pawline/pending-confirm.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub path: PathBuf,
    pub exists: bool,
    pub mtime_s: i64,
    pub mtime_ns: i64,
    pub size: u64,
    pub ino: u64,
}

#[derive(Debug, Clone)]
pub struct Record {
    pub request: String,
    pub fingerprints: Vec<Fingerprint>,
}

pub fn take(p: &Path) -> Fingerprint {
    match std::fs::symlink_metadata(p) {
        Ok(m) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                Fingerprint { path: p.to_path_buf(), exists: true, mtime_s: m.mtime(), mtime_ns: m.mtime_nsec(), size: m.size(), ino: m.ino() }
            }
            #[cfg(not(unix))]
            {
                let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                Fingerprint { path: p.to_path_buf(), exists: true, mtime_s: t.map(|d| d.as_secs() as i64).unwrap_or(0), mtime_ns: t.map(|d| d.subsec_nanos() as i64).unwrap_or(0), size: m.len(), ino: 0 }
            }
        }
        Err(_) => Fingerprint { path: p.to_path_buf(), exists: false, mtime_s: 0, mtime_ns: 0, size: 0, ino: 0 },
    }
}

/// True when every path the plan touches is exactly as it was when the card was shown.
pub fn matches_disk(fps: &[Fingerprint]) -> bool {
    fps.iter().all(|f| take(&f.path) == *f)
}

fn plan_paths(plan: &Plan) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for op in &plan.ops {
        match op {
            Operation::CreateDir { path } | Operation::CreateFile { path } | Operation::Delete { path } => out.push(path.clone()),
            Operation::Rename { from, to } | Operation::Move { from, to } | Operation::Copy { from, to } => {
                out.push(from.clone());
                out.push(to.clone());
            }
            // Anything else (online, decks, shrink, wallpaper, undo, ...) is not persisted.
            _ => return None,
        }
    }
    Some(out)
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path)
}

/// Persist a showing card for crash recovery. Plans that are not purely local
/// file operations (or an unwritable store) simply are not remembered; the
/// in-session card is unaffected either way.
pub fn save(root: &Path, plan: &Plan, request: &str) {
    let Some(paths) = plan_paths(plan) else { return };
    let fps: Vec<Value> = paths
        .iter()
        .map(|p| {
            let f = take(p);
            json!({"path": f.path.to_string_lossy(), "exists": f.exists, "mtime_s": f.mtime_s, "mtime_ns": f.mtime_ns, "size": f.size, "ino": f.ino})
        })
        .collect();
    let rec = json!({"request": request, "fingerprints": fps}).to_string();
    let _ = write_private(&root.join(PENDING), &rec);
}

/// The card was approved, cancelled, replaced or dropped: nothing waits any more.
pub fn clear(root: &Path) {
    let _ = std::fs::remove_file(root.join(PENDING));
}

pub fn load(root: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(root.join(PENDING)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let request = v.get("request")?.as_str()?.to_string();
    let mut fps = Vec::new();
    for f in v.get("fingerprints")?.as_array()? {
        fps.push(Fingerprint {
            path: PathBuf::from(f.get("path")?.as_str()?),
            exists: f.get("exists")?.as_bool()?,
            mtime_s: f.get("mtime_s")?.as_i64()?,
            mtime_ns: f.get("mtime_ns")?.as_i64()?,
            size: f.get("size")?.as_u64()?,
            ino: f.get("ino")?.as_u64()?,
        });
    }
    Some(Record { request, fingerprints: fps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_notices_content_and_name_changes() {
        let d = std::env::temp_dir().join(format!("pawline-pending-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.txt");
        std::fs::write(&f, b"one").unwrap();
        let f1 = take(&f);
        assert!(matches_disk(&[f1.clone()]), "untouched file matches");
        std::fs::write(&f, b"two").unwrap();
        assert!(!matches_disk(&[f1.clone()]), "rewritten file does not match");
        std::fs::remove_file(&f).unwrap();
        let f2 = take(&f);
        assert!(!f2.exists, "missing file fingerprints as absent");
        assert!(!matches_disk(&[f1]), "now-missing file does not match");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_load_clear_roundtrip_only_for_local_plans() {
        let d = std::env::temp_dir().join(format!("pawline-pending-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let victim = d.join("kp");
        std::fs::create_dir(&victim).unwrap();
        let local = Plan {
            ops: vec![Operation::Delete { path: victim.clone() }],
            needs_confirmation: true,
            confirmation_reasons: vec![],
            assist_derived: false,
            notes: vec![],
        };
        save(&d, &local, "delete the folder kp");
        let rec = load(&d).expect("record written for a local plan");
        assert_eq!(rec.request, "delete the folder kp");
        assert!(matches_disk(&rec.fingerprints), "fresh record matches the disk");
        clear(&d);
        assert!(load(&d).is_none(), "cleared record is gone");

        let online = Plan {
            ops: vec![Operation::Weather { place: "cairo".into() }],
            needs_confirmation: true,
            confirmation_reasons: vec![],
            assist_derived: false,
            notes: vec![],
        };
        save(&d, &online, "weather in cairo");
        assert!(load(&d).is_none(), "online plans are never persisted");
        let _ = std::fs::remove_dir_all(&d);
    }
}
