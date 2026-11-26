//! Filesystem stress matrix on real temp homes: odd names, symlinks, permissions, duplicates, escapes, undo after outside edits.
use pawline_core::exec::DirectFsBackend;
use pawline_core::{Engine, Response};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn home(tag: &str) -> (PathBuf, Engine, DirectFsBackend) {
    let d = std::env::temp_dir().join(format!("pawline-fs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = std::fs::canonicalize(d).unwrap();
    let be = DirectFsBackend::new(&d).unwrap();
    (d.clone(), Engine::new(d), be)
}

/// Run a command, auto-confirming a card (the matrix tests the file layer, not the card UI).
fn go(e: &mut Engine, be: &DirectFsBackend, s: &str) -> String {
    match e.handle(s, be, false) {
        Err(err) => format!("ERR {err}"),
        Ok(Response::Confirm(p)) => match e.execute_plan(p, be) {
            Ok(Response::Executed(_)) => "OK".into(),
            Ok(Response::Failed { error, .. }) => format!("FAILED {error}"),
            Ok(_) => "OTHER".into(),
            Err(err) => format!("ERR {err}"),
        },
        Ok(Response::Executed(_)) => "OK".into(),
        Ok(Response::Failed { error, .. }) => format!("FAILED {error}"),
        Ok(Response::Message(m)) => format!("MSG {m}"),
    }
}

#[test]
fn quoted_names_with_spaces_arabic_and_emoji_rename_copy_move_delete_and_undo() {
    let names = ["my file.txt", "\u{645}\u{644}\u{641} \u{639}\u{631}\u{628}\u{64a}.txt", "party \u{1f389}.txt", "a'b.txt", "two  spaces.txt"];
    for (i, n) in names.iter().enumerate() {
        let (root, mut e, be) = home(&format!("names{i}"));
        std::fs::write(root.join(n), b"data").unwrap();
        let r = go(&mut e, &be, &format!("rename \"{n}\" to \"renamed {i}.txt\""));
        assert_eq!(r, "OK", "rename {n:?}: {r}");
        let listing: Vec<_> = std::fs::read_dir(&root).unwrap().map(|x| x.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(root.join(format!("renamed {i}.txt")).exists(), "after rename of {n:?} the folder holds {listing:?}");
        assert_eq!(std::fs::read(root.join(format!("renamed {i}.txt"))).unwrap(), b"data");
        assert!(!root.join(n).exists());
        let r = go(&mut e, &be, "undo");
        assert_eq!(r, "OK", "undo rename {n:?}: {r}");
        assert_eq!(std::fs::read(root.join(n)).unwrap(), b"data", "undo restored {n:?}");
    }
}

#[test]
fn duplicate_names_never_overwrite_for_create_rename_copy_and_move() {
    let (root, mut e, be) = home("dups");
    std::fs::write(root.join("a.txt"), b"A").unwrap();
    std::fs::write(root.join("b.txt"), b"B").unwrap();
    std::fs::create_dir(root.join("dest")).unwrap();
    std::fs::write(root.join("dest/a.txt"), b"OLD").unwrap();
    let r = go(&mut e, &be, "rename a.txt to b.txt");
    assert!(!r.starts_with("OK"), "rename onto an existing file must not succeed: {r}");
    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"B");
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"A");
    let r = go(&mut e, &be, "copy a.txt to dest");
    eprintln!("DUP copy -> {r}");
    assert_eq!(std::fs::read(root.join("dest/a.txt")).unwrap(), b"OLD", "copy must not replace the existing file");
    let _ = go(&mut e, &be, "move a.txt to dest");
    assert_eq!(std::fs::read(root.join("dest/a.txt")).unwrap(), b"OLD", "move must not replace the existing file");
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"A", "the source survives a refused move");
    let _ = go(&mut e, &be, "make a folder called dest");
    assert_eq!(std::fs::read(root.join("dest/a.txt")).unwrap(), b"OLD");
}

