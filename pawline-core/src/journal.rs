//! Change journal: every successful change is recorded under
//! `<root>/.local/share/pawline/` so the last one can be undone.
//!
//! Deletes never remove data: the item moves into `trash/` and the journal
//! remembers where it came from. Entries use paths relative to the root.
//! The journal is a UX aid, not a security boundary: undo re-validates
//! everything at the time it runs, beneath the root handle.

use serde_json::{json, Value};

pub const STORE: &str = ".local/share/pawline";
pub const JOURNAL: &str = ".local/share/pawline/journal.jsonl";
pub const WALLPAPERS: &str = ".local/share/pawline/wallpapers";
pub const TRASH: &str = ".local/share/pawline/trash";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    /// create_dir, create_file, rename, move, delete
    pub kind: String,
    /// Where the item is/was (relative to the root).
    pub a: String,
    /// Rename/move: the new location. Delete: where it sits in the trash.
    pub b: Option<String>,
}

impl Entry {
    pub fn to_line(&self) -> String {
        json!({"id": self.id, "kind": self.kind, "a": self.a, "b": self.b}).to_string()
    }

    pub fn describe_undo(&self) -> String {
        match self.kind.as_str() {
            "create_dir" => format!("Undo: remove the empty folder ~/{}", self.a),
            "create_file" => format!("Undo: remove the empty file ~/{}", self.a),
            "rename" | "move" => format!(
                "Undo: put ~/{} back at ~/{}",
                self.b.clone().unwrap_or_default(),
                self.a
            ),
            "wallpaper" => match &self.b {
                Some(_) => "Undo: put the previous Pawline-set wallpaper back".to_string(),
                None => "Undo: no earlier Pawline-set wallpaper is known, so this cannot be reverted".to_string(),
            },
            "delete" => format!("Undo: restore ~/{} from the Pawline trash", self.a),
            k => format!("Undo: {k}"),
        }
    }
}

pub fn undone_line(id: u64) -> String {
    json!({"undo_of": id}).to_string()
}

/// The most recent entry that has not been undone.
pub fn last_open(text: &str) -> Option<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(u) = v.get("undo_of").and_then(|x| x.as_u64()) {
            entries.retain(|e| e.id != u);
        } else if let (Some(id), Some(kind), Some(a)) = (
            v.get("id").and_then(|x| x.as_u64()),
            v.get("kind").and_then(|x| x.as_str()),
            v.get("a").and_then(|x| x.as_str()),
        ) {
            entries.push(Entry {
                id,
                kind: kind.into(),
                a: a.into(),
                b: v.get("b").and_then(|x| x.as_str()).map(String::from),
            });
        }
    }
    entries.pop()
}

/// The most recent entry of one kind that has not been undone.
pub fn last_open_of(text: &str, kind: &str) -> Option<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if let Some(u) = v.get("undo_of").and_then(|x| x.as_u64()) {
            entries.retain(|e| e.id != u);
        } else if let (Some(id), Some(k), Some(a)) = (
            v.get("id").and_then(|x| x.as_u64()),
            v.get("kind").and_then(|x| x.as_str()),
            v.get("a").and_then(|x| x.as_str()),
        ) {
            entries.push(Entry {
                id,
                kind: k.into(),
                a: a.into(),
                b: v.get("b").and_then(|x| x.as_str()).map(String::from),
            });
        }
    }
    entries.into_iter().rev().find(|e| e.kind == kind)
}

pub fn next_id(text: &str) -> u64 {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|x| x.as_u64()))
        .max()
        .map_or(1, |m| m + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_open_skips_undone() {
        let e1 = Entry { id: 1, kind: "create_dir".into(), a: "x".into(), b: None };
        let e2 = Entry { id: 2, kind: "delete".into(), a: "y".into(), b: Some("t/1-y".into()) };
        let text = format!("{}\n{}\n{}\n", e1.to_line(), e2.to_line(), undone_line(2));
        assert_eq!(last_open(&text), Some(e1));
        assert_eq!(next_id(&text), 3);
        assert_eq!(last_open("garbage\n"), None);
    }
}
