use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pawline_core::error::PawError;
use pawline_core::exec::DirectFsBackend;
use pawline_core::wallpaper::WallpaperSetter;
use pawline_core::{Engine, Response};

struct Rec(Arc<Mutex<Vec<PathBuf>>>);
impl WallpaperSetter for Rec {
    fn set(&self, image: &Path) -> Result<String, PawError> {
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
        "set wallpaper to ~/Pictures/sunset.png",
        "Change my background to Pictures/sunset.png",
        "set ~/Pictures/sunset.png as my wallpaper",
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
    let plan = e.plan("set wallpaper to ~/Pictures/sunset.png").unwrap();
    assert!(plan.needs_confirmation);
    match e.execute_plan(plan, &be).unwrap() {
        Response::Executed(o) => assert_eq!(o.len(), 1),
        _ => panic!("expected executed"),
    }
    assert_eq!(*log.lock().unwrap(), vec![root.join("Pictures/sunset.png")]);
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
    let plan = e.plan("set wallpaper to ~/Pictures/sunset.png").unwrap();
    std::fs::remove_file(root.join("Pictures/sunset.png")).unwrap();
    std::fs::create_dir(root.join("Pictures/sunset.png")).unwrap();
    match e.execute_plan(plan, &be).unwrap() {
        Response::Failed { .. } => {}
        _ => panic!("expected failure"),
    }
    assert!(log.lock().unwrap().is_empty());
}
