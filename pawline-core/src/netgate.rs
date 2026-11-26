//! One gate in front of every network request Pawline makes (item 14). While offline mode is on, nothing is sent: the check
//! runs before the connection is opened, so no DNS lookup, no connection and no bytes leave the machine.
//! The two real senders (`imagegen::UreqHttp` for weather, pictures, decks and questions, and the search lookups) call `check`.
//! Any new network code must call it too; the test below only proves the senders that exist today.
use crate::error::PawError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static OFFLINE: AtomicBool = AtomicBool::new(false);
static ATTEMPTS: AtomicU64 = AtomicU64::new(0);

pub fn set_offline(on: bool) {
    OFFLINE.store(on, Ordering::SeqCst);
}

thread_local! {
    static THREAD_OFFLINE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// For tests only: override offline mode for the CURRENT thread, so parallel tests do not see each other's switch.
/// The app never calls this; it uses `set_offline`.
pub fn set_offline_for_this_thread(v: Option<bool>) {
    THREAD_OFFLINE.with(|c| c.set(v));
}

pub fn is_offline() -> bool {
    THREAD_OFFLINE.with(|c| c.get()).unwrap_or_else(|| OFFLINE.load(Ordering::SeqCst))
}

/// Requests that passed the gate (were allowed to open a connection) since the process started.
pub fn attempts() -> u64 {
    ATTEMPTS.load(Ordering::SeqCst)
}

/// Call right before opening any connection. Err means: do not send anything.
pub fn check() -> Result<(), PawError> {
    if is_offline() {
        return Err(PawError::Exec("Offline mode is on, so nothing was sent. Turn it off in Settings to use online features.".into()));
    }
    ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imagegen::{Http, UreqHttp};
    use crate::plan::SearchEngine;
    use std::time::Duration;

    #[test]
    fn offline_mode_stops_every_real_sender_before_a_connection_is_opened() {
        set_offline_for_this_thread(Some(true));
        let before = attempts();
        let h = UreqHttp::new(Duration::from_secs(2));
        // an unroutable local address: if the gate failed, the error would be a network problem, not the offline message
        for r in [h.get("http://127.0.0.1:9/x", &[]), h.post("http://127.0.0.1:9/x", &[], "{}")] {
            let e = r.err().expect("must fail").to_string();
            assert!(e.contains("Offline mode is on"), "{e}");
        }
        for eng in [SearchEngine::Web, SearchEngine::Wiki] {
            let e = crate::search::run(eng, "tux").unwrap_err().to_string();
            assert!(e.contains("Offline mode is on"), "{e}");
        }
        assert_eq!(attempts(), before, "no request passed the gate while offline");
        set_offline_for_this_thread(Some(false));
        // back online the gate lets one through (to a closed local port, so it fails as a network problem, not as offline)
        let e = h.get("http://127.0.0.1:9/x", &[]).err().expect("closed port").to_string();
        assert!(!e.contains("Offline mode is on"), "{e}");
        assert_eq!(attempts(), before + 1);
    }
}
