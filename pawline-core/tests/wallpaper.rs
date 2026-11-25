use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pawline_core::error::PawError;
use pawline_core::exec::DirectFsBackend;
use pawline_core::wallpaper::WallpaperSetter;
use pawline_core::{Engine, Response};

struct Rec(Arc<Mutex<Vec<PathBuf>>>);
impl WallpaperSetter for Rec {
    fn set(&self, image: &Path, _via: Option<pawline_core::wallpaper::Adapter>) -> Result<String, PawError> {
        self.0.lock().unwrap().push(image.to_path_buf());
        Ok("recorded".into())
    }
}

fn setup() -> (PathBuf, Engine, DirectFsBackend, Arc<Mutex<Vec<PathBuf>>>) {
    let root = std::env::temp_dir().join(format!("pawline-wp-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Pictures")).unwrap();
    std::fs::write(root.join("Pictures/sunset.png"), b"x").unwrap();
    std::fs::write(root.join("Pictures/run.sh"), b"x").unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let be = DirectFsBackend::new(&root).unwrap().with_wallpaper(Box::new(Rec(log.clone())));
    (root.clone(), Engine::new(root), be, log)
}

#[test]
fn wallpaper_always_needs_confirmation_and_does_nothing_before() {
    let (root, mut e, be, log) = setup();
    for phrase in [
        "set wallpaper to ~/Pictures/sunset.png anyway",
        "Change my background to Pictures/sunset.png anyway",
        "set ~/Pictures/sunset.png as my wallpaper anyway",
    ] {
        match e.handle(phrase, &be, false).unwrap() {
            Response::Confirm(p) => {
                assert!(p.summary().contains(&format!("{}/Pictures/sunset.png", root.display())), "{phrase}");
            }
            _ => panic!("expected confirm for {phrase}"),
        }
    }
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn confirmed_wallpaper_runs_once_with_canonical_path() {
    let (root, mut e, be, log) = setup();
    let plan = e.plan("set wallpaper to ~/Pictures/sunset.png anyway").unwrap();
    assert!(plan.needs_confirmation);
    match e.execute_plan(plan, &be).unwrap() {
        Response::Executed(o) => assert_eq!(o.len(), 1),
        _ => panic!("expected executed"),
    }
    // the tool is handed Pawline's private copy, not the user's path
    let got = log.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert!(got[0].starts_with(root.join(".local/share/pawline/wallpapers")), "{:?}", got[0]);
    assert_eq!(std::fs::read(&got[0]).unwrap(), b"x");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&got[0]).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn swapping_the_original_after_the_copy_cannot_change_what_the_tool_sees() {
    let (root, mut e, be, log) = setup();
    let plan = e.plan("set wallpaper to ~/Pictures/sunset.png anyway").unwrap();
    e.execute_plan(plan, &be).unwrap();
    std::fs::write(root.join("Pictures/sunset.png"), b"evil").unwrap();
    let got = log.lock().unwrap().clone();
    assert_eq!(std::fs::read(&got[0]).unwrap(), b"x");
}

#[test]
fn undo_returns_to_previous_pawline_wallpaper_and_refuses_without_one() {
    let (root, mut e, be, log) = setup();
    std::fs::write(root.join("Pictures/two.jpg"), b"y").unwrap();
    let run = |e: &mut Engine, s: &str| {
        let p = e.plan(s).unwrap();
        e.execute_plan(p, &be).unwrap()
    };
    run(&mut e, "set wallpaper to ~/Pictures/sunset.png anyway");
    // only one change so far: nothing earlier to return to
    assert!(matches!(run(&mut e, "undo"), Response::Failed { .. }));
    run(&mut e, "set wallpaper to ~/Pictures/two.jpg");
    assert!(matches!(run(&mut e, "undo"), Response::Executed(_)));
    let l = log.lock().unwrap().clone();
    assert_eq!(l.len(), 3);
    assert_eq!(std::fs::read(&l[2]).unwrap(), b"x");
    assert_eq!(l[2], l[0]);
}

#[test]
fn using_clause_selects_adapter_and_unknown_names_are_refused() {
    let (_root, e, _be, _log) = setup();
    let p = e.plan("set wallpaper to ~/Pictures/sunset.png using awww anyway").unwrap();
    assert!(p.summary().contains("using awww"), "{}", p.summary());
    assert!(e.plan("set wallpaper to ~/Pictures/sunset.png using feh").is_err());
}

#[test]
fn refuses_non_images_missing_files_and_outside_home() {
    let (_root, e, _be, _log) = setup();
    assert!(e.plan("set wallpaper to ~/Pictures/run.sh").is_err());
    assert!(e.plan("set wallpaper to ~/Pictures/missing.png").is_err());
    assert!(e.plan("set wallpaper to /etc/passwd").is_err());
    assert!(e.plan("set wallpaper to ~/Pictures").is_err());
    assert!(e.plan("set wallpaper to").is_err());
}

#[test]
fn symlink_out_of_home_is_refused() {
    let (root, e, _be, _log) = setup();
    std::os::unix::fs::symlink("/etc/passwd", root.join("Pictures/evil.png")).unwrap();
    assert!(e.plan("set wallpaper to ~/Pictures/evil.png").is_err());
}

#[test]
fn execution_rechecks_file_after_plan_swapped_to_dir() {
    let (root, mut e, be, log) = setup();
    let plan = e.plan("set wallpaper to ~/Pictures/sunset.png anyway").unwrap();
    std::fs::remove_file(root.join("Pictures/sunset.png")).unwrap();
    std::fs::create_dir(root.join("Pictures/sunset.png")).unwrap();
    match e.execute_plan(plan, &be).unwrap() {
        Response::Failed { .. } => {}
        _ => panic!("expected failure"),
    }
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn without_a_known_previous_wallpaper_plain_request_is_refused_and_changes_nothing() {
    let (root, mut e, be, log) = setup();
    if pawline_core::wallpaper::current_swaybg_image().is_some() {
        return; // a real swaybg on the test machine makes the previous wallpaper knowable
    }
    let err = e.plan("set wallpaper to ~/Pictures/sunset.png").unwrap_err().to_string();
    assert!(err.contains("Nothing was changed") && err.contains("anyway"), "{err}");
    assert!(e.handle("set wallpaper to ~/Pictures/sunset.png", &be, true).is_err());
    assert!(log.lock().unwrap().is_empty());
    assert!(!root.join(".local/share/pawline/wallpapers").exists());
    // with the explicit acknowledgment the card carries the limitation
    let p = e.plan("set wallpaper to ~/Pictures/sunset.png anyway").unwrap();
    assert!(p.summary().contains("without a backup"), "{}", p.summary());
}

#[test]
fn second_change_needs_no_acknowledgment() {
    let (root, mut e, be, _log) = setup();
    std::fs::write(root.join("Pictures/two.jpg"), b"y").unwrap();
    let p = e.plan("set wallpaper to ~/Pictures/sunset.png anyway").unwrap();
    e.execute_plan(p, &be).unwrap();
    let p = e.plan("set wallpaper to ~/Pictures/two.jpg").unwrap();
    assert!(!p.summary().contains("without a backup"));
}
