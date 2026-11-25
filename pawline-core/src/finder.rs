//! "fd <words>" (find names) and "rg <text>" (find text inside files), read only.
//! Built on the same libraries the fd and ripgrep tools use (the `ignore` and `grep-*` crates, MIT or
//! Unlicense), so no other program has to be installed. Like those tools they skip hidden files and
//! anything .gitignore'd, never follow symlinks, and here also stay inside the home area, skip Pawline's
//! own journal, stop after a time limit and a result limit, and never change anything.
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{sinks::UTF8, BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_RESULTS: usize = 30;
pub const TIME_LIMIT: Duration = Duration::from_secs(2);
const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_LINE: usize = 160;

pub struct Found {
    pub hits: Vec<(PathBuf, bool, Option<String>)>, // path, is_dir, matching line (rg)
    pub truncated: bool,
    pub timed_out: bool,
}

fn walker(root: &Path) -> ignore::Walk {
    let store = root.join(crate::journal::STORE);
    WalkBuilder::new(root).follow_links(false).filter_entry(move |e| !e.path().starts_with(&store)).build()
}

/// Names containing every word (smart case: lower-case words ignore case). Folders and files.
pub fn find_names(root: &Path, words: &[String]) -> Found {
    let start = Instant::now();
    let mut f = Found { hits: vec![], truncated: false, timed_out: false };
    if words.is_empty() {
        return f;
    }
    let sens: Vec<bool> = words.iter().map(|w| w.chars().any(|c| c.is_uppercase())).collect();
    for ent in walker(root).flatten() {
        if start.elapsed() > TIME_LIMIT {
            f.timed_out = true;
            break;
        }
        if ent.path() == root {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        let low = name.to_lowercase();
        if words.iter().zip(&sens).all(|(w, s)| if *s { name.contains(w.as_str()) } else { low.contains(&w.to_lowercase()) }) {
            if f.hits.len() == MAX_RESULTS {
                f.truncated = true;
                break;
            }
            f.hits.push((ent.path().to_path_buf(), ent.file_type().is_some_and(|t| t.is_dir()), None));
        }
    }
    f
}

/// Lines containing the text (literal, smart case) in text files up to 2 MB. Binary files are skipped.
pub fn find_text(root: &Path, text: &str) -> Found {
    let start = Instant::now();
    let mut f = Found { hits: vec![], truncated: false, timed_out: false };
    if text.trim().is_empty() {
        return f;
    }
    let smart = !text.chars().any(|c| c.is_uppercase());
    let Ok(m) = RegexMatcherBuilder::new().case_insensitive(smart).fixed_strings(true).build(text) else { return f };
    let mut s = SearcherBuilder::new().binary_detection(BinaryDetection::quit(0)).line_number(true).build();
    'outer: for ent in walker(root).flatten() {
        if start.elapsed() > TIME_LIMIT {
            f.timed_out = true;
            break;
        }
        if !ent.file_type().is_some_and(|t| t.is_file()) || ent.metadata().map(|m| m.len() > MAX_FILE).unwrap_or(true) {
            continue;
        }
        let mut first: Option<String> = None;
        let _ = s.search_path(&m, ent.path(), UTF8(|n, line| {
            first = Some(format!("{n}: {}", line.trim().chars().take(MAX_LINE).collect::<String>()));
            Ok(false)
        }));
        if let Some(l) = first {
            if f.hits.len() == MAX_RESULTS {
                f.truncated = true;
                break 'outer;
            }
            f.hits.push((ent.path().to_path_buf(), false, Some(l)));
        }
    }
    f
}

pub fn render(kind: &str, query: &str, root: &Path, f: &Found) -> String {
    if f.hits.is_empty() {
        return format!("{kind} \"{query}\": nothing found in your home area{}. Hidden and .gitignore'd files are skipped.", if f.timed_out { " within 2 seconds" } else { "" });
    }
    let mut out = format!("{kind} \"{query}\" (read only, inside your home area):\n");
    for (k, (p, d, l)) in f.hits.iter().enumerate() {
        let rel = p.strip_prefix(root).unwrap_or(p).display();
        out.push_str(&format!("{}. {}{}{}\n", k + 1, rel, if *d { "/" } else { "" }, l.as_ref().map(|l| format!("  {l}")).unwrap_or_default()));
    }
    if f.truncated {
        out.push_str(&format!("Showing the first {MAX_RESULTS}. Add a word to narrow it.\n"));
    }
    if f.timed_out {
        out.push_str("Stopped after 2 seconds, so this may be incomplete.\n");
    }
    out.push_str("Pick a folder from the chooser to list it, or ask for it by path.");
    out
}

/// Folders among the hits (inside the home area): what the chooser may offer.
pub fn dir_rows(f: &Found) -> Vec<String> {
    f.hits.iter().filter(|h| h.1).take(10).map(|h| h.0.to_string_lossy().to_string()).collect()
}

/// "fd words" / "rg text" (case-insensitive command word). None: not one of these commands.
pub fn parse(input: &str) -> Option<(bool, String)> {
    let t = input.trim();
    let (cmd, rest) = t.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    match cmd.to_lowercase().as_str() {
        "fd" => Some((false, rest.to_string())),
        "rg" => Some((true, rest.trim_matches(['"', '\'']).to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-find-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }
    #[test]
    fn names_and_text_respect_limits() {
        let r = tmp("a");
        std::fs::create_dir_all(r.join("docs/Models")).unwrap();
        std::fs::write(r.join("docs/notes.txt"), "alpha\nNeedle here\n").unwrap();
        std::fs::write(r.join("docs/.hidden-needle"), "Needle").unwrap();
        std::fs::write(r.join("bin.dat"), b"Needle\0\0").unwrap();
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(r.join("ignored.txt"), "Needle").unwrap();
        let f = find_names(&r, &["models".into()]);
        assert_eq!(f.hits.len(), 1);
        assert!(f.hits[0].1);
        assert!(find_names(&r, &["Models".into()]).hits.len() == 1 && find_names(&r, &["MODELS".into()]).hits.is_empty());
        let t = find_text(&r, "needle");
        let names: Vec<String> = t.hits.iter().map(|h| h.0.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["notes.txt"], "hidden, binary (and .gitignore in a git repo) are skipped: {names:?}");
        assert!(t.hits[0].2.as_ref().unwrap().starts_with("2: Needle here"));
    }
    #[test]
    fn parse_only_the_two_commands() {
        assert_eq!(parse("fd co models"), Some((false, "co models".into())));
        assert_eq!(parse("rg \"hello world\""), Some((true, "hello world".into())));
        assert!(parse("fd").is_none() && parse("find me").is_none() && parse("fdx a").is_none());
    }
}
