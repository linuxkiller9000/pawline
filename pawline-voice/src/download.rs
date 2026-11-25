//! Consent-gated, verified model download.
//!
//! The caller asks the user first (name, total size, host) and only then calls
//! `download_model`. Every file has a pinned size and SHA-256. Files are
//! written to a hidden staging directory, checked, and the whole directory is
//! renamed into place at the very end, so a half-finished or tampered model is
//! never visible at the final path. Cancel (or any error) deletes the staging
//! directory. Production use is https only.
use crate::VoiceError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}
pub struct ModelPin {
    pub id: &'static str,
    pub base_url: &'static str,
    pub files: &'static [ModelFile],
}
impl ModelPin {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
    pub fn host(&self) -> &str {
        self.base_url.trim_start_matches("https://").split('/').next().unwrap_or("")
    }
}

macro_rules! f {
    ($n:expr, $s:expr, $h:expr) => {
        ModelFile { name: $n, size: $s, sha256: $h }
    };
}
/// Moonshine tiny streaming English, quantized_26_11_02 (45,398,523 bytes).
pub const EN_TINY: ModelPin = ModelPin {
    id: "moonshine-tiny-streaming-en",
    base_url: "https://download.moonshine.ai/model/tiny-streaming-en/quantized_26_11_02",
    files: &[
        f!("adapter.ort", 1319616, "c5f9f7cc98c27a965d8e200816f7e5c07b892712bb30cf07444bbde4ffd38199"),
        f!("cross_kv.ort", 1287504, "5f68900da4ea20f40fbe67bbec36399f0d47faedbd7198b29abd9dc5f472ef9f"),
        f!("decoder_kv.ort", 32748720, "6564f2c9a19de44195a53e336e07d05108f14babab57105d623bb68b79572d75"),
        f!("encoder.ort", 7675392, "617e403b2aae48f2474e0b148bf835deb9510bc8692efeabfe27203959a3256e"),
        f!("frontend.model.ort", 23392, "5662679f0e5758646d8eade3bc4b5d9ebda875feb6e696c0cc5f1a08fea2219e"),
        f!("frontend.weights.ort", 2093416, "55c91d7efab9f7506d7d753131cba72145ff016be3447319fe414a31e13f25b4"),
        f!("streaming_config.json", 509, "74fe5ddebd63b17caf59e8a3b18c17547ff7bce1642050edbb1c3962674f8950"),
        f!("tokenizer.bin", 249974, "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d"),
    ],
};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// True only if `dir` holds every pinned file with the exact size and hash.
pub fn verify_dir(pin: &ModelPin, dir: &Path) -> bool {
    pin.files.iter().all(|f| {
        let p = dir.join(f.name);
        match fs::metadata(&p) {
            Ok(m) if m.len() == f.size => {}
            _ => return false,
        }
        let mut h = Sha256::new();
        let Ok(mut r) = fs::File::open(&p) else { return false };
        let mut buf = [0u8; 65536];
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => h.update(&buf[..n]),
                Err(_) => return false,
            }
        }
        hex(&h.finalize()) == f.sha256
    })
}

/// Download `pin` into `dest` (created atomically). `progress(done, total)` is
/// called as bytes arrive. Setting `cancel` stops within one read and removes
/// the staging directory. https only.
pub fn download_model(pin: &ModelPin, dest: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Result<(), VoiceError> {
    if !pin.base_url.starts_with("https://") {
        return Err(VoiceError::Model("model URL is not https".into()));
    }
    download_inner(pin, pin.base_url, dest, cancel, progress, true)
}

fn download_inner(pin: &ModelPin, base: &str, dest: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64), https_only: bool) -> Result<(), VoiceError> {
    let err = |m: String| VoiceError::Model(m);
    let parent = dest.parent().ok_or_else(|| err("bad destination".into()))?;
    fs::create_dir_all(parent).map_err(|e| err(format!("cannot create {}: {e}", parent.display())))?;
    if verify_dir(pin, dest) {
        return Ok(());
    }
    let stage = parent.join(format!(".{}.partial", dest.file_name().and_then(|s| s.to_str()).unwrap_or("model")));
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir(&stage).map_err(|e| err(format!("cannot create staging dir: {e}")))?;
    let res = fetch_all(pin, base, &stage, cancel, progress, https_only);
    if let Err(e) = res {
        let _ = fs::remove_dir_all(&stage);
        return Err(e);
    }
    if !verify_dir(pin, &stage) {
        let _ = fs::remove_dir_all(&stage);
        return Err(err("verification failed after download".into()));
    }
    // Replace any older/damaged copy, then move the verified dir into place.
    let _ = fs::remove_dir_all(dest);
    fs::rename(&stage, dest).map_err(|e| {
        let _ = fs::remove_dir_all(&stage);
        err(format!("cannot install model: {e}"))
    })
}

