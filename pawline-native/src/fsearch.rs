//! One background worker for the "/in" folder search. Typing only overwrites a single "latest query" slot, so the
//! number of threads is always 1 and at most one search (and one zoxide subprocess) runs at a time, however fast
//! the user types. A result is delivered only if its query is still the latest one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub type Finder = Arc<dyn Fn(&str, &dyn Fn() -> bool) -> Vec<PathBuf> + Send + Sync>;
pub type Deliver = Arc<dyn Fn(u64, String, Vec<PathBuf>) + Send + Sync>;

struct Slot {
    pending: Mutex<Option<(u64, String)>>,
    cv: Condvar,
    gen: AtomicU64,
}

pub struct Searcher {
    slot: Arc<Slot>,
    /// Threads ever started by this searcher (stays 1).
    pub threads: Arc<AtomicUsize>,
    started: Mutex<bool>,
    finder: Finder,
    deliver: Deliver,
    debounce: Duration,
}

impl Searcher {
    pub fn new(finder: Finder, deliver: Deliver, debounce: Duration) -> Searcher {
        Searcher {
            slot: Arc::new(Slot { pending: Mutex::new(None), cv: Condvar::new(), gen: AtomicU64::new(0) }),
            threads: Arc::new(AtomicUsize::new(0)),
            started: Mutex::new(false),
            finder,
            deliver,
            debounce,
        }
    }

    pub fn gen(&self) -> u64 {
        self.slot.gen.load(Ordering::SeqCst)
    }

    /// Ask for a new search; anything older is cancelled. Returns the generation of this request.
    pub fn request(&self, q: &str) -> u64 {
        self.ensure_worker();
        let g = self.slot.gen.fetch_add(1, Ordering::SeqCst) + 1;
        *self.slot.pending.lock().unwrap() = Some((g, q.to_string()));
        self.slot.cv.notify_one();
        g
    }

    /// Cancel whatever is running or waiting and drop its result.
    pub fn cancel(&self) {
        self.slot.gen.fetch_add(1, Ordering::SeqCst);
        *self.slot.pending.lock().unwrap() = None;
    }

    fn ensure_worker(&self) {
        let mut s = self.started.lock().unwrap();
        if *s {
            return;
        }
        *s = true;
        self.threads.fetch_add(1, Ordering::SeqCst);
        let (slot, finder, deliver, debounce) = (self.slot.clone(), self.finder.clone(), self.deliver.clone(), self.debounce);
        std::thread::spawn(move || loop {
            let (g, q) = {
                let mut p = slot.pending.lock().unwrap();
                while p.is_none() {
                    p = slot.cv.wait(p).unwrap();
                }
                p.take().unwrap()
            };
            std::thread::sleep(debounce);
            if slot.gen.load(Ordering::SeqCst) != g {
                continue; // superseded while waiting: the newer request is already pending
            }
            let rows = finder(&q, &|| slot.gen.load(Ordering::SeqCst) != g);
            if slot.gen.load(Ordering::SeqCst) == g {
                deliver(g, q, rows);
            }
        });
    }
}

/// A delivered result is used only when it is for the newest request and the box still holds that query.
pub fn accept_result(cur_gen: u64, cur_q: Option<&str>, gen: u64, q: &str) -> bool {
    gen == cur_gen && cur_q == Some(q)
}

/// Enter may pick a folder only when a finished list is showing: never while a search is pending or empty.
pub fn can_pick(waiting: bool, rows: usize) -> bool {
    !waiting && rows > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_for<T>(rx: &mpsc::Receiver<T>) -> T {
        rx.recv_timeout(Duration::from_secs(5)).expect("expected a delivery")
    }

    #[test]
    fn stale_result_is_never_delivered_and_newest_is() {
        let (dtx, drx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel::<String>();
        let deliver: Deliver = Arc::new(move |g, q, rows| {
            let _ = dtx.send((g, q, rows));
        });
        // "slow" blocks until it is cancelled (deterministic: no sleeping race), then returns a marker row
        let finder: Finder = {
            let started_tx = Mutex::new(started_tx);
            Arc::new(move |q, cancel| {
                let _ = started_tx.lock().unwrap().send(q.to_string());
                if q == "slow" {
                    while !cancel() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    return vec![PathBuf::from("/stale/wrong")];
                }
                vec![PathBuf::from(format!("/right/{q}"))]
            })
        };
        let s = Searcher::new(finder, deliver, Duration::from_millis(1));
        s.request("slow");
        assert_eq!(wait_for(&started_rx), "slow");
        let g2 = s.request("fast"); // supersedes while "slow" is still running
        let (g, q, rows) = wait_for(&drx);
        assert_eq!((g, q.as_str()), (g2, "fast"));
        assert_eq!(rows, vec![PathBuf::from("/right/fast")]);
        assert!(drx.recv_timeout(Duration::from_millis(200)).is_err(), "the stale result is never delivered");
        assert_eq!(s.threads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rapid_typing_uses_one_thread_and_few_searches() {
        let calls = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicUsize::new(0));
        let max_running = Arc::new(AtomicUsize::new(0));
        let (dtx, drx) = mpsc::channel();
        let deliver: Deliver = Arc::new(move |g, q, _| {
            let _ = dtx.send((g, q));
        });
        let finder: Finder = {
            let (calls, running, max_running) = (calls.clone(), running.clone(), max_running.clone());
            Arc::new(move |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                let r = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_running.fetch_max(r, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                running.fetch_sub(1, Ordering::SeqCst);
                vec![]
            })
        };
        let s = Searcher::new(finder, deliver, Duration::from_millis(60));
        let mut last = 0;
        for i in 0..300 {
            last = s.request(&format!("query{i}"));
        }
        let (g, q) = wait_for(&drx);
        assert_eq!((g, q.as_str()), (last, "query299"));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(s.threads.load(Ordering::SeqCst), 1, "never more than one worker thread");
        assert_eq!(max_running.load(Ordering::SeqCst), 1, "never two searches at once");
        assert!(calls.load(Ordering::SeqCst) <= 3, "300 keystrokes ran {} searches", calls.load(Ordering::SeqCst));
        assert!(drx.try_recv().is_err());
    }

    #[test]
    fn cancel_drops_a_running_search() {
        let (dtx, drx) = mpsc::channel();
        let (st_tx, st_rx) = mpsc::channel::<()>();
        let deliver: Deliver = Arc::new(move |g, q, _| {
            let _ = dtx.send((g, q));
        });
        let finder: Finder = {
            let st_tx = Mutex::new(st_tx);
            Arc::new(move |_, cancel| {
                let _ = st_tx.lock().unwrap().send(());
                while !cancel() {
                    std::thread::sleep(Duration::from_millis(2));
                }
                vec![]
            })
        };
        let s = Searcher::new(finder, deliver, Duration::from_millis(1));
        s.request("x");
        wait_for(&st_rx);
        s.cancel();
        assert!(drx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn enter_while_searching_and_stale_acceptance_rules() {
        assert!(!can_pick(true, 5), "Enter while searching must not pick, even if old rows were left");
        assert!(!can_pick(false, 0));
        assert!(can_pick(false, 3));
        assert!(accept_result(7, Some("abc"), 7, "abc"));
        assert!(!accept_result(8, Some("abc"), 7, "abc"), "older generation");
        assert!(!accept_result(7, Some("abd"), 7, "abc"), "box text changed");
        assert!(!accept_result(7, None, 7, "abc"), "chooser closed");
    }
}
