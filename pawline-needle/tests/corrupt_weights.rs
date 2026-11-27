//! Item 16: corrupted model data must be handled gracefully. Each corrupt case runs in a SPAWNED
//! COPY of this test binary, so if the C++ runtime crashed on bad bytes the parent sees the signal
//! instead of dying itself. Linux only (the runtime is not linked on Windows).
#![cfg(not(windows))]

fn run_case(case: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["corrupt_child", "--exact", "--nocapture"])
        .env("CORRUPT_CASE", case)
        .output()
        .unwrap()
}

#[test]
fn corrupt_child() {
    // Runs the real body only inside the parent's spawned copy; harmless pass otherwise.
    let Ok(case) = std::env::var("CORRUPT_CASE") else { return };
    let good = pawline_needle::NEEDLE3_CACT;
    let bytes: Vec<u8> = match case.as_str() {
        "empty" => vec![],
        "truncated" => good[..4096].to_vec(),
        "garbage" => (0..65536u32).map(|i| (i.wrapping_mul(2654435761) % 256) as u8).collect(),
        "flipped" => {
            let mut b = good.to_vec();
            for i in (0..b.len()).step_by(997) {
                b[i] ^= 0xFF;
            }
            b
        }
        other => panic!("unknown case {other}"),
    };
    match pawline_needle::Needle::probe_load_for_test(&bytes) {
        Ok(()) => {
            eprintln!("corrupt weights were ACCEPTED");
            std::process::exit(3);
        }
        Err(e) => {
            eprintln!("graceful refusal: {:?}", e);
            std::process::exit(0);
        }
    }
}

#[test]
fn corrupted_weights_are_refused_gracefully() {
    for case in ["empty", "truncated", "garbage", "flipped"] {
        let out = run_case(case);
        assert!(
            out.status.success(),
            "case {case}: exit {:?} (a signal means the runtime crashed on bad bytes); stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
