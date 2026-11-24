use pawline_core::exec::{DirectFsBackend, DryRunBackend};
use pawline_core::{Engine, Response};
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawline-hard-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A symlink inside home pointing OUTSIDE must not let a create escape,
/// including when the final child does not exist yet.
#[test]
fn symlink_escape_blocked() {
    let root = sandbox("sym");
    let outside = std::env::temp_dir().join(format!("pawline-outside-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("sneaky")).unwrap();

    let mut engine = Engine::new(root.clone());
    // existing dir behind the symlink
    let err = engine.plan("make a folder in sneaky called loot").unwrap_err();
    assert!(err.to_string().contains("outside"), "got: {err}");
    // non-existing child under a symlinked parent
    std::fs::create_dir(outside.join("deep")).unwrap();
    let err = engine
        .plan("create a file in sneaky/deep called x.md")
        .unwrap_err();
    assert!(err.to_string().contains("outside"), "got: {err}");
    assert!(!outside.join("loot").exists());
    assert!(!outside.join("deep").join("x.md").exists());
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
}

/// Rename onto an existing path is an explicit refusal, not a confirmation
/// that cannot succeed: no overwrite feature exists yet.
#[test]
fn rename_overwrite_refused() {
    let root = sandbox("ren");
    std::fs::create_dir(root.join("a")).unwrap();
    std::fs::create_dir(root.join("b")).unwrap();
    let mut engine = Engine::new(root.clone());
    let err = engine.plan("rename a to b").unwrap_err();
    assert!(err.to_string().contains("overwrite"), "got: {err}");
    assert!(root.join("a").is_dir());
    assert!(root.join("b").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

/// ...and the backend independently refuses to clobber, whatever the caller.
#[test]
fn backend_never_clobbers() {
    use pawline_core::exec::ExecutionBackend;
    use pawline_core::plan::Operation;
    let root = sandbox("clob");
    std::fs::create_dir(root.join("a")).unwrap();
    std::fs::create_dir(root.join("b")).unwrap();
    std::fs::write(root.join("b").join("keep.txt"), b"precious").unwrap();
    // hand-built op: no engine, no validation, straight at the backend
    let op = Operation::Rename {
        from: root.join("a"),
        to: root.join("b"),
    };
    let err = DirectFsBackend::new(&root).unwrap().execute(&op).unwrap_err();
    assert!(err.to_string().contains("overwriting"), "got: {err}");
    assert_eq!(std::fs::read(root.join("b").join("keep.txt")).unwrap(), b"precious");
    assert!(root.join("a").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

/// Confirmation is bound to the exact plan object returned, not a re-parse.
#[test]
fn confirmation_executes_the_same_plan() {
    let root = sandbox("immut");
    let mut engine = Engine::new(root.clone());
    engine.handle("make a folder called victim", &DirectFsBackend::new(&root).unwrap(), false).unwrap();
    let r = engine.handle("delete the folder called victim", &DirectFsBackend::new(&root).unwrap(), false).unwrap();
    let plan = match r {
        Response::Confirm(p) => p,
        _ => panic!("expected confirm"),
    };
    // Between ask and confirm, the world changed: victim was renamed away.
    std::fs::rename(root.join("victim"), root.join("renamed-away")).unwrap();
    // The confirmed plan still names the ORIGINAL path and fails loudly
    // instead of deleting whatever now matches a fresh parse.
    let backend = DryRunBackend::default();
    let r = engine.execute_plan(plan, &backend).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    let log = backend.log.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert!(log[0].contains("victim"), "got: {}", log[0]);
    assert!(root.join("renamed-away").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

/// A mid-plan failure reports completed ops, the failing op and the rest,
/// and context only knows about what actually happened.
#[test]
fn partial_failure_reporting() {
    let root = sandbox("part");
    let mut engine = Engine::new(root.clone());
    // second create collides with an existing file -> fails at execution
    std::fs::write(root.join("blocker"), b" occupied").unwrap();
    // planning with the blocker in place is a validation error
    let err = engine
        .plan("make a folder called first. make a folder called blocker. make a folder called third")
        .unwrap_err();
    assert!(err.to_string().contains("already exists"), "got: {err}");

    // execution-time failure: plan while clear, then sabotage between.
    let plan = engine
        .plan("make a folder called first. make a folder called second. make a folder called third")
        .unwrap();
    std::fs::create_dir(root.join("second")).unwrap(); // sabotage
    let r = engine.execute_plan(plan, &DirectFsBackend::new(&root).unwrap()).unwrap();
    match r {
        Response::Failed { completed, failed, remaining, .. } => {
            assert_eq!(completed.len(), 1);
            assert!(failed.contains("second"), "got: {failed}");
            assert_eq!(remaining, 1);
        }
        _ => panic!("expected partial failure"),
    }
    assert!(root.join("first").is_dir());
    assert!(!root.join("third").exists());
    // context: "first" known, "third" not
    assert!(engine.context().resolve_named("first").is_some());
    assert!(engine.context().resolve_named("third").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// TOCTOU regression: a directory swapped for a symlink between planning and
/// execution must not redirect the write. Confinement is enforced at syscall
/// time by the rooted directory handle, not only at validation time.
#[test]
fn symlink_swap_between_plan_and_execute_is_confined() {
    let root = sandbox("swap");
    let outside = std::env::temp_dir().join(format!("pawline-outside-swap-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir(root.join("realdir")).unwrap();

    let mut engine = Engine::new(root.clone());
    let plan = engine.plan("make a folder in realdir called loot").unwrap();

    // sabotage: swap the real directory for a symlink pointing outside
    std::fs::remove_dir(root.join("realdir")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("realdir")).unwrap();

    let r = engine.execute_plan(plan, &DirectFsBackend::new(&root).unwrap()).unwrap();
    match r {
        Response::Failed { .. } => {} // refused: expected
        Response::Executed(_) => panic!("write went through a swapped symlink"),
        _ => panic!("unexpected response"),
    }
    assert!(!outside.join("loot").exists(), "write escaped the root");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
}