fn fetch_all(pin: &ModelPin, base: &str, stage: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64, u64), https_only: bool) -> Result<(), VoiceError> {
    let err = |m: String| VoiceError::Model(m);
    let total = pin.total_bytes();
    let mut done = 0u64;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(https_only)
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(1800)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    progress(0, total);
    for file in pin.files {
        if cancel.load(Ordering::SeqCst) {
            return Err(err("download cancelled".into()));
        }
        let url = format!("{}/{}", base.trim_end_matches('/'), file.name);
        let mut resp = agent.get(&url).call().map_err(|e| err(format!("{}: {e}", file.name)))?;
        if let Some(len) = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()) {
            if len != file.size {
                return Err(err(format!("{}: unexpected size {len}", file.name)));
            }
        }
        let mut body = resp.body_mut().as_reader();
        let mut out = fs::File::create(stage.join(file.name)).map_err(|e| err(format!("write {}: {e}", file.name)))?;
        let mut h = Sha256::new();
        let mut got = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(err("download cancelled".into()));
            }
            let n = body.read(&mut buf).map_err(|e| err(format!("{}: {e}", file.name)))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > file.size {
                return Err(err(format!("{}: more data than expected", file.name)));
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| err(format!("write {}: {e}", file.name)))?;
            done += n as u64;
            progress(done, total);
        }
        out.sync_all().map_err(|e| err(format!("write {}: {e}", file.name)))?;
        if got != file.size || hex(&h.finalize()) != file.sha256 {
            return Err(err(format!("{}: size or hash mismatch", file.name)));
        }
    }
    Ok(())
}

