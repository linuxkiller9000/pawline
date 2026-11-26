//! Opt-in local conversation history. Off by default; nothing here runs unless the user switched it on.
//! Stores only chat lines (what the user typed and Pawline's plain replies). Never stores the unsent draft,
//! confirm cards, pending tasks, errors, thumbnails or voice audio. Private file, atomic replace, deletable.
use std::path::Path;

/// Shown in Settings next to the switch.
pub const NOTE: &str = "Conversation file (off by default, separate from settings): when ON it keeps what you submit from then on (typed or from voice) and Pawline's plain replies. NOT encrypted: anyone who can read your account files can read it. Not kept: draft, cards, tasks, audio. Permissions are restricted on Linux only, unverified on Windows. OFF deletes it. Never added to online chat context.";

pub const MAX_MSGS: usize = 100;
pub const MAX_CHARS: usize = 4000;
const MAX_FILE: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub user: bool,
    pub text: String,
}

/// Only these reply kinds are plain conversation. Cards, errors and anything else are not kept.
pub fn keep_kind(user: bool, kind: &str) -> bool {
    if user {
        kind == "user" || kind == "message"
    } else {
        kind == "message" || kind == "md"
    }
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars().take(MAX_CHARS) {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('t') => o.push('\t'),
                Some('\\') => o.push('\\'),
                _ => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

pub fn encode(lines: &[Line]) -> String {
    let start = lines.len().saturating_sub(MAX_MSGS);
    lines[start..].iter().map(|l| format!("{}\t{}\n", if l.user { 'U' } else { 'A' }, esc(&l.text))).collect()
}

pub fn decode(text: &str) -> Vec<Line> {
    let mut v: Vec<Line> = text
        .lines()
        .filter_map(|l| {
            let (k, t) = l.split_once('\t')?;
            let user = match k {
                "U" => true,
                "A" => false,
                _ => return None,
            };
            let t = unesc(t);
            (!t.is_empty()).then_some(Line { user, text: t })
        })
        .collect();
    let start = v.len().saturating_sub(MAX_MSGS);
    v.drain(..start);
    v
}

/// Write to a private temp file in the same folder, then rename over the old file: a crash leaves the old or the new, never half.
pub fn save(path: &Path, lines: &[Line]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let r = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(encode(lines).as_bytes())?;
        f.sync_all()?;
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// A missing, oversized, unreadable or non-regular file is an empty history, never a crash.
pub fn load(path: &Path) -> Vec<Line> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX_FILE => std::fs::read_to_string(path).map(|t| decode(&t)).unwrap_or_default(),
        _ => vec![],
    }
}

/// Delete the saved conversation. Ok when there was nothing to delete.
pub fn delete(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("history.txt")
    }
    #[test]
    fn round_trip_keeps_newlines_tabs_backslashes_and_arabic() {
        let f = tmp("rt");
        let v = vec![
            Line { user: true, text: "make a folder called a\\b".into() },
            Line { user: false, text: "line1\nline2\tx \u{645}\u{631}\u{62d}\u{628}\u{627}".into() },
        ];
        save(&f, &v).unwrap();
        assert_eq!(load(&f), v);
    }
    #[test]
    #[cfg(unix)]
    fn file_is_private_atomic_and_leaves_no_temp() {
        use std::os::unix::fs::PermissionsExt;
        let f = tmp("priv");
        save(&f, &[Line { user: true, text: "x".into() }]).unwrap();
        save(&f, &[Line { user: true, text: "y".into() }]).unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1, "only history.txt remains");
        assert_eq!(load(&f)[0].text, "y");
    }
    #[test]
    fn capped_and_corrupt_files_are_safe() {
        let f = tmp("cap");
        let many: Vec<Line> = (0..250).map(|i| Line { user: i % 2 == 0, text: format!("m{i}") }).collect();
        save(&f, &many).unwrap();
        let l = load(&f);
        assert_eq!(l.len(), MAX_MSGS);
        assert_eq!(l.last().unwrap().text, "m249");
        std::fs::write(&f, "garbage\nX\tno\nU\t\nA\tok\n").unwrap();
        assert_eq!(load(&f), vec![Line { user: false, text: "ok".into() }]);
        std::fs::write(&f, vec![b'U'; 1_200_000]).unwrap();
        assert!(load(&f).is_empty(), "oversized file is ignored");
        assert!(load(&f.with_file_name("none.txt")).is_empty());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_is_not_followed_and_delete_is_idempotent() {
        let f = tmp("sym");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let target = f.with_file_name("secret.txt");
        std::fs::write(&target, "U\tleak\n").unwrap();
        std::os::unix::fs::symlink(&target, &f).unwrap();
        assert!(load(&f).is_empty(), "a symlink in place of the file is not read");
        delete(&f).unwrap();
        assert!(target.exists(), "delete removed the link only");
        delete(&f).unwrap();
    }
    /// Structural guard (not a behavioural proof): the online chat context is built in bridge.rs from its own per-session
    /// state; this module must stay out of it so restored text can never reach an online request.
    #[test]
    fn history_is_not_wired_into_the_online_chat_bridge() {
        let src = include_str!("bridge.rs");
        const MARK: &str = "==== END OF PRODUCTION CODE";
        assert!(src.matches(MARK).count() == 1, "the marker comment must stay in bridge.rs");
        let b = src.split(MARK).next().unwrap(); // production code only; moves with edits, no line numbers
        assert!(!b.contains("history::") && !b.contains("history.txt") && !b.contains("keep_history"));
    }

    #[test]
    fn only_plain_conversation_is_kept() {
        assert!(keep_kind(true, "user") && keep_kind(false, "message") && keep_kind(false, "md"));
        assert!(!keep_kind(false, "confirm") && !keep_kind(false, "error") && !keep_kind(true, "confirm"));
    }
}
