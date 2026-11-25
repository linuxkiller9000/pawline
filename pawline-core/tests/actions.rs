use pawline_core::actions::{self, Meta};
use pawline_core::exec::DirectFsBackend;
use pawline_core::{Engine, Response};
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawline-act-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(e: &mut Engine, root: &PathBuf, cmd: &str) -> Response {
    e.handle(cmd, &DirectFsBackend::new(root).unwrap(), false).unwrap()
}

fn save(e: &Engine, root: &PathBuf, name: &str) -> Result<(), String> {
    let b = DirectFsBackend::new(root).unwrap();
    let c = e.save_candidate().cloned().ok_or("nothing to save")?;
    let mut all = actions::parse_file(&b.read_store_text(actions::ACTIONS).unwrap());
    actions::add(&mut all, name, &c).map_err(|x| x.to_string())?;
    b.write_store_text(actions::ACTIONS, &actions::render_file(&all)).unwrap();
    Ok(())
}

fn load(root: &PathBuf) -> Vec<actions::Saved> {
    let b = DirectFsBackend::new(root).unwrap();
    actions::parse_file(&b.read_store_text(actions::ACTIONS).unwrap())
}

#[test]
fn save_then_run_needs_fresh_confirmation_and_reruns_exactly() {
    let root = sandbox("rt");
    let mut e = Engine::new(root.clone());
    assert!(matches!(run(&mut e, &root, "make a folder called kit"), Response::Executed(_)));
    save(&e, &root, "mykit").unwrap();
    // the folder is removed, then the action is run in a new session
    std::fs::remove_dir(root.join("kit")).unwrap();
    let mut e2 = Engine::new(root.clone());
    let saved = load(&root).remove(0);
    let plan = e2.plan_saved(&saved).unwrap();
    assert!(plan.needs_confirmation, "a run always needs confirmation");
    assert!(!plan.assist_derived);
    assert!(!root.join("kit").exists(), "planning alone changes nothing");
    let b = DirectFsBackend::new(&root).unwrap();
    assert!(matches!(e2.execute_plan(plan, &b).unwrap(), Response::Executed(_)));
    assert!(root.join("kit").is_dir());
}

#[test]
fn drift_refuses_and_shows_both_plans() {
    let root = sandbox("drift");
    let mut e = Engine::new(root.clone());
    run(&mut e, &root, "make a folder called aa");
    save(&e, &root, "aa").unwrap();
    let mut saved = load(&root).remove(0);
    // same command, but the steps recorded no longer match what it resolves to
    saved.steps = vec!["Create folder /somewhere/else/aa".into()];
    std::fs::remove_dir(root.join("aa")).unwrap();
    let err = Engine::new(root.clone()).plan_saved(&saved).unwrap_err().to_string();
    assert!(err.contains("different"), "{err}");
    assert!(err.contains("/somewhere/else/aa") && err.contains("aa"), "{err}");
}

#[test]
fn run_never_uses_the_assist() {
    // A saved command the fixed parser cannot parse fails; there is no assist hook.
    let root = sandbox("noassist");
    let saved = actions::Saved {
        name: "x".into(),
        command: "could you please set up a new directory named pets for me".into(),
        steps: vec![format!("Create folder {}/pets", root.display())],
    };
    assert!(Engine::new(root).plan_saved(&saved).is_err());
}

#[test]
fn only_a_command_that_fully_ran_can_be_saved() {
    let root = sandbox("cand");
    let mut e = Engine::new(root.clone());
    assert!(e.save_candidate().is_none());
    run(&mut e, &root, "make a folder called one");
    assert!(e.save_candidate().is_some());
    // a failing next command clears the candidate: it cannot save "one" by mistake
    assert!(e.handle("make a folder called one", &DirectFsBackend::new(&root).unwrap(), false).is_err());
    assert!(e.save_candidate().is_none());
    // a refused or unparseable one clears it too
    run(&mut e, &root, "make a folder called two");
    assert!(e.handle("gibberish words", &DirectFsBackend::new(&root).unwrap(), false).is_err());
    assert!(e.save_candidate().is_none());
}

#[test]
fn delete_move_and_session_bound_commands_are_not_saveable() {
    let root = sandbox("notsave");
    let mut e = Engine::new(root.clone());
    run(&mut e, &root, "make a folder called d1");
    // "it" depends on the session
    run(&mut e, &root, "inside it create a folder called inner");
    assert!(e.save_candidate().is_none(), "{:?}", e.save_candidate());
    // delete needs confirmation, never saveable
    let r = e.handle("delete d1", &DirectFsBackend::new(&root).unwrap(), false).unwrap();
    assert!(matches!(r, Response::Confirm(_)));
    assert!(e.save_candidate().is_none());
    run(&mut e, &root, "make a folder called d2");
    assert!(e.save_candidate().is_some());
}

