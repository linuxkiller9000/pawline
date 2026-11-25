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

/// Pick an adapter. `have(program)` says whether a program is installed.
/// On KDE, Plasma's own tool wins. Elsewhere awww (animated, daemon-based)
/// comes before swaybg. None when nothing usable is installed.
pub fn choose(have: &dyn Fn(&str) -> bool, desktop: &str) -> Option<Adapter> {
    let kde = desktop.to_lowercase().split(':').any(|d| d == "kde");
    let order: &[Adapter] = if kde {
        &[Adapter::Plasma, Adapter::Awww, Adapter::Swaybg]
    } else {
        &[Adapter::Awww, Adapter::Swaybg, Adapter::Plasma]
    };
    order.iter().copied().find(|a| {
        // Plasma's tool exists on non-KDE systems only if Plasma is installed;
        // it cannot work outside a Plasma session, so require KDE for it.
        (*a != Adapter::Plasma || kde) && have(a.program())
    })
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
    fn set(&self, image: &Path) -> Result<String, PawError>;
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}

pub struct SystemWallpaper;

impl WallpaperSetter for SystemWallpaper {
    fn set(&self, image: &Path) -> Result<String, PawError> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let adapter = choose(&in_path, &desktop).ok_or_else(|| {
            PawError::Exec(
                "no wallpaper tool found (looked for awww, swaybg, and plasma-apply-wallpaperimage on KDE)"
                    .into(),
            )
        })?;
        run(adapter, image)?;
        Ok(format!("wallpaper set with {}", adapter.program()))
    }
}

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
/// flicker), then stop only the swaybg we started earlier, found through our
/// own pid file and confirmed by process name. We never touch other swaybg
/// processes.
fn run_swaybg(mut cmd: std::process::Command) -> Result<(), PawError> {
    use std::process::Stdio;
    let pidfile = pid_file();
    let old: Option<u32> = pidfile
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok());
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
    if let Some(p) = &pidfile {
        let _ = std::fs::write(p, child.id().to_string());
    }
    if let Some(pid) = old {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim() == "swaybg" && pid != child.id() {
            terminate(pid);
        }
    }
    // Leave it running; dropping the handle does not kill it.
    drop(child);
    Ok(())
}

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
    fn kde_prefers_plasma_tool() {
        let h = have(&["awww", "plasma-apply-wallpaperimage", "swaybg"]);
        assert_eq!(choose(&h, "KDE"), Some(Adapter::Plasma));
    }

    #[test]
    fn plasma_tool_ignored_outside_kde() {
        let h = have(&["plasma-apply-wallpaperimage"]);
        assert_eq!(choose(&h, "niri"), None);
    }

    #[test]
    fn niri_prefers_awww_then_swaybg() {
        assert_eq!(choose(&have(&["awww", "swaybg"]), "niri"), Some(Adapter::Awww));
        assert_eq!(choose(&have(&["swaybg"]), "niri"), Some(Adapter::Swaybg));
        assert_eq!(choose(&have(&[]), "niri"), None);
    }

    #[test]
    fn extension_check() {
        assert!(extension_ok(Path::new("/x/y.PNG")));
        assert!(extension_ok(Path::new("/x/y.jpeg")));
        assert!(!extension_ok(Path::new("/x/y.sh")));
        assert!(!extension_ok(Path::new("/x/y")));
    }
}
