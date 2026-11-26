//! Wallpaper adapters. Fixed program + argument vector, never a shell.
//!
//! Supported (syntax checked against each tool's own docs):
//! - KDE Plasma: `plasma-apply-wallpaperimage <image>`
//! - awww (the renamed swww): `awww img <image>` (needs `awww-daemon` running)
//! - swaybg: `swaybg -i <image> -m fill` (stays running; we track our own pid)
//!
//! The image path arrives already validated (inside the home area, regular
//! file, image extension, confirmed by the user). The adapter choice is a pure
//! function of what is installed and the desktop name, so it is unit-testable
//! without running anything.

use std::path::{Path, PathBuf};

use crate::error::PawError;

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Plasma,
    Awww,
    Swaybg,
}

impl Adapter {
    pub fn program(self) -> &'static str {
        match self {
            Adapter::Plasma => "plasma-apply-wallpaperimage",
            Adapter::Awww => "awww",
            Adapter::Swaybg => "swaybg",
        }
    }

    /// The exact argument vector for this adapter.
    pub fn args(self, image: &Path) -> Vec<String> {
        let img = image.to_string_lossy().to_string();
        match self {
            Adapter::Plasma => vec![img],
            Adapter::Awww => vec!["img".into(), img],
            Adapter::Swaybg => vec!["-i".into(), img, "-m".into(), "fill".into()],
        }
    }
}

impl Adapter {
    pub fn from_name(n: &str) -> Option<Adapter> {
        match n.trim().to_lowercase().as_str() {
            "awww" | "swww" => Some(Adapter::Awww),
            "swaybg" => Some(Adapter::Swaybg),
            "plasma" | "kde" | "plasma-apply-wallpaperimage" => Some(Adapter::Plasma),
            _ => None,
        }
    }

    /// The process whose presence shows this adapter is the live wallpaper manager.
    pub fn daemon(self) -> &'static str {
        match self {
            Adapter::Plasma => "plasmashell",
            Adapter::Awww => "awww-daemon",
            Adapter::Swaybg => "swaybg",
        }
    }
}

/// Pick the adapter, never guessing between wallpaper managers.
/// - `explicit`: the user named one; it must be installed.
/// - Otherwise the candidates are installed adapters whose manager is
///   actually running. Exactly one candidate wins.
/// - With no manager running, a single installed adapter is used only if it
///   is swaybg (it starts its own process). awww and Plasma need their daemon.
/// - Anything else is an error that tells the user how to choose.
pub fn choose_active(
    have: &dyn Fn(&str) -> bool,
    running: &dyn Fn(&str) -> bool,
    explicit: Option<Adapter>,
) -> Result<Adapter, String> {
    let all = [Adapter::Plasma, Adapter::Awww, Adapter::Swaybg];
    if let Some(a) = explicit {
        return if have(a.program()) {
            Ok(a)
        } else {
            Err(format!("{} is not installed", a.program()))
        };
    }
    let installed: Vec<Adapter> = all.iter().copied().filter(|a| have(a.program())).collect();
    let active: Vec<Adapter> = installed.iter().copied().filter(|a| running(a.daemon())).collect();
    match (active.len(), installed.len()) {
        (1, _) => Ok(active[0]),
        (0, 0) => Err("no wallpaper tool found (looked for awww, swaybg and plasma-apply-wallpaperimage)".into()),
        (0, 1) if installed[0] == Adapter::Swaybg => Ok(Adapter::Swaybg),
        (0, 1) => Err(format!(
            "{} is installed but its manager ({}) is not running",
            installed[0].program(),
            installed[0].daemon()
        )),
        _ => {
            let names: Vec<&str> = (if active.is_empty() { &installed } else { &active })
                .iter()
                .map(|a| a.program())
                .collect();
            Err(format!(
                "I can't tell which wallpaper manager you use ({}). Say \"using awww\", \"using swaybg\" or \"using plasma\".",
                names.join(", ")
            ))
        }
    }
}

