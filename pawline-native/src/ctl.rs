//! Tiny local control socket so a panel widget (Waybar, Polybar, xfce4 genmon, a KDE plasmoid) can show
//! Pawline's state and open or close it. It accepts four fixed words only: toggle, show, hide, status.
//! No text from the socket is ever treated as a request, a command or a path. The socket lives in
//! $XDG_RUNTIME_DIR (mode 0600, owner only).
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ctl {
    Toggle,
    Show,
    Hide,
}

pub type Shared = Arc<Mutex<&'static str>>;

pub fn sock_path() -> Option<PathBuf> {
    let d = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(d).join("pawline.sock"))
}

/// Ask a running Pawline. Err when none is running.
pub fn ask(word: &str) -> Result<String, String> {
    let p = sock_path().ok_or("no XDG_RUNTIME_DIR")?;
    let mut s = UnixStream::connect(p).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();
    s.write_all(word.as_bytes()).map_err(|e| e.to_string())?;
    s.write_all(b"\n").ok();
    let mut out = String::new();
    s.take(64).read_to_string(&mut out).map_err(|e| e.to_string())?;
    Ok(out.trim().to_string())
}

/// True when another Pawline already answers on the socket.
pub fn running() -> bool {
    ask("status").is_ok()
}

pub fn serve(tx: smithay_client_toolkit::reexports::calloop::channel::Sender<Ctl>, state: Shared) -> bool {
    let Some(p) = sock_path() else { return false };
    let _ = std::fs::remove_file(&p); // stale one (a live one was ruled out by running())
    let Ok(l) = UnixListener::bind(&p) else { return false };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
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

fn send(tx: &smithay_client_toolkit::reexports::calloop::channel::Sender<Ctl>, c: Ctl) -> String {
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
