//! Starting the local llama.cpp runner for the user.
//!
//! Pawline never downloads a model and never ships one. When the user has pointed
//! at a vision file (Settings, or "use vision model <path>") and the chat provider
//! is the local llama.cpp one, Pawline can start `llama-server` itself with that
//! file, so nothing has to be launched by hand in another terminal.
//!
//! Two rules the code below keeps:
//!  - if a server already answers, nothing is started (never a second copy)
//!  - a server Pawline started cannot outlive Pawline: the child gets a
//!    parent-death signal from the kernel, so a crash or a kill leaves no
//!    orphaned process holding half a gigabyte of RAM
//!
//! No network beyond the loopback address the user's own runner listens on.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What happened when Pawline asked for a runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Something already answers on that address; nothing was started.
    AlreadyRunning,
    /// Pawline started it and it answers now.
    Started { pid: u32, log: PathBuf },
}

/// The port in a base URL like `http://127.0.0.1:8080` (8080 when absent or unparsable).
pub fn port_of(base_url: &str) -> u16 {
    let after_scheme = base_url.split("://").nth(1).unwrap_or(base_url);
    let host_port = after_scheme.split('/').next().unwrap_or("");
    match host_port.rsplit_once(':') {
        Some((_, p)) => p.parse().unwrap_or(8080),
        None => 8080,
    }
}

/// The address Pawline asks "are you up?" on. llama.cpp answers `/props` with JSON.
pub fn probe_url(base_url: &str) -> String {
    format!("{}/props", base_url.trim_end_matches('/'))
}

/// The llama-server arguments for these files. Kept separate from spawning so the
/// exact command line can be tested without a model.
pub fn server_args(model: &Path, mmproj: Option<&Path>, port: u16) -> Vec<String> {
    let mut a = vec![
        "-m".to_string(),
        model.to_string_lossy().to_string(),
        "-c".to_string(),
        "4096".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
    ];
    if let Some(p) = mmproj {
        a.push("--mmproj".to_string());
        a.push(p.to_string_lossy().to_string());
    }
    a
}

