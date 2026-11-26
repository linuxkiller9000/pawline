//! Optional assist path: when the deterministic parser cannot handle an
//! input, a pluggable assist (the embedded Needle3 runtime, or anything else)
//! may PROPOSE directives. Proposals re-enter the normal pipeline at
//! validation — the assist never touches execution, and the validator's
//! confinement rules apply to model output exactly as they apply to parser
//! output.

use crate::error::PawError;
use crate::plan::Directive;

pub trait Assist {
    /// Propose directives for an input the parser refused.
    fn propose(&self, input: &str) -> Result<Vec<Directive>, PawError>;
}

/// Runs an assist on a worker thread with a deadline. If the model does not answer in time, the caller gets an
/// ordinary assist failure (so the user sees the parser's plain explanation) and the assist is switched off for the
/// rest of the session: the abandoned call may still hold the model, so it is never asked again. Nothing here can
/// stop the stuck call itself; it only stops the app from waiting for it.
pub struct TimedAssist {
    inner: std::sync::Arc<dyn Assist + Send + Sync>,
    limit: std::time::Duration,
    dead: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl TimedAssist {
    pub fn new(inner: std::sync::Arc<dyn Assist + Send + Sync>, limit: std::time::Duration) -> Self {
        TimedAssist { inner, limit, dead: Default::default() }
    }

    /// True once a call has timed out or crashed.
    pub fn switched_off(&self) -> bool {
        self.dead.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Assist for TimedAssist {
    fn propose(&self, input: &str) -> Result<Vec<Directive>, PawError> {
        use std::sync::atomic::Ordering::SeqCst;
        if self.dead.load(SeqCst) {
            return Err(PawError::Parse("assist is switched off for this session".into()));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (inner, text) = (self.inner.clone(), input.to_string());
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inner.propose(&text)));
            let _ = tx.send(r);
        });
        match rx.recv_timeout(self.limit) {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => {
                self.dead.store(true, SeqCst);
                Err(PawError::Parse("assist crashed".into()))
            }
            Err(_) => {
                self.dead.store(true, SeqCst);
                Err(PawError::Parse("assist timed out".into()))
            }
        }
    }
}

#[cfg(test)]
mod timed_tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct Slow(Duration);
    impl Assist for Slow {
        fn propose(&self, _i: &str) -> Result<Vec<Directive>, PawError> {
            std::thread::sleep(self.0);
            Ok(vec![])
        }
    }
    struct Boom;
    impl Assist for Boom {
        fn propose(&self, _i: &str) -> Result<Vec<Directive>, PawError> {
            panic!("model crash")
        }
    }

    #[test]
    fn fast_answers_pass_through_and_do_not_switch_it_off() {
        let t = TimedAssist::new(Arc::new(Slow(Duration::from_millis(5))), Duration::from_secs(5));
        assert!(t.propose("x").is_ok());
        assert!(!t.switched_off());
    }

    #[test]
    fn a_hung_model_returns_at_the_deadline_and_is_not_asked_again() {
        let t = TimedAssist::new(Arc::new(Slow(Duration::from_secs(3))), Duration::from_millis(150));
        let t0 = Instant::now();
        assert!(t.propose("x").is_err());
        assert!(t0.elapsed() < Duration::from_secs(2), "returned at the deadline, not after the hang: {:?}", t0.elapsed());
        assert!(t.switched_off());
        let t1 = Instant::now();
        assert!(t.propose("y").is_err());
        assert!(t1.elapsed() < Duration::from_millis(50), "later calls fail at once");
    }

    #[test]
    fn a_crash_is_a_failure_and_switches_it_off() {
        let t = TimedAssist::new(Arc::new(Boom), Duration::from_secs(5));
        assert!(t.propose("x").is_err());
        assert!(t.switched_off());
    }
}
