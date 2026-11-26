//! App launching: resolve a spoken app name to an exact, reviewable command line. No shell, no guessing.
//!
//! Matching is exact up to case and separators (space, hyphen, underscore are treated alike), against:
//! the desktop entry's `Name`, its desktop file id (and the last dotted segment of a reverse-DNS id),
//! the executable name in its `Exec` line, and, as a last resort, an executable with exactly that name on PATH.
//! A prefix, a first word or a "close" name is never accepted. Several different matches are reported
//! so the user can pick one by its exact id.
use crate::error::PawError;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCandidate {
    /// Desktop file id without ".desktop", or the executable name for a PATH match.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Full command: absolute executable first, then fixed arguments from the entry. Never run via a shell.
    pub argv: Vec<String>,
}

impl AppCandidate {
    pub fn command_line(&self) -> String {
        self.argv.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().split(|c: char| c.is_whitespace() || c == '-' || c == '_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Windows has no .desktop entries: only an exact executable name on PATH is found (Start Menu shortcuts are not read
/// yet, so apps that are not on PATH cannot be opened by name there).
#[cfg(windows)]
pub fn default_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn default_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => v.push(PathBuf::from(d).join("applications")),
        _ => {
            if let Some(h) = &home {
                v.push(h.join(".local/share/applications"));
            }
        }
    }
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let dirs = if dirs.is_empty() { "/usr/local/share:/usr/share".to_string() } else { dirs };
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join("applications"));
    }
    if let Some(h) = &home {
        v.push(h.join(".local/share/flatpak/exports/share/applications"));
    }
    v.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    v
}

/// Split an Exec line per the Desktop Entry spec (double quotes, backslash escapes), drop field codes.
pub fn parse_exec(line: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut in_q = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                in_q = !in_q;
                have = true;
            }
            '\\' if in_q => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !in_q => {
                if have {
                    args.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                cur.push(c);
                have = true;
            }
        }
    }
    if have {
        args.push(cur);
    }
    args.into_iter()
        .filter_map(|a| {
            let mut out = String::new();
            let mut ch = a.chars().peekable();
            while let Some(c) = ch.next() {
                if c == '%' {
                    match ch.next() {
                        Some('%') => out.push('%'),
                        Some(_) => {} // field code: no file or URL is passed
                        None => {}
                    }
                } else {
                    out.push(c);
                }
            }
            if out.is_empty() { None } else { Some(out) }
        })
        .collect()
}

/// Extensions Windows may launch directly. Scripts (.bat, .cmd) are not here on purpose: they go through the command
/// interpreter, which would break "no shell, exact argument vector".
#[cfg(windows)]
const EXE_EXTS: &[&str] = &["exe", "com"];
#[cfg(not(windows))]
const EXE_EXTS: &[&str] = &[];

#[cfg(windows)]
const PATH_SEP: char = ';';
#[cfg(not(windows))]
const PATH_SEP: char = ':';

fn which(exe: &str, path_env: &str) -> Option<PathBuf> {
    which_with(exe, path_env, PATH_SEP, EXE_EXTS, cfg!(windows))
}

/// PATH lookup with an explicit separator and extension list (the extension list is empty on Unix). On Windows a
/// bare name only matches `name.exe` / `name.com`, a name that already ends in one of those is taken as written, and
/// names with a path separator or drive colon are never searched for.
fn which_with(exe: &str, path_env: &str, sep: char, exts: &[&str], windows: bool) -> Option<PathBuf> {
    if windows {
        if exe.contains('/') || exe.contains('\\') || exe.contains(':') || exe.is_empty() {
            return None;
        }
        let lower = exe.to_lowercase();
        let names: Vec<String> = if exts.iter().any(|e| lower.ends_with(&format!(".{e}"))) {
            vec![exe.to_string()]
        } else {
            exts.iter().map(|e| format!("{exe}.{e}")).collect()
        };
        for d in path_env.split(sep).filter(|d| !d.is_empty()) {
            for n in &names {
                let p = Path::new(d).join(n);
                if std::fs::metadata(&p).map(|m| m.is_file()).unwrap_or(false) {
                    return Some(p);
                }
            }
        }
        return None;
    }
    if exe.contains('/') {
        let p = PathBuf::from(exe);
        let md = std::fs::metadata(&p).ok()?;
        return (crate::platform::is_executable(&md)).then_some(p);
    }
    for d in path_env.split(sep).filter(|d| !d.is_empty()) {
        let p = Path::new(d).join(exe);
        if let Ok(md) = std::fs::metadata(&p) {
            if crate::platform::is_executable(&md) {
                return Some(p);
            }
        }
    }
    None
}

