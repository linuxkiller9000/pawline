//! Destinations named in words ("make a folder called a in models") resolve to real folders
//! (home children, then the ranked hints from zoxide or Pawline's own list), always inside home,
//! always behind a confirmation, and ask when several folders match.
use pawline_core::exec::DirectFsBackend;
use pawline_core::error::PawError;
use pawline_core::{Engine, Response};
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawline-places-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn confirm_text(r: Result<Response, PawError>) -> String {
    match r {
        Ok(Response::Confirm(p)) => format!("{}\n{}", p.summary(), p.confirmation_reasons.join("\n")),
        Ok(other) => panic!("expected a confirmation, got {:?}", std::mem::discriminant(&other)),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn in_a_hinted_folder_asks_and_shows_the_path() {
    let root = sandbox("hint");
    let models = root.join("Projects/ai/models");
    std::fs::create_dir_all(&models).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![models.clone()]);
    let b = DirectFsBackend::new(&root).unwrap();
    for cmd in ["make a folder called gguf in models", "make a folder in home container models called gguf", "make a folder called gguf in home models"] {
        let t = confirm_text(e.handle(cmd, &b, false));
        assert!(t.contains(&models.join("gguf").display().to_string()), "{cmd}: {t}");
        assert!(t.contains("Check the path"), "{cmd}: {t}");
    }
    assert!(!root.join("gguf").exists() && !models.join("gguf").exists(), "nothing is made before a yes");
}

#[test]
fn several_matches_ask_instead_of_choosing() {
    let root = sandbox("many");
    let (a, b) = (root.join("a/models"), root.join("b/models"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![a.clone(), b.clone()]);
    let err = e.handle("make a folder called x in models", &DirectFsBackend::new(&root).unwrap(), false).err().expect("should ask").to_string();
    assert!(err.contains("several folders") && err.contains(&a.display().to_string()) && err.contains(&b.display().to_string()), "{err}");
}

#[test]
fn nothing_found_keeps_old_behaviour_and_outside_home_is_never_used() {
    let root = sandbox("none");
    let outside = sandbox("outside").join("models");
    std::fs::create_dir_all(&outside).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![outside]);
    let b = DirectFsBackend::new(&root).unwrap();
    // no match inside home: the whole text stays the name, as before ("lost in space")
    let r = e.handle("make a folder called lost in space", &b, false).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("lost in space").is_dir());
    let r = e.handle("make a folder called a in models", &b, false).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("a in models").is_dir(), "a hint outside home is ignored");
}
