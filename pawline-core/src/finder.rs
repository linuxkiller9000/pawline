//! "fd <words>" (find names) and "rg <text>" (find text inside files), read only.
//! Built on the same libraries the fd and ripgrep tools use (the `ignore` and `grep-*` crates, MIT or
//! Unlicense), so no other program has to be installed. Like those tools they skip hidden files and
//! anything .gitignore'd, never follow symlinks, and here also stay inside the home area, skip Pawline's
//! own journal, stop after a time limit and a result limit, and never change anything.
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{sinks::UTF8, BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Searches allowed to run (or be stuck) at once. A hung read keeps its worker, so new searches are refused past this.
pub const MAX_WORKERS: usize = 2;
static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub const MAX_RESULTS: usize = 30;
pub const TIME_LIMIT: Duration = Duration::from_secs(2);
const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_LINE: usize = 160;

#[derive(Default, Clone)]
pub struct Found {
    pub hits: Vec<(PathBuf, bool, Option<String>)>, // path, is_dir, matching line (rg)
    pub truncated: bool,
    pub timed_out: bool,
    /// Files not read: over 2 MB, unreadable, or changed into something else while searching.
    pub skipped: usize,
    pub scanned: usize,
    /// Not run: earlier searches are still stuck on a slow disk.
    pub busy: bool,
}

/// Where a search may look. The root is canonical; folders in `exclude` (Pawline's own data) are never entered.
#[derive(Clone)]
pub struct Scope {
    pub root: PathBuf,
    pub exclude: Vec<PathBuf>,
    pub limit: Duration,
}

impl Scope {
    pub fn new(root: &Path, mut exclude: Vec<PathBuf>) -> Option<Scope> {
        let root = std::fs::canonicalize(root).ok()?;
        exclude.push(root.join(crate::journal::STORE));
        let exclude = exclude.into_iter().filter_map(|p| std::fs::canonicalize(&p).ok().or(Some(p))).collect();
        Some(Scope { root, exclude, limit: TIME_LIMIT })
    }
}

fn walker(sc: &Scope) -> ignore::Walk {
    let ex = sc.exclude.clone();
    WalkBuilder::new(&sc.root).follow_links(false).filter_entry(move |e| !ex.iter().any(|x| e.path().starts_with(x))).build()
}

/// Run `work` on a worker thread and wait at most the limit (plus a little). The UI never waits on a slow disk:
/// on a timeout the hits so far are returned, marked incomplete, and the worker is told to stop.
fn run(sc: &Scope, work: impl FnOnce(&Scope, &AtomicBool, &Mutex<Found>) + Send + 'static) -> Found {
    if ACTIVE.fetch_add(1, Ordering::SeqCst) >= MAX_WORKERS {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
        return Found { busy: true, ..Found::default() };
    }
    let slot = Slot;
    let stop = Arc::new(AtomicBool::new(false));
    let out = Arc::new(Mutex::new(Found::default()));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    {
        let (sc, stop, out) = (sc.clone(), stop.clone(), out.clone());
        std::thread::spawn(move || {
            let _slot = slot;
            work(&sc, &stop, &out);
            let _ = tx.send(());
        });
    }
    let finished = rx.recv_timeout(sc.limit + Duration::from_millis(500)).is_ok();
    stop.store(true, Ordering::SeqCst);
    let mut f = out.lock().map(|g| g.clone()).unwrap_or_default();
    if !finished {
        f.timed_out = true;
    }
    f
}

/// Names containing every word (smart case: a word with a capital letter is case-sensitive). Folders and files.
pub fn find_names(sc: &Scope, words: &[String]) -> Found {
    let words = words.to_vec();
    run(sc, move |sc, stop, out| {
        if words.is_empty() {
            return;
        }
        let start = Instant::now();
        let sens: Vec<bool> = words.iter().map(|w| w.chars().any(|c| c.is_uppercase())).collect();
        for ent in walker(sc).flatten() {
            if stop.load(Ordering::SeqCst) || start.elapsed() > sc.limit {
                if let Ok(mut f) = out.lock() {
                    f.timed_out = true;
                }
                return;
            }
            if ent.path() == sc.root {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            let low = name.to_lowercase();
            if words.iter().zip(&sens).all(|(w, s)| if *s { name.contains(w.as_str()) } else { low.contains(&w.to_lowercase()) }) {
                let Ok(mut f) = out.lock() else { return };
                if f.hits.len() == MAX_RESULTS {
                    f.truncated = true;
                    return;
                }
                f.hits.push((ent.path().to_path_buf(), ent.file_type().is_some_and(|t| t.is_dir()), None));
            }
        }
    })
}

/// Open a file relative to the root DIRECTORY HANDLE (cap-std: no ".." or symlink can lead outside the root, and a parent
/// folder swapped for a link cannot escape either, because every step is resolved under the handle), without blocking on
/// a pipe, and require a regular file of at most 2 MB.
fn open_checked(root: &cap_std::fs::Dir, path: &Path, root_path: &Path) -> Option<std::fs::File> {
    use cap_std::fs::OpenOptionsExt;
    let rel = path.strip_prefix(root_path).ok()?;
    let mut o = cap_std::fs::OpenOptions::new();
    o.read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
    let f = root.open_with(rel, &o).ok()?.into_std();
    let md = f.metadata().ok()?;
    (md.is_file() && md.len() <= MAX_FILE).then_some(f)
}

/// Lines containing the text (literal, smart case) in text files up to 2 MB. Binary files (a NUL byte) are skipped.
pub fn find_text(sc: &Scope, text: &str) -> Found {
    let text = text.to_string();
    run(sc, move |sc, stop, out| {
        if text.trim().is_empty() {
            return;
        }
        let start = Instant::now();
        let smart = !text.chars().any(|c| c.is_uppercase());
        let Ok(m) = RegexMatcherBuilder::new().case_insensitive(smart).fixed_strings(true).build(&text) else { return };
        let Ok(rootdir) = cap_std::fs::Dir::open_ambient_dir(&sc.root, cap_std::ambient_authority()) else { return };
        let mut s = SearcherBuilder::new().binary_detection(BinaryDetection::quit(0)).line_number(true).build();
        for ent in walker(sc).flatten() {
            if stop.load(Ordering::SeqCst) || start.elapsed() > sc.limit {
                if let Ok(mut f) = out.lock() {
                    f.timed_out = true;
                }
                return;
            }
            if !ent.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Some(file) = open_checked(&rootdir, ent.path(), &sc.root) else {
                if let Ok(mut f) = out.lock() {
                    f.skipped += 1;
                }
                continue;
            };
            if let Ok(mut f) = out.lock() {
                f.scanned += 1;
            }
            let mut first: Option<String> = None;
            let _ = s.search_file(&m, &file, UTF8(|n, line| {
                first = Some(format!("{n}: {}", line.trim().chars().take(MAX_LINE).collect::<String>()));
                Ok(false)
            }));
            if let Some(l) = first {
                let Ok(mut f) = out.lock() else { return };
                if f.hits.len() == MAX_RESULTS {
                    f.truncated = true;
                    return;
                }
                f.hits.push((ent.path().to_path_buf(), false, Some(l)));
            }
        }
    })
}

pub fn render(kind: &str, query: &str, root: &Path, f: &Found) -> String {
    if f.busy {
        return format!("{kind}: not run, because earlier searches are still waiting on a slow disk. Try again in a moment.");
    }
    if f.hits.is_empty() {
        return if f.timed_out {
            format!("{kind} \"{query}\": nothing found yet, but I stopped after 2 seconds, so this search is incomplete. Add a word to narrow it.")
        } else {
            format!("{kind} \"{query}\": searched everything I can look at in your home area and found no match.{} Hidden files, .gitignore'd files (inside git repos), binary files and files over 2 MB are not searched.", if f.skipped > 0 { format!(" ({} file(s) could not be read.)", f.skipped) } else { String::new() })
        };
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
    static LOCK: Mutex<()> = Mutex::new(());
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-find-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }
    #[test]
    fn names_and_text_respect_limits() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = tmp("a");
        std::fs::create_dir_all(r.join("docs/Models")).unwrap();
        std::fs::write(r.join("docs/notes.txt"), "alpha\nNeedle here\n").unwrap();
        std::fs::write(r.join("docs/.hidden-needle"), "Needle").unwrap();
        std::fs::write(r.join("bin.dat"), b"Needle\0\0").unwrap();
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(r.join("ignored.txt"), "Needle").unwrap();
        std::fs::write(r.join("big.txt"), "Needle ".repeat(400_000)).unwrap();
        let sc = Scope::new(&r, vec![]).unwrap();
        let f = find_names(&sc, &["models".into()]);
        assert_eq!(f.hits.len(), 1);
        assert!(f.hits[0].1);
        assert!(find_names(&sc, &["Models".into()]).hits.len() == 1 && find_names(&sc, &["MODELS".into()]).hits.is_empty());
        let t = find_text(&sc, "needle");
        let names: Vec<String> = t.hits.iter().map(|h| h.0.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["notes.txt"], "hidden, binary, .gitignore'd and over-2MB files are skipped: {names:?}");
        assert!(t.hits[0].2.as_ref().unwrap().starts_with("2: Needle here"));
        assert!(t.skipped >= 1, "the big file is reported as not read");
        let none = find_text(&sc, "zzzqqq");
        assert!(none.hits.is_empty() && !none.timed_out);
        assert!(render("rg", "zzzqqq", &r, &none).contains("found no match"));
    }

    #[test]
    fn symlinks_and_excluded_folders_are_never_used() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = tmp("b");
        let out = tmp("b-outside");
        std::fs::write(out.join("secret.txt"), "Needle outside").unwrap();
        std::os::unix::fs::symlink(&out, r.join("linkdir")).unwrap();
        std::os::unix::fs::symlink(out.join("secret.txt"), r.join("linkfile.txt")).unwrap();
        std::fs::create_dir_all(r.join("pawdata")).unwrap();
        std::fs::write(r.join("pawdata/mem.json"), "Needle private").unwrap();
        std::fs::write(r.join("ok.txt"), "Needle ok").unwrap();
        let sc = Scope::new(&r, vec![r.join("pawdata")]).unwrap();
        let t = find_text(&sc, "needle");
        let names: Vec<String> = t.hits.iter().map(|h| h.0.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["ok.txt"], "{names:?}");
        assert!(find_names(&sc, &["mem".into()]).hits.is_empty());
        assert!(find_names(&sc, &["secret".into()]).hits.is_empty());
        // opens are resolved under the root handle: a leaf link to outside, or a parent folder swapped for a link, cannot escape
        let rd = cap_std::fs::Dir::open_ambient_dir(&sc.root, cap_std::ambient_authority()).unwrap();
        assert!(open_checked(&rd, &r.join("linkfile.txt"), &sc.root).is_none());
        assert!(open_checked(&rd, &r.join("linkdir/secret.txt"), &sc.root).is_none());
        assert!(open_checked(&rd, &r.join("ok.txt"), &sc.root).is_some());
    }

    #[test]
    fn a_pipe_does_not_hang_and_time_limit_is_reported() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = tmp("c");
        let fifo = r.join("pipe.txt");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        std::fs::write(r.join("a.txt"), "Needle").unwrap();
        let sc = Scope::new(&r, vec![]).unwrap();
        let t0 = Instant::now();
        let t = find_text(&sc, "needle");
        assert!(t0.elapsed() < Duration::from_secs(1) && t.hits.len() == 1, "pipe skipped, others searched");
        let mut zero = sc.clone();
        zero.limit = Duration::from_secs(0);
        let f = find_names(&zero, &["a".into()]);
        assert!(f.timed_out || f.hits.len() == 1);
    }

    #[test]
    fn overload_is_refused_not_queued() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = tmp("d");
        let sc = Scope::new(&r, vec![]).unwrap();
        ACTIVE.fetch_add(MAX_WORKERS, Ordering::SeqCst); // pretend two searches are stuck
        let f = find_names(&sc, &["a".into()]);
        ACTIVE.fetch_sub(MAX_WORKERS, Ordering::SeqCst);
        assert!(f.busy && render("fd", "a", &r, &f).contains("not run"));
        assert!(!find_names(&sc, &["a".into()]).busy, "slots are released when workers finish");
    }

    #[test]
    fn parse_only_the_two_commands() {
        assert_eq!(parse("fd co models"), Some((false, "co models".into())));
        assert_eq!(parse("rg \"hello world\""), Some((true, "hello world".into())));
        assert!(parse("fd").is_none() && parse("find me").is_none() && parse("fdx a").is_none());
    }
}