pub fn extension_ok(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Something that can set the wallpaper. The real one runs the adapter;
/// tests substitute a recorder.
pub trait WallpaperSetter: Send + Sync {
    fn set(&self, image: &Path, via: Option<Adapter>) -> Result<String, PawError>;
    /// The picture currently shown, when it can be read reliably. Default: unknown.
    fn current(&self) -> Option<PathBuf> {
        None
    }
}

/// The image of the single user-owned swaybg process, read from its command
/// line (`-i <path>`). None when there is no swaybg or more than one.
pub fn current_swaybg_image() -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let n = e.file_name();
        if !n.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let owned = crate::platform::same_owner_as_self(&e.path()).unwrap_or(false);
        if !owned || std::fs::read_to_string(e.path().join("comm")).map(|c| c.trim() != "swaybg").unwrap_or(true) {
            continue;
        }
        let raw = std::fs::read(e.path().join("cmdline")).ok()?;
        let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).to_string()).collect();
        let img = args.iter().position(|a| a == "-i" || a == "--image").and_then(|i| args.get(i + 1));
        match img {
            Some(p) => found.push(PathBuf::from(p)),
            None => return None,
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

/// True when a process with this exact name (from /proc/<pid>/comm) runs
/// under the current user. comm is truncated to 15 chars by the kernel.
fn process_running(name: &str) -> bool {
    let want: String = name.chars().take(15).collect();
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    rd.flatten().any(|e| {
        let n = e.file_name();
        let n = n.to_string_lossy();
        if !n.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let owned = crate::platform::same_owner_as_self(&e.path()).unwrap_or(false);
        owned
            && std::fs::read_to_string(e.path().join("comm"))
                .map(|c| c.trim() == want)
                .unwrap_or(false)
    })
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}

pub struct SystemWallpaper;

impl WallpaperSetter for SystemWallpaper {
    fn current(&self) -> Option<PathBuf> {
        #[cfg(windows)]
        return windows_current();
        #[cfg(not(windows))]
        current_swaybg_image()
    }

    fn set(&self, image: &Path, via: Option<Adapter>) -> Result<String, PawError> {
        #[cfg(windows)]
        {
            if via.is_some() {
                return Err(PawError::Exec("on Windows the wallpaper is set with the system call only; naming a Linux wallpaper tool does not apply".into()));
            }
            windows_check(image).map_err(PawError::Exec)?;
            windows_set(image)?;
            return Ok("wallpaper set with the Windows system call (SystemParametersInfoW)".into());
        }
        #[cfg(not(windows))]
        {
            let adapter = choose_active(&in_path, &process_running, via).map_err(PawError::Exec)?;
            run(adapter, image)?;
            Ok(format!("wallpaper set with {}", adapter.program()))
        }
    }
}

/// What the Windows wallpaper call is allowed to receive: an absolute, existing, ordinary file with an image extension
/// the system call can read. Pure checks, so they run on any OS; the call itself is Windows-only and unverified.
pub fn windows_check(image: &Path) -> Result<(), String> {
    if !image.is_absolute() {
        return Err("the wallpaper path must be absolute".into());
    }
    if image.to_string_lossy().contains('\0') {
        return Err("bad wallpaper path".into());
    }
    let ext = image.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "bmp" | "jpg" | "jpeg" | "png") {
        return Err("Windows can set bmp, jpg or png pictures here; this file type is not supported".into());
    }
    match std::fs::symlink_metadata(image) {
        Ok(m) if m.is_file() => Ok(()),
        _ => Err("the wallpaper file does not exist or is not an ordinary file".into()),
    }
}

#[cfg(windows)]
fn windows_set(image: &Path) -> Result<(), PawError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut w: Vec<u16> = image.as_os_str().encode_wide().collect();
    w.push(0);
    // SAFETY: w is NUL-terminated and lives for the call. Nothing is written back.
    let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, w.as_mut_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    if ok == 0 { Err(PawError::Exec("Windows refused to set the wallpaper".into())) } else { Ok(()) }
}

#[cfg(windows)]
fn windows_current() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut buf = vec![0u16; 520];
    // SAFETY: buf holds 520 UTF-16 units, the size passed in.
    let ok = unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, buf.as_mut_ptr() as *mut _, 0) };
    if ok == 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0)?;
    if n == 0 {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
}

#[cfg(not(windows))]
fn run(adapter: Adapter, image: &Path) -> Result<(), PawError> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(adapter.program());
    cmd.args(adapter.args(image)).stdin(Stdio::null());
    if adapter == Adapter::Swaybg {
        return run_swaybg(cmd);
    }
    let out = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| PawError::Exec(format!("could not run {}: {e}", adapter.program())))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let hint = if adapter == Adapter::Awww {
            " (is awww-daemon running?)"
        } else {
            ""
        };
        Err(PawError::Exec(format!(
            "{} failed: {}{hint}",
            adapter.program(),
            err.trim().lines().next().unwrap_or("no message")
        )))
    }
}

