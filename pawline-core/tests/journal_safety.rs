use pawline_core::error::PawError;
use pawline_core::exec::DirectFsBackend;
use pawline_core::wallpaper::{Adapter, WallpaperSetter};
use pawline_core::{Engine, Response};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn setup(tag: &str) -> (PathBuf, Engine) {
    let d = std::env::temp_dir().join(format!("pawline-js-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = std::fs::canonicalize(d).unwrap();
    (d.clone(), Engine::new(d))
}

fn go(e: &mut Engine, be: &DirectFsBackend, s: &str) -> Response {
    match e.handle(s, be, false).unwrap() {
        Response::Confirm(p) => e.execute_plan(p, be).unwrap(),
        r => r,
    }
}

#[test]
fn journal_write_failure_means_no_change_at_all() {
    let (root, mut e) = setup("jfail");
    let store = root.join(".local/share/pawline");
    std::fs::create_dir_all(&store).unwrap();
    // make the journal unwritable: a directory sits where the file should be
    std::fs::create_dir(store.join("journal.jsonl")).unwrap();
    let be = DirectFsBackend::new(&root).unwrap();
    match go(&mut e, &be, "make a folder called pics") {
        Response::Failed { error, .. } => assert!(error.contains("nothing was changed"), "{error}"),
        _ => panic!("must fail"),
    }
    assert!(!root.join("pics").exists());
    std::fs::write(root.join("keep.txt"), b"k").unwrap();
    assert!(matches!(go(&mut e, &be, "delete keep.txt"), Response::Failed { .. }));
    assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"k");
}

#[test]
fn store_is_private() {
    let (root, mut e) = setup("perms");
    let be = DirectFsBackend::new(&root).unwrap();
    std::fs::write(root.join("a.txt"), b"1").unwrap();
    go(&mut e, &be, "delete a.txt");
    let mode = |p: &str| std::fs::metadata(root.join(p)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(".local/share/pawline"), 0o700);
    assert_eq!(mode(".local/share/pawline/trash"), 0o700);
    assert_eq!(mode(".local/share/pawline/journal.jsonl"), 0o600);
}

#[test]
fn recovery_settles_pending_intents_from_disk_state() {
    let (root, _e) = setup("recover");
    let store = root.join(".local/share/pawline");
    std::fs::create_dir_all(store.join("trash")).unwrap();
    // 1: create_dir that did happen. 2: create_dir that did not.
    // 3: rename that completed (b exists, a gone). 4: delete that never moved (a exists, no trash copy).
    std::fs::create_dir(root.join("made")).unwrap();
    std::fs::write(root.join("new.txt"), b"").unwrap();
    std::fs::write(root.join("stay.txt"), b"s").unwrap();
    let lines = [
        r#"{"intent":{"id":1,"kind":"create_dir","a":"made","b":null}}"#,
        r#"{"intent":{"id":2,"kind":"create_dir","a":"ghost","b":null}}"#,
        r#"{"intent":{"id":3,"kind":"rename","a":"old.txt","b":"new.txt"}}"#,
        r#"{"intent":{"id":4,"kind":"delete","a":"stay.txt","b":".local/share/pawline/trash/4-stay.txt"}}"#,
    ];
    std::fs::write(store.join("journal.jsonl"), lines.join("\n") + "\n").unwrap();
    let be = DirectFsBackend::new(&root).unwrap();
    assert_eq!(be.recovery_notes().len(), 4, "{:?}", be.recovery_notes());
    // the completed rename is now the last undoable entry
    let mut e = Engine::new(root.clone());
    let plan = e.plan("undo").unwrap();
    assert!(plan.summary().contains("put ~/new.txt back at ~/old.txt"), "{}", plan.summary());
    // a recovered move has no fingerprint, so undo cannot verify the file is unchanged: it fails closed
    let r = e.execute_plan(plan, &be).unwrap();
    assert!(matches!(r, Response::Failed { ref error, .. } if error.to_string().contains("can't verify")));
    assert!(root.join("new.txt").exists() && !root.join("old.txt").exists());
    // a second start finds nothing pending
    assert!(DirectFsBackend::new(&root).unwrap().recovery_notes().is_empty());
}

#[test]
fn create_file_undo_refuses_when_touched_even_if_truncated_back_to_empty() {
    let (root, mut e) = setup("touch");
    let be = DirectFsBackend::new(&root).unwrap();
    go(&mut e, &be, "make a file called note.txt");
    // same path, same inode, empty again, but it was written in between
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    std::fs::write(root.join("note.txt"), b"").unwrap();
    assert!(matches!(go(&mut e, &be, "undo"), Response::Failed { .. }));
    assert!(root.join("note.txt").exists());
}

#[test]
fn create_file_undo_works_when_untouched() {
    let (root, mut e) = setup("untouched");
    let be = DirectFsBackend::new(&root).unwrap();
    go(&mut e, &be, "make a file called note.txt");
    assert!(matches!(go(&mut e, &be, "undo"), Response::Executed(_)));
    assert!(!root.join("note.txt").exists());
}

#[test]
fn copy_is_journaled_and_undo_trashes_the_copy_only() {
    let (root, mut e) = setup("copy");
    let be = DirectFsBackend::new(&root).unwrap();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::create_dir(root.join("backup")).unwrap();
    std::fs::write(root.join("docs/a.txt"), b"1").unwrap();
    go(&mut e, &be, "copy docs to backup");
    assert!(root.join("backup/docs/a.txt").exists());
    assert!(matches!(go(&mut e, &be, "undo"), Response::Executed(_)));
    assert!(!root.join("backup/docs").exists());
    assert_eq!(std::fs::read(root.join("docs/a.txt")).unwrap(), b"1");
}

struct Fake { current: Option<PathBuf>, log: Arc<Mutex<Vec<PathBuf>>> }
impl WallpaperSetter for Fake {
    fn set(&self, image: &Path, _v: Option<Adapter>) -> Result<String, PawError> {
        self.log.lock().unwrap().push(image.to_path_buf());
        Ok("ok".into())
    }
    fn current(&self) -> Option<PathBuf> { self.current.clone() }
}

#[test]
fn first_wallpaper_change_captures_baseline_so_undo_restores_it() {
    let (root, mut e) = setup("base");
    let outside = root.join("Pictures");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("old.png"), b"OLD").unwrap();
    std::fs::write(outside.join("new.png"), b"NEW").unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let be = DirectFsBackend::new(&root).unwrap().with_wallpaper(Box::new(Fake {
        current: Some(outside.join("old.png")),
        log: log.clone(),
    }));
    go(&mut e, &be, "set wallpaper to ~/Pictures/new.png anyway");
    assert!(matches!(go(&mut e, &be, "undo"), Response::Executed(_)));
    let l = log.lock().unwrap().clone();
    assert_eq!(l.len(), 2);
    assert_eq!(std::fs::read(&l[1]).unwrap(), b"OLD");
    // user's original file untouched
    assert_eq!(std::fs::read(outside.join("old.png")).unwrap(), b"OLD");
}