struct Entry {
    id: String,
    name: String,
    exec: String,
    terminal: bool,
}

fn read_entry(path: &Path, id: String) -> Option<Entry> {
    let text = std::fs::read_to_string(path).ok()?;
    let (mut name, mut exec, mut ty, mut hidden, mut terminal) = (None, None, None, false, false);
    let mut in_main = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            match k.trim() {
                "Name" => name = Some(v.trim().to_string()),
                "Exec" => exec = Some(v.trim().to_string()),
                "Type" => ty = Some(v.trim().to_string()),
                "Hidden" => hidden = v.trim() == "true",
                "Terminal" => terminal = v.trim() == "true",
                _ => {}
            }
        }
    }
    if hidden || ty.as_deref() != Some("Application") {
        return None;
    }
    Some(Entry { id, name: name?, exec: exec?, terminal })
}

/// All distinct commands that match `spoken` exactly (see module docs).
pub fn find(spoken: &str, dirs: &[PathBuf], path_env: &str) -> Result<Vec<AppCandidate>, PawError> {
    let want = norm(spoken);
    if want.is_empty() {
        return Err(PawError::Validation("open what?".into()));
    }
    let mut by_id: Vec<AppCandidate> = Vec::new();
    let mut loose: Vec<AppCandidate> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut terminal_only: Vec<String> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "desktop").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            let id = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if id.is_empty() || !seen_ids.insert(id.clone()) {
                continue; // earlier directories (the user's own) win, as in the spec
            }
            let Some(e) = read_entry(&f, id.clone()) else { continue };
            let mut argv = parse_exec(&e.exec);
            if argv.is_empty() {
                continue;
            }
            let exe_base = Path::new(&argv[0]).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let last_seg = id.rsplit('.').next().unwrap_or(&id).to_string();
            let id_exact = norm(&id) == want;
            let loose_hit = norm(&e.name) == want || norm(&exe_base) == want || (id.contains('.') && norm(&last_seg) == want);
            if !(id_exact || loose_hit) {
                continue;
            }
            if e.terminal {
                terminal_only.push(e.name.clone());
                continue;
            }
            let Some(abs) = which(&argv[0], path_env) else { continue };
            argv[0] = abs.to_string_lossy().into_owned();
            let c = AppCandidate { id: id.clone(), name: e.name, argv };
            if id_exact {
                by_id.push(c);
            } else {
                loose.push(c);
            }
        }
    }
    if !by_id.is_empty() {
        return Ok(by_id);
    }
    // An executable with exactly this name on PATH (as typed, or with hyphens for spaces).
    let typed = spoken.trim();
    for exe in [typed.to_string(), typed.split_whitespace().collect::<Vec<_>>().join("-")] {
        if exe.is_empty() || exe.contains('/') {
            continue;
        }
        if let Some(p) = which(&exe, path_env) {
            let argv = vec![p.to_string_lossy().into_owned()];
            if !loose.iter().any(|c| c.argv == argv) {
                loose.push(AppCandidate { id: exe.clone(), name: exe, argv });
            }
        }
    }
    loose.dedup_by(|a, b| a.argv == b.argv);
    if loose.is_empty() && !terminal_only.is_empty() {
        return Err(PawError::Validation(format!(
            "\"{spoken}\" ({}) runs inside a terminal, and I don't launch terminal programs yet. Nothing was opened.",
            terminal_only.join(", ")
        )));
    }
    Ok(loose)
}