/// swaybg keeps running to draw the image. Start the new one first (no
/// flicker), then stop only the swaybg Pawline started earlier. The pid file
/// stores "pid starttime"; we stop a process only if its start time, name and
/// command line (our private wallpaper store) all still match, so a reused pid
/// or somebody else's swaybg is never touched.
fn run_swaybg(mut cmd: std::process::Command) -> Result<(), PawError> {
    use std::process::Stdio;
    let pidfile = pid_file();
    let old: Option<(u32, u64)> = pidfile
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| {
            let mut it = s.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        });
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| PawError::Exec(format!("could not run swaybg: {e}")))?;
    std::thread::sleep(std::time::Duration::from_millis(600));
    if let Ok(Some(st)) = child.try_wait() {
        return Err(PawError::Exec(format!(
            "swaybg exited immediately ({st}); is a layer-shell compositor running?"
        )));
    }
    let new_pid = child.id();
    if let (Some(p), Some(st)) = (&pidfile, proc_start_time(new_pid)) {
        let _ = std::fs::write(p, format!("{new_pid} {st}"));
    }
    if let Some((pid, st)) = old {
        if pid != new_pid && is_our_swaybg(pid, st) {
            terminate(pid);
        }
    }
    drop(child);
    Ok(())
}

/// Field 22 of /proc/<pid>/stat (start time in clock ticks since boot).
fn proc_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = &stat[stat.rfind(')')? + 2..];
    after.split_whitespace().nth(19)?.parse().ok()
}

fn is_our_swaybg(pid: u32, start: u64) -> bool {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
    comm.trim() == "swaybg"
        && proc_start_time(pid) == Some(start)
        && cmdline.contains(".local/share/pawline/wallpapers/")
}

#[cfg(not(unix))]
fn terminate(_pid: u32) {} // swaybg is a Linux program; nothing to stop here

#[cfg(unix)]
fn terminate(pid: u32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGTERM: i32 = 15;
    // Only reached after /proc/<pid>/comm matched "swaybg" from our pid file.
    unsafe {
        kill(pid as i32, SIGTERM);
    }
}

fn pid_file() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("pawline-swaybg.pid"))
}

#[cfg(test)]
mod windows_tests {
    use super::*;
    #[test]
    fn windows_check_refuses_unsafe_inputs() {
        let d = std::env::temp_dir().join(format!("pawline-wp-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let png = d.join("a.png");
        std::fs::write(&png, b"x").unwrap();
        assert!(windows_check(&png).is_ok());
        assert!(windows_check(Path::new("a.png")).is_err(), "relative");
        assert!(windows_check(&d.join("a.svg")).is_err(), "type");
        assert!(windows_check(&d.join("missing.png")).is_err(), "missing");
        assert!(windows_check(&d).is_err(), "folder");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have<'a>(set: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |p| set.contains(&p)
    }

    #[test]
    fn argv_matches_each_tools_documented_syntax() {
        let p = Path::new("/home/u/a b.png");
        assert_eq!(Adapter::Plasma.args(p), ["/home/u/a b.png"]);
        assert_eq!(Adapter::Awww.args(p), ["img", "/home/u/a b.png"]);
        assert_eq!(Adapter::Swaybg.args(p), ["-i", "/home/u/a b.png", "-m", "fill"]);
    }

    #[test]
    fn single_running_manager_wins() {
        let inst = have(&["awww", "swaybg", "plasma-apply-wallpaperimage"]);
        let run = have(&["awww-daemon"]);
        assert_eq!(choose_active(&inst, &run, None), Ok(Adapter::Awww));
        let run = have(&["plasmashell"]);
        assert_eq!(choose_active(&inst, &run, None), Ok(Adapter::Plasma));
    }

    #[test]
    fn two_running_or_none_running_with_many_installed_asks_the_user() {
        let inst = have(&["awww", "swaybg"]);
        assert!(choose_active(&inst, &have(&["awww-daemon", "swaybg"]), None).is_err());
        assert!(choose_active(&inst, &have(&[]), None).is_err());
        // explicit choice settles it
        assert_eq!(choose_active(&inst, &have(&[]), Some(Adapter::Swaybg)), Ok(Adapter::Swaybg));
        assert!(choose_active(&have(&["swaybg"]), &have(&[]), Some(Adapter::Awww)).is_err());
    }

    #[test]
    fn lone_installs() {
        assert_eq!(choose_active(&have(&["swaybg"]), &have(&[]), None), Ok(Adapter::Swaybg));
        assert!(choose_active(&have(&["awww"]), &have(&[]), None).is_err());
        assert!(choose_active(&have(&[]), &have(&[]), None).is_err());
    }

    #[test]
    fn names() {
        assert_eq!(Adapter::from_name("SWWW"), Some(Adapter::Awww));
        assert_eq!(Adapter::from_name("kde"), Some(Adapter::Plasma));
        assert_eq!(Adapter::from_name("feh"), None);
    }

    #[test]
    fn extension_check() {
        assert!(extension_ok(Path::new("/x/y.PNG")));
        assert!(extension_ok(Path::new("/x/y.jpeg")));
        assert!(!extension_ok(Path::new("/x/y.sh")));
        assert!(!extension_ok(Path::new("/x/y")));
    }
}
