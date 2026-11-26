//! One-time in-app setup for the experimental Laya suggestions: download the
//! int8 model and tokenizer from huggingface.co, verify them, convert the model
//! to fp32 with [`crate::laya_convert`], and install the three files atomically.
//!
//! Guarantees: downloads are size- and sha256-pinned so a changed or corrupted
//! file never installs; every partial state lives in dot-temp files that are
//! removed on error or cancel; the final files appear only via rename; an
//! existing complete install is never touched (`AlreadyInstalled`). After a
//! successful install the 289 MB int8 source is deleted - the kept footprint is
//! the fp32 model files (~640 MB) and the tokenizer.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

use sha2::{Digest, Sha256};

pub const MODEL_URL: &str =
    "https://huggingface.co/vrajnotviraj/laya-intent-router-150m-onnx/resolve/main/laya.onnx";
pub const TOKENIZER_URL: &str =
    "https://huggingface.co/vrajnotviraj/laya-intent-router-150m-onnx/resolve/main/tokenizer.json";
pub const MODEL_BYTES: u64 = 302_276_803;
pub const MODEL_SHA256: &str = "f4e29e431074a059977293de52408c4e18ce7f6e38d6b7b77e7e3e9406d67f34";
pub const TOKENIZER_BYTES: u64 = 2_132_967;
pub const TOKENIZER_SHA256: &str = "9fd55248d51d33976b324fc11592e28071da7d41e0e9401dfb7082e30574b7b1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    AlreadyInstalled,
    Installed,
    Canceled,
}

/// Where the files come from and what they must hash to. Tests point this at a
/// local server; the app uses [`hf_sources`].
#[derive(Debug, Clone)]
pub struct Sources {
    pub model_url: String,
    pub tokenizer_url: String,
    pub model_bytes: u64,
    pub model_sha256: String,
    pub tokenizer_bytes: u64,
    pub tokenizer_sha256: String,
}

pub fn hf_sources() -> Sources {
    Sources {
        model_url: MODEL_URL.into(),
        tokenizer_url: TOKENIZER_URL.into(),
        model_bytes: MODEL_BYTES,
        model_sha256: MODEL_SHA256.into(),
        tokenizer_bytes: TOKENIZER_BYTES,
        tokenizer_sha256: TOKENIZER_SHA256.into(),
    }
}

pub fn files_present(dir: &Path) -> bool {
    dir.join("laya_fp32.onnx").is_file()
        && dir.join("laya_fp32.onnx.data").is_file()
        && dir.join("tokenizer.json").is_file()
}

/// Install the model into `dir`. `progress` gets short user-facing status lines;
/// `cancel` stops the work between chunks and cleans up. Never deletes or
/// overwrites a complete existing install.
pub fn install(
    dir: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(&str),
) -> Result<Outcome, String> {
    install_from(dir, &hf_sources(), cancel, progress)
}

pub fn install_from(
    dir: &Path,
    src: &Sources,
    cancel: &AtomicBool,
    progress: &dyn Fn(&str),
) -> Result<Outcome, String> {
    if files_present(dir) {
        return Ok(Outcome::AlreadyInstalled);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let int8 = dir.join(".laya-int8.onnx.tmp");
    let tok = dir.join(".tokenizer.json.tmp");
    let model = dir.join(".laya_fp32.onnx.tmp");
    let data = dir.join(".laya_fp32.onnx.data.tmp");
    let temps = [&int8, &tok, &model, &data];
    let result = run(dir, src, cancel, progress, &int8, &tok, &model, &data);
    if !matches!(result, Ok(Outcome::Installed)) {
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn run(
    dir: &Path,
    src: &Sources,
    cancel: &AtomicBool,
    progress: &dyn Fn(&str),
    int8: &Path,
    tok: &Path,
    model: &Path,
    data: &Path,
) -> Result<Outcome, String> {
    if download(&src.model_url, src.model_bytes, &src.model_sha256, int8, "the Laya model (289 MB)", cancel, progress)? {
        return Ok(Outcome::Canceled);
    }
    if download(&src.tokenizer_url, src.tokenizer_bytes, &src.tokenizer_sha256, tok, "the tokenizer (2 MB)", cancel, progress)? {
        return Ok(Outcome::Canceled);
    }
    progress("Converting the model for Pawline (a one-time step)...");
    let input = std::fs::read(int8).map_err(|e| format!("read downloaded model: {e}"))?;
    let mut data_file = std::io::BufWriter::new(
        std::fs::File::create(data).map_err(|e| format!("create {}: {e}", data.display()))?,
    );
    let (model_bytes, stats) = match crate::laya_convert::convert_onnx_bytes(
        &input,
        &mut data_file,
        &mut |done| {
            if cancel.load(SeqCst) {
                return Err("cancelled".into());
            }
            progress(&format!("Converting the model: layer {done}..."));
            Ok(())
        },
    ) {
        Ok(v) => v,
        Err(e) if e == "cancelled" => return Ok(Outcome::Canceled),
        Err(e) => return Err(e),
    };
    drop(input);
    if cancel.load(SeqCst) {
        return Ok(Outcome::Canceled);
    }
    std::io::Write::flush(&mut data_file).map_err(|e| format!("write model data: {e}"))?;
    std::fs::write(model, &model_bytes).map_err(|e| format!("write {}: {e}", model.display()))?;

    // Atomic install: each file appears complete or not at all; a crash between
    // renames leaves an incomplete set, which files_present() treats as absent.
    let mv = |tmp: &Path, name: &str| -> Result<(), String> {
        std::fs::rename(tmp, dir.join(name)).map_err(|e| format!("install {name}: {e}"))
    };
    mv(data, "laya_fp32.onnx.data")?;
    mv(model, "laya_fp32.onnx")?;
    mv(tok, "tokenizer.json")?;
    // The int8 source is re-downloadable; keeping it would double the footprint.
    let _ = std::fs::remove_file(int8);
    progress(&format!(
        "Done: {} layers converted, {} MB installed.",
        stats.nodes_converted,
        stats.external_bytes / 1_000_000
    ));
    Ok(Outcome::Installed)
}

/// Download one pinned file. Returns Ok(true) when cancelled (partial file is
/// removed by the caller's cleanup).
fn download(
    url: &str,
    want_bytes: u64,
    want_sha256: &str,
    dest: &Path,
    label: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(&str),
) -> Result<bool, String> {
    // gate-checked sender: offline mode stops the request before it is made
    // (the card is also refused in offline mode, so this is the backstop).
    crate::netgate::check().map_err(|e| format!("download {label}: {e}"))?;
    let mut resp = ureq::get(url)
        .call()
        .map_err(|e| format!("download {label}: {e}"))?;
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?,
    );
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let mut last_report_mb = u64::MAX;
    loop {
        if cancel.load(SeqCst) {
            return Ok(true);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download {label}: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", dest.display()))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        let mb = done / (16 * 1024 * 1024);
        if mb != last_report_mb {
            last_report_mb = mb;
            progress(&format!(
                "Downloading {label}: {} / {} MB",
                done / (1024 * 1024),
                want_bytes / (1024 * 1024)
            ));
        }
    }
    std::io::Write::flush(&mut file).map_err(|e| format!("write {}: {e}", dest.display()))?;
    if done != want_bytes {
        return Err(format!(
            "download {label}: got {done} bytes, expected {want_bytes} - the file changed, refusing to install"
        ));
    }
    let got = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in got {
        hex.push_str(&format!("{b:02x}"));
    }
    if hex != want_sha256 {
        return Err(format!(
            "download {label}: checksum mismatch - the file changed, refusing to install"
        ));
    }
    Ok(false)
}
