//! Tiny local control socket so a panel widget (Waybar, Polybar, xfce4 genmon, a KDE plasmoid) can show
//! Pawline's state and open or close it. It accepts four fixed words only: toggle, show, hide, status.
//! No text from the socket is ever treated as a request, a command or a path. The socket lives in
//! $XDG_RUNTIME_DIR (mode 0600, owner only).
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ctl {
    Toggle,
    Show,
    Hide,
}

#[cfg(windows)]
pub fn ask(_word: &str) -> Result<String, String> {
    Err("the panel control socket is not available on Windows".into())
}
#[cfg(windows)]
pub fn running() -> bool {
    false
}
#[cfg(windows)]
pub fn serve(_tx: calloop::channel::Sender<Ctl>, _state: Shared) -> bool {
    false
}

pub type Shared = Arc<Mutex<&'static str>>;

#[cfg(unix)]
pub fn sock_path() -> Option<PathBuf> {
    let d = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(d).join("pawline.sock"))
}

/// Our own user id (from /proc/self, no libc needed).
#[cfg(unix)]
fn my_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// The socket path is only used when it is a real socket owned by us, in a folder owned by us that
/// nobody else can write to. A foreign or odd file is never connected to and never deleted.
#[cfg(unix)]
pub fn path_ok(p: &std::path::Path, uid: u32) -> Result<(), String> {
    let dir = p.parent().ok_or("no parent folder")?;
    let dm = std::fs::metadata(dir).map_err(|e| e.to_string())?;
    if dm.uid() != uid || dm.mode() & 0o022 != 0 {
        return Err("socket folder is not private to this user".into());
    }
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_socket() && m.uid() == uid => Ok(()),
        Ok(_) => Err("something else is at the socket path".into()),
        Err(_) => Ok(()), // nothing there
    }
}

/// Ask a running Pawline. Err when none is running.
#[cfg(unix)]
pub fn ask(word: &str) -> Result<String, String> {
    ask_at(&sock_path().ok_or("no XDG_RUNTIME_DIR")?, word)
}

#[cfg(unix)]
pub fn ask_at(p: &std::path::Path, word: &str) -> Result<String, String> {
    path_ok(p, my_uid().ok_or("no uid")?)?;
    if !p.exists() {
        return Err("not running".into());
    }
    let mut s = UnixStream::connect(p).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
    s.write_all(word.as_bytes()).map_err(|e| e.to_string())?;
    s.write_all(b"\n").ok();
    let mut out = String::new();
    s.take(64).read_to_string(&mut out).map_err(|e| e.to_string())?;
    Ok(out.trim().to_string())
}

/// True when another Pawline already answers on the socket.
#[cfg(unix)]
pub fn running() -> bool {
    ask("status").is_ok()
}

#[cfg(unix)]
pub fn serve(tx: calloop::channel::Sender<Ctl>, state: Shared) -> bool {
    let Some(p) = sock_path() else { return false };
    serve_at(&p, tx, state)
}

#[cfg(unix)]
pub fn serve_at(p: &std::path::Path, tx: calloop::channel::Sender<Ctl>, state: Shared) -> bool {
    let Some(uid) = my_uid() else { return false };
    if path_ok(p, uid).is_err() {
        return false; // foreign/odd file or shared folder: leave it alone
    }
    if p.exists() {
        if ask_at(p, "status").is_ok() {
            return false; // a live Pawline owns it
        }
        let _ = std::fs::remove_file(p); // our own stale socket (nobody answers)
    }
    let Ok(l) = UnixListener::bind(p) else { return false };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
    }
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            s.set_read_timeout(Some(std::time::Duration::from_secs(1))).ok();
            let mut buf = [0u8; 16];
            let n = s.read(&mut buf).unwrap_or(0);
            let word = String::from_utf8_lossy(&buf[..n]).trim().to_string();
            let reply: String = match word.as_str() {
                "status" => state.lock().map(|g| g.to_string()).unwrap_or_else(|_| "idle".into()),
                "toggle" => send(&tx, Ctl::Toggle),
                "show" => send(&tx, Ctl::Show),
                "hide" => send(&tx, Ctl::Hide),
                _ => "unknown".into(),
            };
            let _ = s.write_all(reply.as_bytes());
        }
    });
    true
}

#[cfg(unix)]
fn send(tx: &calloop::channel::Sender<Ctl>, c: Ctl) -> String {
    if tx.send(c).is_ok() { "ok".into() } else { "gone".into() }
}

