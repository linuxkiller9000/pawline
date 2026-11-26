//! Structural privacy audit (item 14): Pawline's own network requests may only ever come from the two
//! gate-checked senders. This test reads the crate sources and fails when:
//!   - an HTTP client primitive (`ureq::`) appears outside imagegen.rs / search.rs,
//!   - a raw socket or another client crate (TcpStream, UdpSocket, reqwest, isahc, curl) appears anywhere in src/,
//!   - either known sender stops calling netgate::check before sending.
//! It cannot prove runtime behaviour (that is the netgate behavioural tests' job) and it cannot see
//! subprocesses the user asks to open; it guards the code shape so a new ungated sender cannot slip in.
use std::path::{Path, PathBuf};

fn srcs() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for dir in [root.join("src"), root.join("../pawline-native/src"), root.join("../pawline-needle/src")] {
        collect(&dir, &mut out);
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|_| panic!("read {}", dir.display())) {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap_or_default()));
        }
    }
}

fn file_name(f: &str) -> &str {
    f.rsplit('/').next().unwrap_or(f)
}

#[test]
fn http_client_primitives_live_only_in_the_two_gated_senders() {
    for (f, src) in srcs() {
        let known_sender = file_name(&f) == "imagegen.rs" || file_name(&f) == "search.rs";
        let n = src.matches("ureq::").count();
        if known_sender {
            assert!(n > 0, "{f} lost its ureq client - did the sender change without updating this audit?");
        } else {
            assert_eq!(n, 0, "{f} mentions ureq:: - new network code must live in a gate-checked sender");
        }
    }
}

#[test]
fn no_raw_sockets_or_other_http_crates_anywhere() {
    for (f, src) in srcs() {
        for bad in ["TcpStream", "UdpSocket", "reqwest", "isahc", "curl::", "hyper::"] {
            assert!(!src.contains(bad), "{f} mentions {bad} - Pawline's own requests go through the two gate-checked senders only");
        }
    }
}

#[test]
fn both_known_senders_call_the_gate_before_sending() {
    for (f, src) in srcs() {
        if file_name(&f) == "imagegen.rs" || file_name(&f) == "search.rs" {
            assert!(src.contains("netgate::check"), "{f} sends without netgate::check - offline mode would be bypassed");
        }
    }
    // and the gate itself still exists with its refusal message
    let gate = srcs().into_iter().find(|(f, _)| file_name(&f) == "netgate.rs").expect("netgate.rs exists");
    assert!(gate.1.contains("Offline mode is on"), "netgate lost its refusal");
}

#[test]
fn no_extra_http_client_dependencies_in_manifests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for manifest in [root.join("Cargo.toml"), root.join("../pawline-native/Cargo.toml"), root.join("../pawline-needle/Cargo.toml"), root.join("../Cargo.toml")] {
        let Ok(t) = std::fs::read_to_string(&manifest) else { continue };
        for bad in ["reqwest", "isahc", "hyper", "curl", "attohttpc", "minreq"] {
            assert!(!t.contains(bad), "{} adds HTTP client {bad} - the audit allows ureq only", manifest.display());
        }
    }
}