/// What the app does once `download_model` returned. Cancel always wins: a
/// download that finished after the user cancelled is discarded (unless a
/// verified copy existed before, which is never deleted) and voice stays off.
#[derive(Debug, PartialEq)]
pub enum Settled {
    Failed(String),
    CancelledLate,
    Proceed,
}
pub fn settle(result: Result<(), VoiceError>, cancelled: bool, had_model: bool, dest: &Path) -> Settled {
    match result {
        Err(e) => Settled::Failed(e.to_string()),
        Ok(()) if cancelled => {
            if !had_model {
                let _ = fs::remove_dir_all(dest);
            }
            Settled::CancelledLate
        }
        Ok(()) => Settled::Proceed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    fn sha(b: &[u8]) -> String {
        hex(&Sha256::digest(b))
    }
    fn leak<T>(v: T) -> &'static T {
        Box::leak(Box::new(v))
    }
    /// Tiny local HTTP server. `corrupt` flips the second file; `slow` sends in pieces.
    fn serve(files: Vec<(String, Vec<u8>)>, slow: bool) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let files = files.clone();
                std::thread::spawn(move || {
                    let mut req = [0u8; 2048];
                    let n = s.read(&mut req).unwrap_or(0);
                    let line = String::from_utf8_lossy(&req[..n]).lines().next().unwrap_or("").to_string();
                    let path = line.split(' ').nth(1).unwrap_or("").trim_start_matches('/').to_string();
                    match files.iter().find(|(n, _)| *n == path) {
                        Some((_, b)) => {
                            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
                            for c in b.chunks(if slow { 1000 } else { 65536 }) {
                                if s.write_all(c).is_err() {
                                    break;
                                }
                                if slow {
                                    std::thread::sleep(std::time::Duration::from_millis(20));
                                }
                            }
                        }
                        None => {
                            let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                });
            }
        });
        format!("http://127.0.0.1:{port}")
    }
    fn mk(data: &[(&'static str, Vec<u8>)]) -> &'static ModelPin {
        let files: Vec<ModelFile> = data
            .iter()
            .map(|(n, b)| ModelFile { name: n, size: b.len() as u64, sha256: Box::leak(sha(b).into_boxed_str()) })
            .collect();
        leak(ModelPin { id: "t", base_url: "http://x", files: Box::leak(files.into_boxed_slice()) })
    }
    fn tmp(n: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pwdl-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn data() -> Vec<(&'static str, Vec<u8>)> {
        vec![("a.bin", vec![1u8; 300_000]), ("b.bin", (0..200_000u32).map(|i| (i % 251) as u8).collect())]
    }
    fn served(d: &[(&'static str, Vec<u8>)], slow: bool) -> String {
        serve(d.iter().map(|(n, b)| (n.to_string(), b.clone())).collect(), slow)
    }

    #[test]
    fn good_download_installs_atomically_and_reuses() {
        let d = data();
        let pin = mk(&d);
        let base = served(&d, false);
        let t = tmp("good");
        let dest = t.join("models/en");
        let last = AtomicU64::new(0);
        download_inner(pin, &base, &dest, &AtomicBool::new(false), &|a, b| { assert!(a <= b); last.store(a, Ordering::SeqCst) }, false).unwrap();
        assert_eq!(last.load(Ordering::SeqCst), pin.total_bytes());
        assert!(verify_dir(pin, &dest));
        assert!(!t.join("models/.en.partial").exists());
        // second call with a dead server: already verified, no network
        download_inner(pin, "http://127.0.0.1:1", &dest, &AtomicBool::new(false), &|_, _| {}, false).unwrap();
    }
    #[test]
    fn tampered_file_is_rejected_and_nothing_remains() {
        let d = data();
        let pin = mk(&d);
        let mut bad = d.clone();
        bad[1].1[777] ^= 0xff;
        let base = served(&bad, false);
        let t = tmp("bad");
        let dest = t.join("models/en");
        assert!(download_inner(pin, &base, &dest, &AtomicBool::new(false), &|_, _| {}, false).is_err());
        assert!(!dest.exists());
        assert!(!t.join("models/.en.partial").exists());
    }
    #[test]
    fn wrong_size_and_missing_file_are_rejected() {
        let d = data();
        let pin = mk(&d);
        let mut short = d.clone();
        short[0].1.truncate(1000);
        let t = tmp("short");
        let dest = t.join("m");
        assert!(download_inner(pin, &served(&short, false), &dest, &AtomicBool::new(false), &|_, _| {}, false).is_err());
        assert!(!dest.exists());
        let only_a = vec![d[0].clone()];
        assert!(download_inner(pin, &served(&only_a, false), &dest, &AtomicBool::new(false), &|_, _| {}, false).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn cancel_mid_download_leaves_no_model_and_no_partial() {
        let d = data();
        let pin = mk(&d);
        let base = served(&d, true);
        let t = tmp("cancel");
        let dest = t.join("models/en");
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let r = download_inner(pin, &base, &dest, &cancel, &|a, _| { if a > 50_000 { c2.store(true, Ordering::SeqCst) } }, false);
        assert!(r.is_err());
        assert!(!dest.exists());
        assert!(!t.join("models/.en.partial").exists());
    }
    #[test]
    fn damaged_existing_copy_is_replaced_only_by_a_verified_one() {
        let d = data();
        let pin = mk(&d);
        let t = tmp("dmg");
        let dest = t.join("models/en");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("a.bin"), b"junk").unwrap();
        assert!(!verify_dir(pin, &dest));
        download_inner(pin, &served(&d, false), &dest, &AtomicBool::new(false), &|_, _| {}, false).unwrap();
        assert!(verify_dir(pin, &dest));
    }
    #[test]
    fn production_entry_refuses_plain_http() {
        let d = data();
        let pin = mk(&d); // base_url is http://x
        let t = tmp("http");
        assert!(download_model(pin, &t.join("m"), &AtomicBool::new(false), &|_, _| {}).is_err());
    }
    #[test]
    fn https_only_agent_refuses_http_redirect_target() {
        let d = data();
        let pin = mk(&d);
        let t = tmp("h2");
        let r = download_inner(pin, &served(&d, false), &t.join("m"), &AtomicBool::new(false), &|_, _| {}, true);
        assert!(r.is_err());
        assert!(!t.join("m").exists());
    }
    #[test]
    fn real_en_pin_totals_match_published_size() {
        assert_eq!(EN_TINY.total_bytes(), 45_398_523);
        assert_eq!(EN_TINY.host(), "download.moonshine.ai");
    }
    #[test]
    fn late_cancel_discards_new_model_but_never_a_preexisting_one() {
        let d = data();
        let pin = mk(&d);
        let t = tmp("late");
        let dest = t.join("models/en");
        download_inner(pin, &served(&d, false), &dest, &AtomicBool::new(false), &|_, _| {}, false).unwrap();
        // finished OK, cancel arrived afterwards, no model before: removed
        assert_eq!(settle(Ok(()), true, false, &dest), Settled::CancelledLate);
        assert!(!dest.exists());
        // a verified model existed before: kept, voice still not enabled
        download_inner(pin, &served(&d, false), &dest, &AtomicBool::new(false), &|_, _| {}, false).unwrap();
        assert_eq!(settle(Ok(()), true, true, &dest), Settled::CancelledLate);
        assert!(verify_dir(pin, &dest));
        // no cancel: proceed; an error is reported as failed
        assert_eq!(settle(Ok(()), false, false, &dest), Settled::Proceed);
        assert!(matches!(settle(Err(VoiceError::Model("x".into())), false, false, &dest), Settled::Failed(_)));
    }
}
