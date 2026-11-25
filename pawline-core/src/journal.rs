//! Change journal with write-ahead intent records, so a crash or a failed
//! write cannot leave an untracked change.
//!
//! Lines, in `<root>/.local/share/pawline/journal.jsonl`:
//!   {"intent":{id,kind,a,b}}   written BEFORE the change (fails => no change)
//!   {"commit":id,"meta":..}    written after the change succeeded
//!   {"abort":id}               written when the change failed or recovery
//!                              found it never happened
//!   {"undo_of":id}             written after an undo succeeded
//! An intent with no commit/abort is "pending": startup recovery inspects the
//! disk and settles it. Pending entries are never offered for undo.
//! Paths are relative to the root. The journal is a UX aid; undo re-validates
//! everything when it runs, beneath the root handle.

use serde_json::{json, Value};

pub const STORE: &str = ".local/share/pawline";
pub const JOURNAL: &str = ".local/share/pawline/journal.jsonl";
pub const TRASH: &str = ".local/share/pawline/trash";
pub const WALLPAPERS: &str = ".local/share/pawline/wallpapers";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    /// create_dir, create_file, rename, move, copy, delete, wallpaper, wallpaper_base
    pub kind: String,
    /// Where the item is/was (relative to the root).
    pub a: String,
    /// Rename/move: the new location. Delete: its place in the trash.
    /// Wallpaper: the previous wallpaper copy.
    pub b: Option<String>,
    /// Fingerprint taken at commit time (create_file: "mtime_ns:ino").
    pub meta: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Committed,
    Aborted,
    Undone,
}

impl Entry {
    pub fn intent_line(&self) -> String {
        json!({"intent": {"id": self.id, "kind": self.kind, "a": self.a, "b": self.b}}).to_string()
    }

    pub fn describe_undo(&self) -> String {
        match self.kind.as_str() {
            "create_dir" => format!("Undo: remove the empty folder ~/{}", self.a),
            "create_file" => format!("Undo: remove the file ~/{} (only if still untouched and empty)", self.a),
            "rename" | "move" => format!(
                "Undo: put ~/{} back at ~/{}",
                self.b.clone().unwrap_or_default(),
                self.a
            ),
            "copy" => format!("Undo: move the copy ~/{} into the Pawline trash", self.a),
            "wallpaper" => match &self.b {
                Some(_) => "Undo: put the previous wallpaper back".to_string(),
                None => "Undo: the previous wallpaper is unknown, so this cannot be reverted".to_string(),
            },
            "delete" => format!("Undo: restore ~/{} from the Pawline trash", self.a),
            k => format!("Undo: {k}"),
        }
    }
}

pub fn commit_line(id: u64, meta: Option<&str>) -> String {
    json!({"commit": id, "meta": meta}).to_string()
}
pub fn abort_line(id: u64) -> String {
    json!({"abort": id}).to_string()
}
pub fn undone_line(id: u64) -> String {
    json!({"undo_of": id}).to_string()
}

/// Fold the log into entries with their final status, in intent order.
pub fn fold(text: &str) -> Vec<(Entry, Status)> {
    let mut out: Vec<(Entry, Status)> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(i) = v.get("intent") {
            if let (Some(id), Some(kind), Some(a)) = (
                i.get("id").and_then(|x| x.as_u64()),
                i.get("kind").and_then(|x| x.as_str()),
                i.get("a").and_then(|x| x.as_str()),
            ) {
                out.push((
                    Entry {
                        id,
                        kind: kind.into(),
                        a: a.into(),
                        b: i.get("b").and_then(|x| x.as_str()).map(String::from),
                        meta: None,
                    },
                    Status::Pending,
                ));
            }
        } else if let Some(id) = v.get("commit").and_then(|x| x.as_u64()) {
            if let Some(e) = out.iter_mut().find(|(e, _)| e.id == id) {
                e.1 = Status::Committed;
                e.0.meta = v.get("meta").and_then(|x| x.as_str()).map(String::from);
            }
        } else if let Some(id) = v.get("abort").and_then(|x| x.as_u64()) {
            if let Some(e) = out.iter_mut().find(|(e, s)| e.id == id && *s == Status::Pending) {
                e.1 = Status::Aborted;
            }
        } else if let Some(id) = v.get("undo_of").and_then(|x| x.as_u64()) {
            if let Some(e) = out.iter_mut().find(|(e, _)| e.id == id) {
                e.1 = Status::Undone;
            }
        }
    }
    out
}

fn undoable(kind: &str) -> bool {
    !kind.ends_with("_base")
}

/// The most recent committed, not-undone, user-visible change.
pub fn last_open(text: &str) -> Option<Entry> {
    fold(text)
        .into_iter()
        .rev()
        .find(|(e, s)| *s == Status::Committed && undoable(&e.kind))
        .map(|(e, _)| e)
}

/// The most recent committed, not-undone entry of one kind.
pub fn last_open_of(text: &str, kind: &str) -> Option<Entry> {
    fold(text)
        .into_iter()
        .rev()
        .find(|(e, s)| *s == Status::Committed && e.kind == kind)
        .map(|(e, _)| e)
}

pub fn pending(text: &str) -> Vec<Entry> {
    fold(text)
        .into_iter()
        .filter(|(_, s)| *s == Status::Pending)
        .map(|(e, _)| e)
        .collect()
}

pub fn next_id(text: &str) -> u64 {
    fold(text).iter().map(|(e, _)| e.id).max().map_or(1, |m| m + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn e(id: u64, kind: &str, a: &str) -> Entry {
        Entry { id, kind: kind.into(), a: a.into(), b: None, meta: None }
    }
    #[test]
    fn pending_is_not_undoable_and_commit_makes_it_so() {
        let mut t = format!("{}\n", e(1, "create_dir", "x").intent_line());
        assert_eq!(last_open(&t), None);
        assert_eq!(pending(&t).len(), 1);
        t += &format!("{}\n", commit_line(1, Some("m")));
        assert_eq!(last_open(&t).unwrap().meta.as_deref(), Some("m"));
        t += &format!("{}\n{}\n", e(2, "delete", "y").intent_line(), abort_line(2));
        assert_eq!(last_open(&t).unwrap().id, 1);
        t += &format!("{}\n", undone_line(1));
        assert_eq!(last_open(&t), None);
        assert_eq!(next_id(&t), 3);
        assert!(pending(&t).is_empty());
    }
    #[test]
    fn baseline_entries_are_not_offered_for_undo_but_are_found_by_kind() {
        let t = format!("{}\n{}\n", e(1, "wallpaper_base", "w").intent_line(), commit_line(1, None));
        assert_eq!(last_open(&t), None);
        assert!(last_open_of(&t, "wallpaper_base").is_some());
    }
    #[test]
    fn garbage_lines_are_skipped() {
        assert_eq!(last_open("not json\n{\"x\":1}\n"), None);
    }
}
