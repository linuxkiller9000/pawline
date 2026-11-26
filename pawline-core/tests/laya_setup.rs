use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

use pawline_core::laya_convert::*;
use pawline_core::laya_setup::*;

fn vf(num: u32, v: u64) -> Field {
    Field { num, wire: 0, val: Val::Varint(v) }
}
fn sf(num: u32, s: &str) -> Field {
    Field { num, wire: 2, val: Val::Len(s.as_bytes().to_vec()) }
}
fn lf(num: u32, b: Vec<u8>) -> Field {
    Field { num, wire: 2, val: Val::Len(b) }
}
fn tensor_raw(name: &str, dtype: u64, dims: &[u64], raw: Vec<u8>) -> Vec<u8> {
    let mut fields: Vec<Field> = dims.iter().map(|d| vf(1, *d)).collect();
    fields.push(vf(2, dtype));
    fields.push(sf(8, name));
    fields.push(lf(9, raw));
    let mut out = Vec::new();
    write_fields(&fields, &mut out);
    out
}

/// Minimal one-MatMulNBits model, same shape as the convert tests.
fn tiny_model() -> (Vec<u8>, [f32; 4]) {
    let b: Vec<u8> = (0..128u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    let s: [f32; 4] = [0.5, -1.25, 2.0, 0.03125];
    let s_raw: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut a1 = Vec::new();
    write_fields(&[sf(1, "bits"), vf(3, 8), vf(20, 2)], &mut a1);
    let mut a2 = Vec::new();
    write_fields(&[sf(1, "block_size"), vf(3, 32), vf(20, 2)], &mut a2);
    let mut node1 = Vec::new();
    write_fields(
        &[sf(1, "x"), sf(1, "B"), sf(1, "S"), sf(2, "y"), sf(4, "MatMulNBits"), lf(5, a1), lf(5, a2)],
        &mut node1,
    );
    let mut graph = Vec::new();
    write_fields(
        &[lf(1, node1), lf(5, tensor_raw("B", 2, &[2, 64], b)), lf(5, tensor_raw("S", 1, &[2, 2], s_raw))],
        &mut graph,
    );
    let mut model = Vec::new();
    write_fields(&[vf(1, 9), lf(7, graph)], &mut model);
    (model, s)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Serve `bodies` on a one-request-per-body localhost server. Returns the base URL.
fn serve(bodies: Vec<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for body in bodies {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = conn.read(&mut buf); // the request; contents ignored
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            conn.write_all(head.as_bytes()).unwrap();
            conn.write_all(&body).unwrap();
            conn.flush().unwrap();
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "laya-setup-test-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sources(base: &str, model: &[u8], tok: &[u8]) -> Sources {
    Sources {
        model_url: format!("{base}/laya.onnx"),
        tokenizer_url: format!("{base}/tokenizer.json"),
        model_bytes: model.len() as u64,
        model_sha256: sha256_hex(model),
        tokenizer_bytes: tok.len() as u64,
        tokenizer_sha256: sha256_hex(tok),
    }
}

#[test]
fn installs_converts_and_cleans_up() {
    let (model, s) = tiny_model();
    let tok = b"{\"tokenizer\":true}".to_vec();
    let base = serve(vec![model.clone(), tok.clone()]);
    let dir = temp_dir("ok");
    let cancel = AtomicBool::new(false);
    let lines = std::sync::Mutex::new(Vec::<String>::new());
    let out = install_from(&dir, &sources(&base, &model, &tok), &cancel, &|l| lines.lock().unwrap().push(l.to_string()))
        .expect("install");
    assert_eq!(out, Outcome::Installed);
    assert!(files_present(&dir));
    assert_eq!(std::fs::read(dir.join("tokenizer.json")).unwrap(), tok);
    let data = std::fs::read(dir.join("laya_fp32.onnx.data")).unwrap();
    assert_eq!(data.len(), 64 * 2 * 4);
    let b: Vec<u8> = (0..128u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    for k in 0..64usize {
        for n in 0..2usize {
            let got = f32::from_le_bytes(data[(k * 2 + n) * 4..][..4].try_into().unwrap());
            let want = (b[n * 64 + k] as f32 - 128.0) * s[n * 2 + k / 32];
            assert_eq!(got.to_bits(), want.to_bits());
        }
    }
    // no temp files or leftover int8 source remain
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("tmp") || n.contains("int8"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(lines.lock().unwrap().iter().any(|l| l.contains("Downloading the Laya model")));
    assert!(lines.lock().unwrap().iter().any(|l| l.contains("Converting the model: layer 1")));
    // second run installs nothing
    let out2 = install_from(&dir, &sources(&base, &model, &tok), &cancel, &|_| {}).expect("again");
    assert_eq!(out2, Outcome::AlreadyInstalled);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn refuses_a_changed_download_and_leaves_no_trace() {
    let (mut model, _) = tiny_model();
    let tok = b"{}".to_vec();
    let mut good = model.clone();
    let src_bad = {
        let base = serve(vec![model.clone()]);
        let mut s = sources(&base, &model, &tok);
        // server sends `model` but the pin expects a different hash
        s.model_sha256 = sha256_hex(b"definitely not it");
        s
    };
    let dir = temp_dir("bad");
    let cancel = AtomicBool::new(false);
    let err = install_from(&dir, &src_bad, &cancel, &|_| {}).unwrap_err();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert!(!files_present(&dir));
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    good.push(0); // keep `good` used; proves pins bind exact bytes
    let mut src_size = sources(&serve(vec![good.clone()]), &model, &tok);
    src_size.model_bytes = model.len() as u64; // pin says old size, server sends +1 byte
    let dir2 = temp_dir("size");
    let err2 = install_from(&dir2, &src_size, &cancel, &|_| {}).unwrap_err();
    assert!(err2.contains("bytes"), "{err2}");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

#[test]
fn cancel_before_download_installs_nothing() {
    let (model, _) = tiny_model();
    let tok = b"{}".to_vec();
    let base = serve(vec![model.clone(), tok.clone()]);
    let dir = temp_dir("cancel");
    let cancel = AtomicBool::new(true); // already cancelled: stops at the first chunk
    let out = install_from(&dir, &sources(&base, &model, &tok), &cancel, &|_| {}).expect("run");
    assert_eq!(out, Outcome::Canceled);
    assert!(!files_present(&dir));
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    std::fs::remove_dir_all(&dir).ok();
}
