use pawline_core::exec::{DirectFsBackend, DryRunBackend};
use pawline_core::{Engine, Response};
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pawline-test-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The brief's acceptance example, end to end on a real (temp) filesystem.
#[test]
fn kitten_paws_example() {
    let root = sandbox("kitten");
    let mut engine = Engine::new(root.clone());

    let r = engine
        .handle(
            "Make a folder in my home directory called kitten paws.",
            &DirectFsBackend::new(&root).unwrap(),
            false,
        )
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    let kp = root.join("kitten paws");
    assert!(kp.is_dir());

    let r = engine
        .handle(
            "Inside it create folders called gumpus, katkota, and pics.",
            &DirectFsBackend::new(&root).unwrap(),
            false,
        )
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(kp.join("gumpus").is_dir());
    assert!(kp.join("katkota").is_dir());
    assert!(kp.join("pics").is_dir());

    let r = engine
        .handle(
            "Inside pics create a Markdown file called kitten paws.md.",
            &DirectFsBackend::new(&root).unwrap(),
            false,
        )
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(kp.join("pics").join("kitten paws.md").is_file());

    let _ = std::fs::remove_dir_all(&root);
}

/// The whole thing as one message, the way people actually talk.
#[test]
fn kitten_paws_single_message() {
    let root = sandbox("kitten-one");
    let mut engine = Engine::new(root.clone());
    let r = engine
        .handle(
            "Make a folder in my home directory called kitten paws. Inside it create folders called gumpus, katkota, and pics. Inside pics create a Markdown file called kitten paws.md.",
            &DirectFsBackend::new(&root).unwrap(),
            false,
        )
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    let kp = root.join("kitten paws");
    assert!(kp.join("gumpus").is_dir());
    assert!(kp.join("katkota").is_dir());
    assert!(kp.join("pics").join("kitten paws.md").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn synonyms_and_markdown_hint() {
    let root = sandbox("syn");
    let mut engine = Engine::new(root.clone());
    engine
        .handle("create a directory named notes in ~", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    assert!(root.join("notes").is_dir());
    engine
        .handle("inside notes create a markdown file called todo", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    // no extension given, markdown hint applies
    assert!(root.join("notes").join("todo.md").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn delete_requires_confirmation_and_then_runs() {
    let root = sandbox("del");
    let mut engine = Engine::new(root.clone());
    engine
        .handle("make a folder called scratch", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    assert!(root.join("scratch").is_dir());

    // without confirmation: plan returned, nothing deleted
    let r = engine
        .handle("delete the folder called scratch", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    match r {
        Response::Confirm(plan) => {
            assert!(plan.needs_confirmation);
            assert_eq!(plan.confirmation_reasons.len(), 1);
        }
        _ => panic!("expected a confirmation request"),
    }
    assert!(root.join("scratch").is_dir());

    // confirmed: actually deleted
    let r = engine
        .handle("delete the folder called scratch", &DirectFsBackend::new(&root).unwrap(), true)
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(!root.join("scratch").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn traversal_and_home_delete_rejected() {
    let root = sandbox("safe");
    let mut engine = Engine::new(root.clone());
    let err = engine.plan("make a folder called ../evil").unwrap_err();
    assert!(err.to_string().contains("path") || err.to_string().contains("allowed"));

    // delete ~ resolves to the root itself and must be refused
    let err = engine.plan("delete ~").unwrap_err();
    assert!(err.to_string().contains("home") || err.to_string().contains("allowed"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_reference_is_a_helpful_error() {
    let root = sandbox("ref");
    let mut engine = Engine::new(root.clone());
    let err = engine
        .plan("inside banana create a folder called x")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("banana"), "got: {msg}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dry_run_touches_nothing() {
    let root = sandbox("dry");
    let mut engine = Engine::new(root.clone());
    let backend = DryRunBackend::default();
    let r = engine
        .handle("make a folder called ghost", &backend, false)
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(!root.join("ghost").exists());
    assert_eq!(backend.log.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_reports_contents() {
    let root = sandbox("list");
    let mut engine = Engine::new(root.clone());
    engine
        .handle("make a folder called a. inside it create a file called f.txt", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    let r = engine.handle("list a", &DirectFsBackend::new(&root).unwrap(), false).unwrap();
    match r {
        Response::Executed(outcomes) => {
            assert!(outcomes[0].detail.contains("f.txt"), "got: {}", outcomes[0].detail);
        }
        _ => panic!("expected listing"),
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn duplicate_create_is_a_validation_error() {
    let root = sandbox("dup");
    let mut engine = Engine::new(root.clone());
    engine
        .handle("make a folder called twice", &DirectFsBackend::new(&root).unwrap(), false)
        .unwrap();
    let err = engine.plan("make a folder called twice").unwrap_err();
    assert!(err.to_string().contains("already exists"), "got: {err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_parent_is_a_friendly_offer_not_an_os_error() {
    let root = sandbox("parent");
    let mut engine = Engine::new(root.clone());
    // "make a folder called pics in old/stuff" where neither exists: the reply names the nearest
    // missing folder and offers the parents variant; no raw OS error text, nothing created.
    let text = match engine.handle("make a folder called pics in old/stuff", &DirectFsBackend::new(&root).unwrap(), false) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected a validation error"),
    };
    assert!(text.contains("does not exist yet"), "{text}");
    assert!(text.contains("with parent folders"), "{text}");
    assert!(!text.contains("os error"), "{text}");
    assert!(!root.join("old").exists());

    // With the parents ask, the missing folders are created as their own confirmed steps.
    let r = engine
        .handle("make a folder called pics in old/stuff with parent folders", &DirectFsBackend::new(&root).unwrap(), true)
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("old/stuff/pics").is_dir());

    // Undo reverses one journaled step at a time, newest first; three undos remove all three.
    let be = DirectFsBackend::new(&root).unwrap();
    for expect_gone in ["old/stuff/pics", "old/stuff", "old"] {
        let r = engine.handle("undo", &be, true).unwrap();
        assert!(matches!(r, Response::Executed(_)), "undo of {expect_gone}");
        assert!(!root.join(expect_gone).exists(), "{expect_gone} still there after undo");
    }
    let _ = std::fs::remove_dir_all(&root);
}