#[test]
fn no_overwrite_and_store_is_private() {
    let root = sandbox("priv");
    let mut e = Engine::new(root.clone());
    run(&mut e, &root, "make a folder called p1");
    save(&e, &root, "p").unwrap();
    assert!(save(&e, &root, "P").is_err());
    use std::os::unix::fs::PermissionsExt;
    let m = std::fs::metadata(root.join(actions::ACTIONS)).unwrap().permissions().mode() & 0o777;
    assert_eq!(m, 0o600);
    assert_eq!(actions::parse_meta("list saved actions"), Some(Meta::List));
}

#[test]
fn copy_template_revalidates_paths_not_content() {
    // A saved copy is a fixed template: same source PATH and destination PATH.
    // If the source file's content changed since saving, the steps are the
    // same and the run is allowed (after confirmation); it copies what is
    // there now. If the source is gone or the target exists, validation fails.
    let root = sandbox("copy");
    std::fs::write(root.join("src.txt"), "v1").unwrap();
    std::fs::create_dir(root.join("bak")).unwrap();
    let mut e = Engine::new(root.clone());
    let r = run(&mut e, &root, &format!("copy {}/src.txt to {}/bak", root.display(), root.display()));
    assert!(matches!(r, Response::Executed(_)), "copy should run");
    save(&e, &root, "cp").unwrap();
    let saved = load(&root).remove(0);
    std::fs::remove_file(root.join("bak/src.txt")).unwrap();
    std::fs::write(root.join("src.txt"), "v2 changed since save").unwrap();
    let plan = Engine::new(root.clone()).plan_saved(&saved).unwrap();
    assert!(plan.needs_confirmation);
    let b = DirectFsBackend::new(&root).unwrap();
    Engine::new(root.clone()).execute_plan(plan, &b).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("bak/src.txt")).unwrap(), "v2 changed since save");
    // source removed: refused by validation, nothing changes
    std::fs::remove_file(root.join("bak/src.txt")).unwrap();
    std::fs::remove_file(root.join("src.txt")).unwrap();
    assert!(Engine::new(root.clone()).plan_saved(&saved).is_err());
    // source replaced by a symlink pointing outside the root: refused
    let outside = sandbox("copy-out");
    std::fs::write(outside.join("secret"), "x").unwrap();
    std::os::unix::fs::symlink(outside.join("secret"), root.join("src.txt")).unwrap();
    assert!(Engine::new(root.clone()).plan_saved(&saved).is_err());
    assert!(!root.join("bak/src.txt").exists());
}

#[test]
fn symlinked_store_cannot_escape_the_root() {
    use std::os::unix::fs::symlink;
    let root = sandbox("lnk");
    let outside = sandbox("lnk-out");
    let b = DirectFsBackend::new(&root).unwrap();
    let mut e = Engine::new(root.clone());
    run(&mut e, &root, "make a folder called s1");
    save(&e, &root, "first").unwrap();
    // 1. actions.tsv replaced by a symlink to an outside file: not read, not written through
    std::fs::write(outside.join("evil.tsv"), "evil\tmake a folder called evil\tCreate folder /x\n").unwrap();
    std::fs::remove_file(root.join(actions::ACTIONS)).unwrap();
    symlink(outside.join("evil.tsv"), root.join(actions::ACTIONS)).unwrap();
    assert!(b.read_store_text(actions::ACTIONS).is_err() || b.read_store_text(actions::ACTIONS).unwrap().is_empty());
    let before = std::fs::read_to_string(outside.join("evil.tsv")).unwrap();
    let _ = b.write_store_text(actions::ACTIONS, "rewritten\n");
    assert_eq!(std::fs::read_to_string(outside.join("evil.tsv")).unwrap(), before, "outside file untouched");
    // 2. a pre-planted symlink at the temp name is replaced, not followed
    std::fs::remove_file(root.join(actions::ACTIONS)).ok();
    let tmp = format!("{}.tmp", actions::ACTIONS);
    symlink(outside.join("tmp-target"), root.join(&tmp)).unwrap();
    b.write_store_text(actions::ACTIONS, "ok\tmake a folder called a\tCreate folder /a\n").unwrap();
    assert!(!outside.join("tmp-target").exists(), "temp symlink was not followed");
    // 3. the store directory itself replaced by a symlink to outside
    let root2 = sandbox("lnk2");
    std::fs::create_dir_all(root2.join(".local/share")).unwrap();
    std::fs::create_dir_all(outside.join("pl")).unwrap();
    symlink(outside.join("pl"), root2.join(".local/share/pawline")).unwrap();
    let b2 = DirectFsBackend::new(&root2).unwrap();
    let _ = b2.write_store_text(actions::ACTIONS, "x\ty\tz\n");
    assert!(!outside.join("pl/actions.tsv").exists(), "store symlink was not followed out of the root");
}