/// Restart: a brand-new Engine and backend over the same home (the app was closed and reopened).
#[test]
fn undo_survives_an_application_restart_for_delete_and_create_dir() {
    let (root, mut e) = setup("restart");
    let be = DirectFsBackend::new(&root).unwrap();
    std::fs::write(root.join("a.txt"), b"alpha").unwrap();
    std::fs::write(root.join("b.txt"), b"beta").unwrap();
    assert!(matches!(go(&mut e, &be, "delete a.txt"), Response::Executed(_)), "delete");
    assert!(!root.join("a.txt").exists());
    drop(e);
    drop(be);

    let mut e2 = Engine::new(root.clone());
    let be2 = DirectFsBackend::new(&root).unwrap();
    assert!(be2.recovery_notes().is_empty(), "a clean exit leaves nothing to recover");
    assert!(matches!(go(&mut e2, &be2, "undo"), Response::Executed(_)), "undo after restart");
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"alpha");

    assert!(matches!(go(&mut e2, &be2, "make a folder called pics"), Response::Executed(_)));
    drop(e2);
    drop(be2);
    let mut e3 = Engine::new(root.clone());
    let be3 = DirectFsBackend::new(&root).unwrap();
    assert!(matches!(go(&mut e3, &be3, "undo"), Response::Executed(_)));
    assert!(!root.join("pics").exists(), "empty folder created before the restart is removed by undo");
    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"beta", "unrelated file untouched");
}

/// A crash after the intent line but before the change: the next start settles it and says so.
#[test]
fn crash_after_intent_before_change_is_reported_at_next_start_and_leaves_no_partial_state() {
    let (root, _e) = setup("crashintent");
    let store = root.join(".local/share/pawline");
    std::fs::create_dir_all(&store).unwrap();
    let be0 = DirectFsBackend::new(&root).unwrap();
    drop(be0);
    let line = pawline_core::journal::Entry { id: 1, kind: "create_dir".into(), a: "half".into(), b: None, meta: None }.intent_line();
    std::fs::write(store.join("journal.jsonl"), format!("{line}\n")).unwrap();
    let be = DirectFsBackend::new(&root).unwrap();
    assert_eq!(be.recovery_notes().len(), 1, "{:?}", be.recovery_notes());
    assert!(be.recovery_notes()[0].contains("had not completed"));
    assert!(!root.join("half").exists());
    // settled once: a second start has nothing left to report
    assert!(DirectFsBackend::new(&root).unwrap().recovery_notes().is_empty());
}

/// A move into a folder that cannot be written fails: the source stays, nothing half-made is left, and the journal shows the attempt as not done.
#[test]
fn failed_move_leaves_source_in_place_no_stray_files_and_nothing_to_undo() {
    let (root, mut e) = setup("midfail");
    let be = DirectFsBackend::new(&root).unwrap();
    std::fs::write(root.join("a.txt"), b"alpha").unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let r = go(&mut e, &be, "move a.txt to locked");
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o700)).unwrap();
    match r {
        Response::Failed { error, .. } => assert!(!error.is_empty()),
        Response::Executed(_) => panic!("must not report success into a read-only folder (running as root?)"),
        _ => panic!("unexpected response variant"),
    }
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"alpha");
    assert_eq!(std::fs::read_dir(root.join("locked")).unwrap().count(), 0, "nothing half-made in the target");
    let j = std::fs::read_to_string(root.join(".local/share/pawline/journal.jsonl")).unwrap_or_default();
    assert!(pawline_core::journal::pending(&j).is_empty(), "no intent left dangling: {j}");
}