/// Find `llama-server`: on PATH first, then next to the model file (a folder people
/// unpack a release into), then in the usual release folder under the data dir.
pub fn find_binary(has: &dyn Fn(&str) -> bool, model: &Path, data_dir: Option<&Path>) -> Option<PathBuf> {
    if has("llama-server") {
        return Some(PathBuf::from("llama-server"));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = model.parent() {
        candidates.push(dir.join("llama-server"));
        candidates.push(dir.join("bin/llama-server"));
    }
    if let Some(d) = data_dir {
        candidates.push(d.join("llama-server"));
        candidates.push(d.join("llama/bin/llama-server"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Ask for a runner, starting one if nothing answers. `probe` is given the URL and
/// answers whether something healthy is there; `wait` bounds how long a fresh one
/// may take to come up (loading a vision model reads a few hundred MB from disk).
pub fn ensure_running(
    base_url: &str,
    model: &Path,
    mmproj: Option<&Path>,
    binary: &Path,
    probe: &dyn Fn(&str) -> bool,
    wait: Duration,
) -> Result<Outcome, String> {
    let url = probe_url(base_url);
    if probe(&url) {
        return Ok(Outcome::AlreadyRunning);
    }
    if !model.is_file() {
        return Err(format!(
            "The vision model file {} is not there any more, so there is nothing to start. Point Pawline at it again in Settings. Nothing was captured.",
            model.display()
        ));
    }
    let port = port_of(base_url);
    let log = std::env::temp_dir().join(format!("pawline-runner-{}.log", std::process::id()));
    let logf = std::fs::File::create(&log).map_err(|e| format!("Could not open {} for the runner's output: {e}. Nothing was captured.", log.display()))?;
    let logf2 = logf.try_clone().map_err(|e| format!("Could not prepare the runner's output file: {e}. Nothing was captured."))?;
    let mut cmd = Command::new(binary);
    cmd.args(server_args(model, mmproj, port)).stdin(Stdio::null()).stdout(logf).stderr(logf2);
    // The child dies with Pawline: no orphaned runner holding the model in RAM.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // SIGTERM when the parent goes away. Failure here is not fatal; the
                // process still runs, it just loses the safety net.
                let _ = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let child = cmd.spawn().map_err(|e| {
        format!(
            "Could not start {} ({e}). If llama-server is not installed, install it (for example the llama.cpp package) or start your own server. Nothing was captured.",
            binary.display()
        )
    })?;
    let pid = child.id();
    // Wait for it to answer. The child is not waited on: it keeps running for the
    // life of Pawline, and the kernel reaps it when Pawline exits.
    let deadline = Instant::now() + wait;
    loop {
        if probe(&url) {
            return Ok(Outcome::Started { pid, log });
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Started {} but it did not answer within {} seconds. Its output is in {}. Nothing was captured.",
                binary.display(),
                wait.as_secs(),
                log.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_and_probe_address_come_from_the_provider_url() {
        assert_eq!(port_of("http://127.0.0.1:8080"), 8080);
        assert_eq!(port_of("http://127.0.0.1:9090/"), 9090);
        assert_eq!(port_of("http://localhost"), 8080, "no port means llama.cpp's default");
        assert_eq!(port_of("garbage"), 8080);
        assert_eq!(probe_url("http://127.0.0.1:8080/"), "http://127.0.0.1:8080/props");
        assert_eq!(probe_url("http://127.0.0.1:8080"), "http://127.0.0.1:8080/props");
    }

    #[test]
    fn the_command_line_names_the_model_the_projector_and_the_port() {
        let a = server_args(Path::new("/m/SmolVLM-256M-Instruct-Q8_0.gguf"), Some(Path::new("/m/mmproj-SmolVLM.gguf")), 8080);
        assert_eq!(a[0], "-m");
        assert_eq!(a[1], "/m/SmolVLM-256M-Instruct-Q8_0.gguf");
        let joined = a.join(" ");
        assert!(joined.contains("--mmproj /m/mmproj-SmolVLM.gguf"), "{joined}");
        assert!(joined.contains("--port 8080"), "{joined}");
        assert!(joined.contains("-c 4096"), "{joined}");
        assert!(joined.contains("--host 127.0.0.1"), "the runner listens on loopback only: {joined}");
        // no projector: no --mmproj at all, never an empty argument
        let b = server_args(Path::new("/m/v.gguf"), None, 8081);
        assert!(!b.join(" ").contains("--mmproj"));
    }

    #[test]
    fn a_healthy_server_is_never_started_twice() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let probe = |_: &str| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            true
        };
        let out = ensure_running("http://127.0.0.1:8080", Path::new("/m/v.gguf"), None, Path::new("/nonexistent/llama-server"), &probe, Duration::from_millis(50));
        assert_eq!(out.unwrap(), Outcome::AlreadyRunning);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "one probe, no spawn attempt");
    }

    #[test]
    fn a_missing_model_or_binary_says_what_is_wrong_instead_of_starting_nothing() {
        let never = |_: &str| false;
        let out = ensure_running("http://127.0.0.1:8080", Path::new("/nowhere/v.gguf"), None, Path::new("/nowhere/llama-server"), &never, Duration::from_millis(50));
        let err = out.unwrap_err();
        assert!(err.contains("is not there any more") && err.contains("Nothing was captured"), "{err}");
        // model present, binary missing
        let d = std::env::temp_dir().join(format!("pawline-runner-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let m = d.join("v.gguf");
        std::fs::write(&m, b"x").unwrap();
        let out2 = ensure_running("http://127.0.0.1:8080", &m, None, &d.join("no-such-binary"), &never, Duration::from_millis(50));
        let err2 = out2.unwrap_err();
        assert!(err2.contains("Could not start") && err2.contains("Nothing was captured"), "{err2}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_binary_is_found_on_path_then_next_to_the_model() {
        let d = std::env::temp_dir().join(format!("pawline-runner-bin-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let model = d.join("v.gguf");
        std::fs::write(&model, b"x").unwrap();
        assert_eq!(find_binary(&|b| b == "llama-server", &model, None), Some(PathBuf::from("llama-server")), "PATH wins");
        assert_eq!(find_binary(&|_| false, &model, None), None, "nothing on disk, nothing found");
        let bin = d.join("llama-server");
        std::fs::write(&bin, b"x").unwrap();
        assert_eq!(find_binary(&|_| false, &model, None), Some(bin), "next to the model is found");
        let _ = std::fs::remove_dir_all(&d);
    }
}
