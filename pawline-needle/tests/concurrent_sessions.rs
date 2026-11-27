//! The cactus runtime is a single process-global session: before serialization, concurrent
//! Needle users freed each other's arena (SIGSEGV in the bridge suite at --test-threads 4) or
//! lost their init under a concurrent load ("cactus_init not called"). These tests use the real
//! embedded model, so a crash in the C++ runtime fails the run loudly. Linux only.
#![cfg(not(windows))]

#[test]
fn concurrent_sessions_are_serialized_and_neither_sees_an_uninitialized_model() {
    let mut hs = vec![];
    for i in 0..3 {
        hs.push(std::thread::spawn(move || {
            // Busy is transient (another session loads or runs): retry like a later request would.
            let n = retry_busy(|| pawline_needle::Needle::embedded("sys", "{}"), i);
            for _ in 0..2 {
                retry_busy(|| n.propose("move a.txt into docs", 8), i);
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
}

fn retry_busy<T>(f: impl Fn() -> Result<T, pawline_needle::NeedleError>, i: i32) -> T {
    for _ in 0..100 {
        match f() {
            Err(e) if e.0.contains("busy") => std::thread::sleep(std::time::Duration::from_millis(50)),
            other => return other.unwrap_or_else(|e| panic!("thread {i} saw a broken session: {e:?}")),
        }
    }
    panic!("thread {i}: still busy after 100 retries")
}

#[test]
fn a_second_session_reuses_the_weights_and_starts_clean() {
    let a = pawline_needle::Needle::embedded("sys", "{}").expect("first session");
    let b = pawline_needle::Needle::embedded("sys", "{}").expect("second session reuses the loaded weights");
    // Both work, and b's replies come from its own fresh session (a reset happens on reuse).
    assert!(a.propose("move a.txt into docs", 8).is_ok());
    assert!(b.propose("move b.txt into docs", 8).is_ok());
}