/// One exact choice, or an explanation of why not.
pub fn resolve_in(spoken: &str, dirs: &[PathBuf], path_env: &str) -> Result<AppCandidate, PawError> {
    let mut c = find(spoken, dirs, path_env)?;
    match c.len() {
        0 => Err(PawError::Validation(format!(
            "I couldn't find an app called \"{spoken}\". I looked for an installed app with exactly that name (its desktop entry, or a program of that name on PATH) and chose nothing. Nothing was opened."
        ))),
        1 => Ok(c.remove(0)),
        _ => {
            let list: Vec<String> = c.iter().map(|a| format!("  open {}   ({}: {})", a.id, a.name, a.command_line())).collect();
            Err(PawError::Validation(format!(
                "More than one app matches \"{spoken}\". Say which one by its exact id:\n{}\nNothing was opened.",
                list.join("\n")
            )))
        }
    }
}

pub fn resolve(spoken: &str) -> Result<AppCandidate, PawError> {
    resolve_in(spoken, &default_dirs(), &std::env::var("PATH").unwrap_or_default())
}

/// Start the reviewed command: no shell, no extra arguments, detached, no inherited stdio.
pub fn launch(argv: &[String]) -> Result<String, PawError> {
    use std::process::{Command, Stdio};
    let (exe, args) = argv.split_first().ok_or_else(|| PawError::Exec("nothing to launch".into()))?;
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    crate::platform::detach(&mut cmd);
    if let Some(h) = std::env::var_os("HOME") {
        cmd.current_dir(h);
    }
    let mut child = cmd.spawn().map_err(|e| PawError::Exec(format!("could not start {exe}: {e}")))?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(format!("Started {exe} (process {pid}). It runs on its own; Pawline does not watch it."))
}

