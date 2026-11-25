//! Text that came from speech recognition always needs a fresh confirmation.
use pawline_core::exec::DirectFsBackend;
use pawline_core::{Engine, Response};

fn setup(tag: &str) -> (std::path::PathBuf, Engine) {
    let d = std::env::temp_dir().join(format!("pawline-vf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = std::fs::canonicalize(d).unwrap();
    (d.clone(), Engine::new(d))
}
const WHY: &str = "this text came from speech recognition, so check the words before you say yes";

#[test]
fn typed_create_runs_but_voice_create_waits_for_confirmation() {
    let (root, mut e) = setup("a");
    let be = DirectFsBackend::new(&root).unwrap();
    // typed: the planner's normal behaviour (create needs no confirmation)
    assert!(matches!(e.handle_with_force("make a folder called typed", &be, false, None, None).unwrap(), Response::Executed(_)));
    assert!(root.join("typed").is_dir());
    // voice: same kind of command must not run
    match e.handle_with_force("make a folder called heard", &be, false, None, Some(WHY)).unwrap() {
        Response::Confirm(p) => {
            assert!(!root.join("heard").exists(), "voice create ran without confirmation");
            assert!(p.confirmation_reasons.iter().any(|r| r.contains("speech recognition")));
            // cancel = drop the plan: still nothing on disk
            drop(p);
            assert!(!root.join("heard").exists());
        }
        _ => panic!("expected a confirmation"),
    }
    // and after explicit confirmation it does run
    if let Response::Confirm(p) = e.handle_with_force("make a folder called heard", &be, false, None, Some(WHY)).unwrap() {
        assert!(matches!(e.execute_plan(p, &be).unwrap(), Response::Executed(_)));
    }
    assert!(root.join("heard").is_dir());
}

#[test]
fn voice_copy_and_list_also_wait_but_pure_math_answers() {
    let (root, mut e) = setup("b");
    std::fs::write(root.join("a.txt"), "x").unwrap();
    std::fs::create_dir(root.join("dest")).unwrap();
    let be = DirectFsBackend::new(&root).unwrap();
    for cmd in ["copy a.txt to dest", "list files"] {
        assert!(
            matches!(e.handle_with_force(cmd, &be, false, None, Some(WHY)), Ok(Response::Confirm(_)) | Err(_)),
            "{cmd} ran from voice without confirmation"
        );
    }
    assert!(!root.join("dest/a.txt").exists());
    assert!(matches!(e.handle_with_force("what is 2 plus 2", &be, false, None, Some(WHY)), Ok(Response::Executed(_)) | Ok(Response::Message(_)) | Err(_)));
}
