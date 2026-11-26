//! Opt-in local conversation history. Off by default; nothing here runs unless the user switched it on.
//! Stores only chat lines (what the user typed and Pawline's plain replies). Never stores the unsent draft,
//! confirm cards, pending tasks, errors, thumbnails or voice audio. Private file, atomic replace, deletable.
use std::path::Path;

/// Shown in Settings next to the switch.
pub const NOTE: &str = "Conversation file (off by default, separate from settings): when ON it keeps what you submit from then on (typed or from voice) and Pawline's plain replies. NOT encrypted: anyone who can read your account files can read it. Not kept: draft, cards, tasks, audio. Permissions are restricted on Linux only, unverified on Windows. OFF deletes it and every archived copy (archives are made when you say 'new conversation'). Never added to online chat context.";

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

/// Render the saved conversation and every archived copy in `dir` as plain readable text
/// (item 19: export). Returns the text and the number of saved lines; empty text when nothing
/// is saved. Read-only: files are read through `load`, which skips missing/oversized/corrupt ones.
pub fn render_export(dir: &Path) -> (String, usize) {
    let mut files: Vec<(bool, std::path::PathBuf)> = Vec::new();
    let cur = dir.join("history.txt");
    if cur.symlink_metadata().map(|m| m.is_file()).unwrap_or(false) {
        files.push((false, cur));
    }
    let mut arch: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("history-archived-") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();
    arch.sort();
    files.extend(arch.into_iter().map(|p| (true, p)));
    let mut out = String::new();
    let mut total = 0usize;
    for (archived, f) in files {
        if archived {
            out.push_str(&format!("--- archived copy: {} ---\n", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        }
        for l in load(&f) {
            total += 1;
            out.push_str(if l.user { "You: " } else { "Pawline: " });
            out.push_str(&l.text);
            out.push('\n');
        }
    }
    (out, total)
}

/// One entry of the saved-conversation listing (item 19): the current saved conversation first,
/// then every archived copy in name order. `lines` counts decodable lines; a file that exists but
/// is empty or unreadable through `load` is still listed, with 0 lines.
pub struct ConvEntry {
    pub path: std::path::PathBuf,
    pub label: String,
    pub current: bool,
    pub lines: usize,
}

/// List the current saved conversation and every archived copy in `dir`. Read-only.
pub fn list_dir(dir: &Path) -> Vec<ConvEntry> {
    let mut out: Vec<ConvEntry> = Vec::new();
    let cur = dir.join("history.txt");
    if cur.symlink_metadata().map(|m| m.is_file()).unwrap_or(false) {
        out.push(ConvEntry { lines: load(&cur).len(), path: cur, label: "current saved conversation".into(), current: true });
    }
    let mut arch: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("history-archived-") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();
    arch.sort();
    for p in arch {
        let label = format!("archived copy {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        out.push(ConvEntry { lines: load(&p).len(), path: p, label, current: false });
    }
    out
}

/// Write `text` to `target` without ever replacing an existing file: a temporary sibling is written
/// and flushed, hard-linked into place (fails cleanly when the name is taken), then removed.
/// Ok(true) = written, Ok(false) = the name was taken and nothing was written.
pub fn write_no_replace(target: &Path, text: &str) -> std::io::Result<bool> {
    let tmp = target.with_file_name(format!(".{}.tmp-{}", target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        use std::io::Write;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    match std::fs::hard_link(&tmp, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(&tmp);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&tmp);
            Ok(false)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub const SEARCH_MAX_HITS: usize = 10;
pub const SEARCH_HIT_MAX: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// True when the line comes from an archived copy (made by "new conversation"), not the current file.
    pub archived: bool,
    /// True when the user said the line, false when Pawline did.
    pub user: bool,
    pub text: String,
}

/// Search the saved conversation and every archived copy in `dir` for a case-insensitive substring
/// (item 19: history search). Read-only. Files are read through `load`, so missing, oversized,
/// symlinked-away or corrupt files are simply skipped. Returns up to SEARCH_MAX_HITS hits, each
/// clipped to SEARCH_HIT_MAX chars, plus the total number of matching lines.
pub fn search_dir(dir: &Path, query: &str) -> (Vec<Hit>, usize) {
    let q = query.to_lowercase();
    let mut files: Vec<(bool, std::path::PathBuf)> = Vec::new();
    let cur = dir.join("history.txt");
    if cur.symlink_metadata().map(|m| m.is_file()).unwrap_or(false) {
        files.push((false, cur));
    }
    let mut arch: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("history-archived-") && n.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();
    arch.sort();
    files.extend(arch.into_iter().map(|p| (true, p)));
    let mut hits = Vec::new();
    let mut total = 0usize;
    for (archived, f) in files {
        for l in load(&f) {
            if l.text.to_lowercase().contains(&q) {
                total += 1;
                if hits.len() < SEARCH_MAX_HITS {
                    hits.push(Hit { archived, user: l.user, text: l.text.chars().take(SEARCH_HIT_MAX).collect() });
                }
            }
        }
    }
    (hits, total)
}

#[cfg(test)]
mod list_tests {
    use super::*;

    #[test]
    fn lists_current_first_then_archives_with_counts() {
        let d = std::env::temp_dir().join(format!("pawline-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        save(&d.join("history.txt"), &[Line { user: true, text: "hi".into() }, Line { user: false, text: "hello".into() }]).unwrap();
        save(&d.join("history-archived-2000.txt"), &[Line { user: true, text: "b".into() }]).unwrap();
        save(&d.join("history-archived-1000.txt"), &[Line { user: true, text: "a1".into() }, Line { user: false, text: "a2".into() }, Line { user: true, text: "a3".into() }]).unwrap();
        std::fs::write(d.join("unrelated.txt"), "not a conversation").unwrap();
        let v = list_dir(&d);
        assert_eq!(v.len(), 3, "unrelated files are not listed");
        assert!(v[0].current && v[0].label == "current saved conversation" && v[0].lines == 2, "{:?}", v[0].label);
        assert!(!v[1].current && v[1].label == "archived copy history-archived-1000.txt" && v[1].lines == 3, "{:?}", v[1].label);
        assert!(!v[2].current && v[2].lines == 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_lists_nothing_and_empty_file_still_lists() {
        let d = std::env::temp_dir().join(format!("pawline-list-none-{}", std::process::id()));
        assert!(list_dir(&d).is_empty());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("history-archived-3000.txt"), "").unwrap();
        let v = list_dir(&d);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].lines, 0, "an existing but empty file is listed with 0 lines");
        let _ = std::fs::remove_dir_all(&d);
    }
}

mod export_tests {
    use super::*;

    #[test]
    fn render_export_labels_speakers_and_archives_and_counts_lines() {
        let d = std::env::temp_dir().join(format!("pawline-exp-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        save(&d.join("history.txt"), &[Line { user: true, text: "hi".into() }, Line { user: false, text: "hello".into() }]).unwrap();
        save(&d.join("history-archived-1000.txt"), &[Line { user: true, text: "old question".into() }]).unwrap();
        let (text, total) = render_export(&d);
        assert_eq!(total, 3);
        assert!(text.starts_with("You: hi\nPawline: hello\n"), "{text}");
        assert!(text.contains("--- archived copy: history-archived-1000.txt ---\nYou: old question\n"), "{text}");
        let missing = d.join("nope");
        assert_eq!(render_export(&missing), (String::new(), 0), "missing dir renders nothing");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_no_replace_never_overwrites_and_cleans_up() {
        let d = std::env::temp_dir().join(format!("pawline-wr-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let t = d.join("out.txt");
        assert_eq!(write_no_replace(&t, "first").unwrap(), true);
        assert_eq!(std::fs::read_to_string(&t).unwrap(), "first");
        assert_eq!(write_no_replace(&t, "second").unwrap(), false, "taken name: nothing written");
        assert_eq!(std::fs::read_to_string(&t).unwrap(), "first", "the existing file is untouched");
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.')).collect();
        assert!(leftovers.is_empty(), "no temp files left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn dir_with_files(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-hs-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        save(&d.join("history.txt"), &[Line { user: true, text: "how do I build with CMake".into() }, Line { user: false, text: "Run cmake -B build first".into() }, Line { user: true, text: "unrelated question".into() }]).unwrap();
        save(&d.join("history-archived-1000.txt"), &[Line { user: true, text: "older cmake error".into() }]).unwrap();
        save(&d.join("history-archived-2000.txt"), &[Line { user: false, text: "nothing relevant".into() }]).unwrap();
        d
    }

    #[test]
    fn search_finds_across_current_and_archives_case_insensitively() {
        let d = dir_with_files("basic");
        let (hits, total) = search_dir(&d, "CMAKE");
        assert_eq!(total, 3);
        assert_eq!(hits.len(), 3);
        assert!(!hits[0].archived && hits[0].user);
        assert!(!hits[1].archived && !hits[1].user);
        assert!(hits[2].archived, "the archived copy is searched and labelled");
        let (hits, total) = search_dir(&d, "nothing here matches");
        assert_eq!((hits.len(), total), (0, 0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn search_caps_hits_but_counts_everything_and_clips_long_lines() {
        let d = std::env::temp_dir().join(format!("pawline-hs-cap-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let lines: Vec<Line> = (0..25).map(|i| Line { user: true, text: format!("match line {i}") }).collect();
        save(&d.join("history.txt"), &lines).unwrap();
        let long = "x".repeat(500) + "needle";
        save(&d.join("history-archived-1000.txt"), &[Line { user: false, text: long.clone() }]).unwrap();
        let (hits, total) = search_dir(&d, "match line");
        assert_eq!(total, 25);
        assert_eq!(hits.len(), SEARCH_MAX_HITS, "only the first hits are carried");
        let (hits, total) = search_dir(&d, "needle");
        assert_eq!(total, 1);
        assert_eq!(hits[0].text.chars().count(), SEARCH_HIT_MAX, "long lines are clipped");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn search_an_empty_or_missing_dir_finds_nothing() {
        let d = std::env::temp_dir().join(format!("pawline-hs-empty-{}", std::process::id()));
        assert_eq!(search_dir(&d, "x"), (vec![], 0), "missing dir");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(search_dir(&d, "x"), (vec![], 0), "empty dir");
        std::fs::write(d.join("history-archived-5.txt"), "garbage without header").unwrap();
        assert_eq!(search_dir(&d, "x"), (vec![], 0), "corrupt file is skipped, no panic");
        let _ = std::fs::remove_dir_all(&d);
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
