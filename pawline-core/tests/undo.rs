use pawline_core::exec::DirectFsBackend;
use pawline_core::{Engine, Response};
use std::path::PathBuf;

fn setup(tag: &str) -> (PathBuf, Engine, DirectFsBackend) {
    let d = std::env::temp_dir().join(format!("pawline-undo-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = std::fs::canonicalize(d).unwrap();
    (d.clone(), Engine::new(d.clone()), DirectFsBackend::new(&d).unwrap())
}

fn run(e: &mut Engine, be: &DirectFsBackend, s: &str) -> Response {
    let r = e.handle(s, be, false).unwrap();
    match r {
        Response::Confirm(p) => e.execute_plan(p, be).unwrap(),
        r => r,
    }
}

#[test]
fn delete_keeps_data_and_undo_restores_it() {
    let (root, mut e, be) = setup("del");
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/a.txt"), b"precious").unwrap();
    // delete asks first
    assert!(matches!(e.handle("delete docs", &be, false).unwrap(), Response::Confirm(_)));
    assert!(root.join("docs/a.txt").exists());
    assert!(matches!(run(&mut e, &be, "delete docs"), Response::Executed(_)));
    assert!(!root.join("docs").exists());
    let trashed: Vec<_> = std::fs::read_dir(root.join(".local/share/pawline/trash")).unwrap().collect();
    assert_eq!(trashed.len(), 1);
    // undo asks first and describes what it will do
    match e.handle("undo", &be, false).unwrap() {
        Response::Confirm(p) => assert!(p.summary().contains("restore ~/docs"), "{}", p.summary()),
        _ => panic!("undo must confirm"),
    }
    assert!(!root.join("docs").exists());
    assert!(matches!(run(&mut e, &be, "undo"), Response::Executed(_)));
    assert_eq!(std::fs::read(root.join("docs/a.txt")).unwrap(), b"precious");
    // nothing further to undo except the earlier create-less history: journal now empty of open entries
    assert!(e.plan("undo").is_err());
}

#[test]
fn undo_create_only_removes_empty() {
    let (root, mut e, be) = setup("create");
    run(&mut e, &be, "make a folder called pics");
    run(&mut e, &be, "make a file called note.txt");
    assert!(root.join("pics").is_dir());
    // file got content: undo refuses, keeps it
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    match run(&mut e, &be, "undo") {
        Response::Failed { .. } => {}
        _ => panic!("must refuse"),
    }
    assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), b"hello");
}

#[test]
fn undo_rename_and_refuse_overwrite() {
    let (root, mut e, be) = setup("ren");
    std::fs::write(root.join("a.txt"), b"1").unwrap();
    run(&mut e, &be, "rename a.txt to b.txt");
    assert!(root.join("b.txt").exists() && !root.join("a.txt").exists());
    std::fs::write(root.join("a.txt"), b"new").unwrap();
    assert!(matches!(run(&mut e, &be, "undo"), Response::Failed { .. }));
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"new");
    std::fs::remove_file(root.join("a.txt")).unwrap();
    assert!(matches!(run(&mut e, &be, "undo"), Response::Executed(_)));
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"1");
}

#[test]
fn history_change_between_plan_and_confirm_is_refused() {
    let (root, mut e, be) = setup("hist");
    std::fs::write(root.join("a.txt"), b"1").unwrap();
    run(&mut e, &be, "rename a.txt to b.txt");
    let plan = e.plan("undo").unwrap();
    run(&mut e, &be, "make a folder called zzz");
    match e.execute_plan(plan, &be).unwrap() {
        Response::Failed { .. } => {}
        _ => panic!("stale undo must fail"),
    }
    assert!(root.join("b.txt").exists());
}

#[test]
fn pawline_store_is_off_limits_and_undo_with_no_history_errors() {
    let (root, mut e, be) = setup("store");
    assert!(e.plan("undo").is_err());
    std::fs::write(root.join("x"), b"").unwrap();
    run(&mut e, &be, "delete x");
    assert!(e.plan("delete .local/share/pawline").is_err());
    assert!(e.plan("delete ~/.local/share/pawline/trash").is_err());
}