#[cfg(test)]
mod windows_tests {
    use super::*;
    #[test]
    fn windows_lookup_is_exe_only_and_exact() {
        let d = std::env::temp_dir().join(format!("pawline-win-which-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for f in ["notepad.exe", "tool.cmd", "run.bat", "x.com"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let path = format!("/nonexistent;{}", d.display());
        let ex = ["exe", "com"];
        assert_eq!(which_with("notepad", &path, ';', &ex, true), Some(d.join("notepad.exe")));
        assert_eq!(which_with("NOTEPAD.EXE", &path, ';', &ex, true).is_some(), false, "case of the file on disk is the filesystem's job; Linux FS is case-sensitive");
        assert_eq!(which_with("x", &path, ';', &ex, true), Some(d.join("x.com")));
        assert!(which_with("tool", &path, ';', &ex, true).is_none(), "scripts are refused");
        assert!(which_with("run.bat", &path, ';', &ex, true).is_none(), "scripts are refused");
        assert!(which_with("..\\notepad", &path, ';', &ex, true).is_none(), "path separators are refused");
        assert!(which_with("C:notepad", &path, ';', &ex, true).is_none(), "drive prefixes are refused");
        assert!(which_with("", &path, ';', &ex, true).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile_dir::D, Vec<PathBuf>, String) {
        let d = tempfile_dir::D::new();
        let apps = d.0.join("applications");
        let bin = d.0.join("bin");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let mk = |n: &str| {
            let p = bin.join(n);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        mk("zen-browser");
        mk("zen");
        mk("browser");
        mk("mpv");
        std::fs::write(apps.join("zen-browser.desktop"), "[Desktop Entry]\nType=Application\nName=Zen Browser\nExec=zen-browser %u\n").unwrap();
        std::fs::write(apps.join("mpv.desktop"), "[Desktop Entry]\nType=Application\nName=mpv Media Player\nExec=mpv --player-operation-mode=pseudo-gui -- %U\n").unwrap();
        let path = bin.to_string_lossy().into_owned();
        (d, vec![apps], path)
    }

    mod tempfile_dir {
        pub struct D(pub std::path::PathBuf);
        impl D {
            pub fn new() -> D {
                let p = std::env::temp_dir().join(format!("pawline-apps-{}-{:?}", std::process::id(), std::thread::current().id()));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).unwrap();
                D(p)
            }
        }
        impl Drop for D {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn spoken_name_matches_exactly_up_to_separators() {
        let (_d, dirs, path) = fixture();
        for s in ["zen browser", "zen-browser", "Zen Browser", "ZEN_BROWSER"] {
            let c = resolve_in(s, &dirs, &path).unwrap();
            assert_eq!(c.id, "zen-browser");
            assert!(c.argv[0].ends_with("/bin/zen-browser"));
            assert_eq!(c.argv.len(), 1, "%u is dropped, no file or URL is passed");
        }
    }

    #[test]
    fn first_word_or_partial_name_is_never_chosen() {
        let (_d, dirs, path) = fixture();
        // "zen" exists as a program too, so asking for "zen" is exact and fine, but "browser zen" is not zen-browser.
        assert!(resolve_in("browser zen", &dirs, &path).is_err());
        assert!(resolve_in("zen brow", &dirs, &path).is_err());
        assert!(resolve_in("firefox", &dirs, &path).is_err());
        let c = resolve_in("zen", &dirs, &path).unwrap();
        assert!(c.argv[0].ends_with("/bin/zen"), "exact name only");
    }

    #[test]
    fn exec_fixed_arguments_are_kept_field_codes_dropped() {
        let (_d, dirs, path) = fixture();
        let c = resolve_in("mpv", &dirs, &path).unwrap();
        assert_eq!(c.id, "mpv");
        assert_eq!(c.argv[1..], ["--player-operation-mode=pseudo-gui", "--"]);
        assert_eq!(parse_exec(r#""/opt/My App/run" --x "a b" %F 100%%"#), vec!["/opt/My App/run", "--x", "a b", "100%"]);
    }

    #[test]
    fn path_like_and_shell_like_names_are_refused() {
        let (_d, dirs, path) = fixture();
        for s in ["/bin/sh", "mpv; rm -rf ~", "$(id)", "../bin/mpv", ""] {
            assert!(resolve_in(s, &dirs, &path).is_err(), "{s}");
        }
    }

    #[test]
    fn ambiguity_asks_for_an_exact_id() {
        let (d, dirs, path) = fixture();
        std::fs::write(dirs[0].join("org.example.zenbrowser.desktop"), "[Desktop Entry]\nType=Application\nName=Zen Browser\nExec=/bin/sh\n").unwrap();
        let _ = &d;
        let e = resolve_in("zen browser", &dirs, &path);
        // the file id "zen-browser" matches exactly, so it wins over the same-named entry
        assert_eq!(e.unwrap().id, "zen-browser");
        std::fs::remove_file(dirs[0].join("zen-browser.desktop")).unwrap();
        std::fs::write(dirs[0].join("zen2.desktop"), "[Desktop Entry]\nType=Application\nName=Zen Browser\nExec=zen-browser\n").unwrap();
        let e = resolve_in("zen browser", &dirs, &path).unwrap_err().to_string();
        assert!(e.contains("More than one") || e.contains("zen2"), "{e}");
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use crate::{context::ContextManager, intent, validate::Validator};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn open_app_plan_shows_exact_command_needs_confirmation_and_unknown_names_are_refused() {
        let d = std::env::temp_dir().join(format!("pawline-openplan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (apps, bin, root) = (d.join("apps"), d.join("bin"), d.join("root"));
        for p in [&apps, &bin, &root] {
            std::fs::create_dir_all(p).unwrap();
        }
        let exe = bin.join("mpv");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut v = Validator::new(root);
        v.app_env = Some((vec![apps], bin.to_string_lossy().into_owned()));
        let ctx = ContextManager::new();
        let plan = v.build_plan(&intent::parse("open mpv").unwrap(), &ctx).unwrap();
        assert!(plan.needs_confirmation, "launching a program always shows the command first");
        let text = plan.ops[0].describe();
        assert!(text.contains(&exe.to_string_lossy().into_owned()), "{text}");
        let err = v.build_plan(&intent::parse("open zen browser").unwrap(), &ctx).unwrap_err().to_string();
        assert!(err.contains("couldn't find an app called \"zen browser\""), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