/// The word the widgets show. `off` = Pawline is not running.
pub fn label(state: &str) -> &'static str {
    match state {
        "busy" => "working",
        "confirm" => "needs a yes/no",
        "open" => "open",
        "hidden" => "ready",
        "idle" => "ready",
        _ => "off",
    }
}

/// Waybar custom module, return-type json.
pub fn waybar(state: &str) -> String {
    let class = match state {
        "busy" | "confirm" | "open" | "hidden" | "idle" => state,
        _ => "off",
    };
    format!("{{\"text\":\"Pawline\",\"tooltip\":\"Pawline: {}\",\"class\":\"{}\"}}", label(state), class)
}

/// Polybar custom/script: plain one-line text.
pub fn polybar(state: &str) -> String {
    match state {
        "busy" | "confirm" => format!("Pawline ({})", label(state)),
        "off" | "" => "Pawline (off)".into(),
        _ => "Pawline".into(),
    }
}

/// xfce4-panel Generic Monitor: txt, tooltip and a click command.
pub fn genmon(state: &str) -> String {
    format!(
        "<txt>{}</txt><txtclick>pawline --ctl toggle</txtclick><tool>Pawline: {}</tool>",
        polybar(state),
        label(state)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use calloop::channel;
    use std::os::unix::fs::PermissionsExt;

    fn private_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-ctl-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }
    fn shared() -> Shared {
        Arc::new(Mutex::new("idle"))
    }

    #[test]
    fn live_server_answers_fixed_words_only() {
        let d = private_dir("live");
        let p = d.join("pawline.sock");
        let (tx, rx) = channel::channel::<Ctl>();
        assert!(serve_at(&p, tx, shared()));
        assert_eq!(ask_at(&p, "status").unwrap(), "idle");
        assert_eq!(ask_at(&p, "toggle").unwrap(), "ok");
        assert_eq!(ask_at(&p, "rm -rf /").unwrap(), "unknown");
        drop(rx);
        // a second server must not take over a live one
        let (tx2, _rx2) = channel::channel::<Ctl>();
        assert!(!serve_at(&p, tx2, shared()));
        assert_eq!(ask_at(&p, "status").unwrap(), "idle");
        assert_eq!(std::fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn stale_own_socket_is_replaced() {
        let d = private_dir("stale");
        let p = d.join("pawline.sock");
        drop(UnixListener::bind(&p).unwrap()); // file stays, nobody listens
        assert!(p.exists());
        let (tx, _rx) = channel::channel::<Ctl>();
        assert!(serve_at(&p, tx, shared()));
        assert_eq!(ask_at(&p, "status").unwrap(), "idle");
    }

    #[test]
    fn odd_files_and_shared_folders_are_left_alone() {
        let d = private_dir("odd");
        let p = d.join("pawline.sock");
        std::fs::write(&p, b"precious").unwrap();
        let (tx, _rx) = channel::channel::<Ctl>();
        assert!(!serve_at(&p, tx, shared()));
        assert_eq!(std::fs::read(&p).unwrap(), b"precious", "regular file must not be deleted");
        assert!(ask_at(&p, "status").is_err());
        // symlink at the path is not followed or removed
        let q = d.join("link.sock");
        std::os::unix::fs::symlink(&p, &q).unwrap();
        assert!(path_ok(&q, my_uid().unwrap()).is_err());
        assert!(q.symlink_metadata().is_ok());
        // folder others can write to is refused
        let w = private_dir("shared");
        std::fs::set_permissions(&w, std::fs::Permissions::from_mode(0o777)).unwrap();
        let (tx, _rx) = channel::channel::<Ctl>();
        assert!(!serve_at(&w.join("pawline.sock"), tx, shared()));
        assert!(path_ok(&w.join("x"), my_uid().unwrap()).is_err());
        // a different uid is refused (the socket/folder is ours, not theirs)
        assert!(path_ok(&d.join("nothing"), my_uid().unwrap() + 1).is_err());
    }

    #[test]
    fn formats_are_fixed_and_safe() {
        assert_eq!(waybar("busy"), r#"{"text":"Pawline","tooltip":"Pawline: working","class":"busy"}"#);
        assert!(waybar("weird\"state").contains("\"class\":\"off\""));
        assert_eq!(polybar("idle"), "Pawline");
        assert_eq!(polybar("confirm"), "Pawline (needs a yes/no)");
        assert_eq!(polybar("off"), "Pawline (off)");
        assert!(genmon("idle").contains("<txtclick>pawline --ctl toggle</txtclick>"));
    }
}