#[test]
fn undo_of_a_copy_keeps_a_copy_the_user_added_to_and_moves_an_untouched_one_to_the_trash() {
    let (root, mut e, be) = setup("copyundo");
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::create_dir(root.join("backup")).unwrap();
    std::fs::write(root.join("docs/a.txt"), b"one").unwrap();
    assert!(matches!(run(&mut e, &be, "copy docs to backup"), Response::Executed(_)));
    assert_eq!(std::fs::read(root.join("backup/docs/a.txt")).unwrap(), b"one");
    // the user added a file to the copy: undo refuses and leaves it all there
    std::fs::write(root.join("backup/docs/mine.txt"), b"my work").unwrap();
    match run(&mut e, &be, "undo") {
        Response::Failed { .. } => {}
        _ => panic!("must refuse to undo a copy that was changed"),
    }
    assert_eq!(std::fs::read(root.join("backup/docs/mine.txt")).unwrap(), b"my work");
    assert_eq!(std::fs::read(root.join("docs/a.txt")).unwrap(), b"one");
}

#[test]
fn undo_of_an_untouched_copy_moves_it_to_the_trash() {
    let (root, mut e, be) = setup("copyundo2");
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::create_dir(root.join("backup")).unwrap();
    std::fs::write(root.join("docs/a.txt"), b"one").unwrap();
    assert!(matches!(run(&mut e, &be, "copy docs to backup"), Response::Executed(_)));
    assert!(matches!(run(&mut e, &be, "undo"), Response::Executed(_)));
    assert!(!root.join("backup/docs").exists());
    let trash = root.join(".local/share/pawline/trash");
    assert!(std::fs::read_dir(trash).unwrap().flatten().any(|f| f.path().join("a.txt").exists()));
    assert_eq!(std::fs::read(root.join("docs/a.txt")).unwrap(), b"one");
}

fn copied_tree(tag: &str) -> (PathBuf, Engine, DirectFsBackend) {
    let (root, mut e, be) = setup(tag);
    std::fs::create_dir_all(root.join("docs/deep/er")).unwrap();
    std::fs::create_dir(root.join("backup")).unwrap();
    std::fs::write(root.join("docs/deep/er/f.txt"), b"one").unwrap();
    assert!(matches!(run(&mut e, &be, "copy docs to backup"), Response::Executed(_)));
    (root, e, be)
}

#[test]
fn undo_of_a_copy_refuses_nested_edit_and_nested_replacement_and_keeps_everything() {
    let (root, mut e, be) = copied_tree("nested1");
    // edit deep inside the copy (same folder, size changes)
    std::fs::write(root.join("backup/docs/deep/er/f.txt"), b"one and more").unwrap();
    assert!(matches!(run(&mut e, &be, "undo"), Response::Failed { .. }));
    assert_eq!(std::fs::read(root.join("backup/docs/deep/er/f.txt")).unwrap(), b"one and more");
    let (root, mut e, be) = copied_tree("nested2");
    // same-size replacement by a different file deep inside the copy
    let f = root.join("backup/docs/deep/er/f.txt");
    std::fs::write(root.join("backup/docs/deep/er/g.tmp"), b"two").unwrap();
    std::fs::rename(root.join("backup/docs/deep/er/g.tmp"), &f).unwrap();
    assert!(matches!(run(&mut e, &be, "undo"), Response::Failed { .. }));
    assert_eq!(std::fs::read(&f).unwrap(), b"two");
}

#[test]
fn undo_of_a_legacy_copy_entry_without_fingerprint_fails_closed() {
    let (root, mut e, be) = setup("legacycopy");
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    std::fs::create_dir_all(root.join(".local/share/pawline")).unwrap();
    std::fs::write(root.join("b.txt"), b"replacement data").unwrap();
    let j = r#"{"id":1,"kind":"copy","a":"b.txt"}"#;
    let _ = j;
    let entry = pawline_core::journal::Entry { id: 1, kind: "copy".into(), a: "b.txt".into(), b: None, meta: None };
    let text = format!("{}\n{}\n", entry.intent_line(), pawline_core::journal::commit_line(1, None));
    std::fs::write(root.join(".local/share/pawline/journal.jsonl"), text).unwrap();
    assert!(matches!(run(&mut e, &be, "undo"), Response::Failed { .. }));
    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"replacement data");
}
