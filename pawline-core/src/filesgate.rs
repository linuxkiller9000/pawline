//! One gate in front of every file change Pawline makes (item 14: a files permission with revocation).
//! While the "File changes" switch is blocked, Pawline stays read-only: no plan that would create,
//! change, move, delete or save anything reaches a confirmation card (a card would promise a change),
//! and `execute_plan` refuses anyway, so a card confirmed before the switch was flipped still writes
//! nothing. Reading, listing, searching and asking stay available. Programs the user opens are their
//! own act (same reasoning as the offline-mode note): this gate covers Pawline's own operations only.
//! Any new write operation must be added to `changes_files`; the tests below pin the full list.
use crate::error::PawError;
use crate::plan::Operation;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static READ_ONLY: AtomicBool = AtomicBool::new(false);
static BLOCKED: AtomicU64 = AtomicU64::new(0);

pub fn set_read_only(on: bool) {
    READ_ONLY.store(on, Ordering::SeqCst);
}

thread_local! {
    static THREAD_READ_ONLY: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// For tests only: override read-only mode for the CURRENT thread, so parallel tests do not see each other's switch.
/// The app never calls this; it uses `set_read_only`.
pub fn set_read_only_for_this_thread(v: Option<bool>) {
    THREAD_READ_ONLY.with(|c| c.set(v));
}

pub fn is_read_only() -> bool {
    THREAD_READ_ONLY.with(|c| c.get()).unwrap_or_else(|| READ_ONLY.load(Ordering::SeqCst))
}

/// Write operations the gate refused since the process started.
pub fn blocked_count() -> u64 {
    BLOCKED.load(Ordering::SeqCst)
}

/// True when the operation changes a file or the desktop on Pawline's own initiative.
/// `OpenApp` is the user opening a program (their act, like the offline note), and
/// `List`/`Answer`/online lookups write nothing, so they are not file changes.
pub fn changes_files(op: &Operation) -> bool {
    matches!(
        op,
        Operation::CreateDir { .. }
            | Operation::CreateFile { .. }
            | Operation::Delete { .. }
            | Operation::Rename { .. }
            | Operation::Move { .. }
            | Operation::Copy { .. }
            | Operation::Shrink { .. }
            | Operation::Deck { .. }
            | Operation::DeckText { .. }
            | Operation::SetWallpaper { .. }
            | Operation::Undo { .. }
            | Operation::GenerateImage { .. }
    )
}

pub const READ_ONLY_MESSAGE: &str =
    "File changes are blocked, so Pawline stays read-only: nothing was changed, saved or deleted. Turn \"File changes\" back to allowed in Settings to do this.";

/// Call before showing a plan or executing it. Err means: no card, no change.
pub fn check_ops(ops: &[Operation]) -> Result<(), PawError> {
    if is_read_only() && ops.iter().any(changes_files) {
        BLOCKED.fetch_add(1, Ordering::SeqCst);
        return Err(PawError::Exec(READ_ONLY_MESSAGE.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Operation;
    use std::path::PathBuf;

    fn p() -> PathBuf {
        PathBuf::from("/tmp/x")
    }

    #[test]
    fn the_write_list_is_exact_and_complete() {
        // Every operation that makes Pawline change a file or the desktop must be in here;
        // a new Operation variant makes this test fail to compile until it is classified.
        for op in [
            Operation::CreateDir { path: p() },
            Operation::CreateFile { path: p() },
            Operation::Delete { path: p() },
            Operation::Rename { from: p(), to: p() },
            Operation::Move { from: p(), to: p() },
            Operation::Copy { from: p(), to: p() },
            Operation::Shrink { from: p(), to: p() },
            Operation::Deck { from: p(), to: p() },
            Operation::DeckText { text: "t".into(), to: p() },
            Operation::SetWallpaper { image: p(), via: None, no_backup_ok: true },
            Operation::Undo { entry: 0, summary: "s".into() },
            Operation::GenerateImage { provider: crate::imagegen::ImageProvider::Pollinations, prompt: "x".into() },
        ] {
            assert!(changes_files(&op), "{}", op.describe());
        }
        for op in [
            Operation::List { path: p(), page: 0, contains: None },
            Operation::Answer { text: "a".into() },
            Operation::Search { engine: crate::plan::SearchEngine::Web, query: "q".into() },
            Operation::Weather { place: "here".into() },
            Operation::DeckDraft { topic: "t".into() },
            Operation::AskOnline { text: "q".into() },
            Operation::ChatOnline { context: vec![], text: "hi".into() },
            Operation::OpenApp { name: "x".into(), argv: vec!["x".into()] },
        ] {
            assert!(!changes_files(&op), "{}", op.describe());
        }
    }

    #[test]
    fn gate_blocks_only_writes_and_counts_them() {
        set_read_only_for_this_thread(Some(true));
        let before = blocked_count();
        assert!(check_ops(&[Operation::Delete { path: p() }]).is_err());
        assert!(check_ops(&[Operation::List { path: p(), page: 0, contains: None }]).is_ok());
        assert!(check_ops(&[Operation::List { path: p(), page: 0, contains: None }, Operation::Copy { from: p(), to: p() }]).is_err(), "one write in a mixed plan blocks the whole plan");
        assert_eq!(blocked_count(), before + 2);
        set_read_only_for_this_thread(Some(false));
        assert!(check_ops(&[Operation::Delete { path: p() }]).is_ok());
        set_read_only_for_this_thread(None);
    }

    #[test]
    fn engine_never_cards_nor_runs_a_write_while_read_only_and_a_pre_confirmed_plan_still_writes_nothing() {
        use crate::exec::DirectFsBackend;
        use crate::{Engine, Response};
        let d = std::env::temp_dir().join(format!("pawline-fg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let root = std::fs::canonicalize(&d).unwrap();
        std::fs::write(root.join("victim.txt"), b"keep me").unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        let be = DirectFsBackend::new(&root).unwrap();
        let mut e = Engine::new(root.clone());

        // 1. confirmed plan made while changes are allowed
        let plan = match e.handle("delete victim.txt", &be, false).unwrap() {
            Response::Confirm(p) => p,
            other => panic!("expected a confirm card"),
        };
        // 2. the user flips read-only on before clicking Do it: the plan writes nothing
        set_read_only_for_this_thread(Some(true));
        match e.execute_plan(plan, &be).unwrap() {
            Response::Message(m) => assert!(m.contains("read-only"), "{m}"),
            other => panic!("expected a refusal message"),
        }
        assert_eq!(std::fs::read(root.join("victim.txt")).unwrap(), b"keep me", "the pre-confirmed delete changed nothing");
        // 3. fresh commands: writes get the message, no card; reads still work
        match e.handle("delete victim.txt", &be, false).unwrap() {
            Response::Message(m) => assert!(m.contains("read-only"), "{m}"),
            other => panic!("expected a refusal message"),
        }
        match e.handle("make a folder called newdir", &be, false).unwrap() {
            Response::Message(m) => assert!(m.contains("read-only"), "{m}"),
            other => panic!("expected a refusal message"),
        }
        assert!(!root.join("newdir").exists());
        let listed = match e.handle("list docs", &be, false).unwrap() {
            Response::Confirm(p) => e.execute_plan(p, &be).unwrap(),
            other => other,
        };
        match listed {
            Response::Executed(_) => {}
            Response::Message(m) => panic!("reads must still work: {m}"),
            Response::Failed { error, .. } => panic!("reads must still work: {error}"),
            _ => panic!("reads must still work"),
        }
        // 4. back to allowed: the same delete cards again
        set_read_only_for_this_thread(Some(false));
        match e.handle("delete victim.txt", &be, false).unwrap() {
            Response::Confirm(_) => {}
            other => panic!("expected a confirm card after re-allowing"),
        }
        set_read_only_for_this_thread(None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