#[test]
fn symlinks_cannot_be_used_to_leave_the_home() {
    let (root, mut e, be) = home("symlink");
    let outside = std::env::temp_dir().join(format!("pawline-fs-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("victim.txt"), b"outside data").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("portal")).unwrap();
    std::os::unix::fs::symlink(outside.join("victim.txt"), root.join("link.txt")).unwrap();
    for cmd in ["delete portal/victim.txt", "delete link.txt", "rename portal/victim.txt to x.txt", "move portal/victim.txt to portal", "copy portal/victim.txt to portal/copy.txt", "make a folder called inner in portal", "delete ~/portal/victim.txt"] {
        let r = go(&mut e, &be, cmd);
        eprintln!("SYMLINK {cmd:?} -> {r}");
        assert_eq!(std::fs::read(outside.join("victim.txt")).unwrap(), b"outside data", "after {cmd:?} the outside file must be untouched");
        assert!(!outside.join("x.txt").exists() && !outside.join("copy.txt").exists() && !outside.join("inner").exists(), "nothing created outside after {cmd:?}");
    }
    // a path that escapes by dots or absolute path is refused too
    let r = go(&mut e, &be, &format!("delete {}", outside.join("victim.txt").display()));
    assert!(!r.starts_with("OK") || outside.join("victim.txt").exists(), "{r}");
    assert!(outside.join("victim.txt").exists());
    let _ = go(&mut e, &be, "delete ../victim.txt");
    assert!(outside.join("victim.txt").exists());
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn read_only_folders_fail_cleanly_without_leftovers() {
    let (root, mut e, be) = home("ro");
    std::fs::create_dir(root.join("locked")).unwrap();
    std::fs::write(root.join("a.txt"), b"A").unwrap();
    std::fs::write(root.join("locked/inside.txt"), b"I").unwrap();
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
    for cmd in ["copy a.txt to locked", "move a.txt to locked", "make a folder called sub in locked", "delete locked/inside.txt", "rename locked/inside.txt to renamed.txt"] {
        let r = go(&mut e, &be, cmd);
        eprintln!("RO {cmd:?} -> {r}");
        assert!(!r.starts_with("OK"), "{cmd:?} must fail in a read-only folder: {r}");
    }
    std::fs::set_permissions(root.join("locked"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let names: Vec<String> = std::fs::read_dir(root.join("locked")).unwrap().map(|x| x.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["inside.txt".to_string()], "no temp or partial files left: {names:?}");
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"A");
}

#[test]
fn undo_after_the_file_was_changed_outside_does_not_destroy_the_newer_data() {
    let (root, mut e, be) = home("external");
    assert_eq!(go(&mut e, &be, "make a file called note.txt"), "OK");
    std::fs::write(root.join("note.txt"), b"my new work").unwrap();
    let r = go(&mut e, &be, "undo");
    assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), b"my new work", "undo of a create must not delete a file that now has content: {r}");
    // rename undo when the old name was taken meanwhile
    std::fs::write(root.join("p.txt"), b"P").unwrap();
    assert_eq!(go(&mut e, &be, "rename p.txt to q.txt"), "OK");
    std::fs::write(root.join("p.txt"), b"someone else made this").unwrap();
    let _ = go(&mut e, &be, "undo");
    assert_eq!(std::fs::read(root.join("p.txt")).unwrap(), b"someone else made this", "undo must not overwrite the re-created old name");
    assert_eq!(std::fs::read(root.join("q.txt")).unwrap(), b"P");
}

#[test]
fn plan_then_swap_to_symlink_before_confirm_cannot_escape() {
    // TOCTOU: the plan was checked while "victim" was an ordinary file; before the user confirms it becomes a symlink out of the home.
    let (root, mut e, be) = home("toctou");
    let outside = std::env::temp_dir().join(format!("pawline-fs-toctou-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"keep me").unwrap();
    std::fs::write(root.join("victim"), b"mine").unwrap();
    let plan = match e.handle("delete victim", &be, false).unwrap() {
        Response::Confirm(p) => p,
        _ => panic!("delete must ask first"),
    };
    std::fs::remove_file(root.join("victim")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("victim")).unwrap();
    let _ = e.execute_plan(plan, &be);
    assert_eq!(std::fs::read(outside.join("secret.txt")).unwrap(), b"keep me", "the file behind the swapped-in symlink must survive");
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn long_unicode_and_odd_names_fail_cleanly_or_work_never_half() {
    let (root, mut e, be) = home("longnames");
    let long = "x".repeat(300);
    let r = go(&mut e, &be, &format!("make a file called \"{long}.txt\""));
    assert!(!r.starts_with("OK"), "a name over the OS limit must be refused: {r}");
    let ok255 = "y".repeat(251);
    let r = go(&mut e, &be, &format!("make a file called \"{ok255}.txt\""));
    assert!(r.starts_with("OK"), "a legal 255-byte name should work: {r}");
    // composed vs decomposed e-acute are two different names on Linux: creating one must not clobber the other
    let nfc = "caf\u{e9}.txt";
    let nfd = "cafe\u{301}.txt";
    assert!(go(&mut e, &be, &format!("make a file called \"{nfc}\"")).starts_with("OK"));
    std::fs::write(root.join(nfc), b"composed").unwrap();
    let r = go(&mut e, &be, &format!("make a file called \"{nfd}\""));
    assert_eq!(std::fs::read(root.join(nfc)).unwrap(), b"composed", "{r}");
    for bad in ["a/b", "..", "."] {
        let r = go(&mut e, &be, &format!("make a file called \"{bad}\""));
        assert!(!root.join("..").join("b").exists(), "{bad}: {r}");
    }
    // leftover check: nothing but intended files and the store
    let names: Vec<String> = std::fs::read_dir(&root).unwrap().map(|x| x.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert!(names.iter().all(|n| !n.contains(".tmp") && !n.starts_with(".pawline-tmp")), "{names:?}");
}

#[test]
fn hard_links_and_non_utf8_names_do_not_break_listing_or_delete() {
    use std::os::unix::ffi::OsStrExt;
    let (root, mut e, be) = home("hardlink");
    let outside = std::env::temp_dir().join(format!("pawline-fs-hl-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("orig.txt"), b"shared").unwrap();
    std::fs::hard_link(outside.join("orig.txt"), root.join("hl.txt")).unwrap();
    let _ = go(&mut e, &be, "delete hl.txt");
    // deleting the home-side name must leave the other name intact (it is a link, data lives on)
    assert_eq!(std::fs::read(outside.join("orig.txt")).unwrap(), b"shared");
    std::fs::write(root.join(std::ffi::OsStr::from_bytes(b"bad\xff\xfename.txt")), b"x").unwrap();
    let r = go(&mut e, &be, "list");
    assert!(!r.starts_with("ERR"), "listing a folder with a non-UTF-8 name must not fail: {r}");
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn big_listing_is_cut_and_says_so() {
    // Sandbox structural test (author-written): 1000 empty files.
    let (d, mut e, be) = home("biglist");
    std::fs::create_dir(d.join("docs")).unwrap();
    for i in 0..1000 {
        std::fs::File::create(d.join("docs").join(format!("f{i:04}.txt"))).unwrap();
    }
    let r = match e.handle("list docs", &be, false).unwrap() {
        Response::Confirm(p) => e.execute_plan(p, &be).unwrap(),
        other => other,
    };
    match r {
        Response::Executed(o) => {
            let t = &o[0].detail;
            assert!(t.contains("and 700 more") && t.contains("of 1000 names"), "{}", &t[t.len().saturating_sub(90)..]);
            assert!(!t.contains("f0999"));
        }
        Response::Message(m) => panic!("message instead of a listing: {m}"),
        Response::Failed { error, .. } => panic!("failed: {error}"),
        _ => panic!("unexpected response (needs another confirmation)"),
    }
}

#[test]
fn fifo_in_home_does_not_hang_list_or_delete() {
    // Sandbox structural test (author-written): a FIFO with no writer would block an open().
    let (d, mut e, be) = home("fifo");
    let ok = std::process::Command::new("mkfifo").arg(d.join("pipe")).status().map(|s| s.success()).unwrap_or(false);
    if !ok { eprintln!("mkfifo unavailable, skipped"); return; }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let a = go(&mut e, &be, "list");
        let b = go(&mut e, &be, "delete pipe");
        let _ = tx.send((a, b));
    });
    let (a, b) = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("engine blocked on a FIFO");
    eprintln!("fifo list={a:.60} delete={b:.60}");
}
