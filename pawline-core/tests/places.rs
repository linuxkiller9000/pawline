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
    for cmd in ["make a folder called gguf in models", "make a folder called gguf in home models"] {
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
    assert!(err.contains("Several folders") && err.contains(&a.display().to_string()) && err.contains(&b.display().to_string()), "{err}");
}

#[test]
fn container_words_must_be_honored() {
    let root = sandbox("hier");
    let (a, b) = (root.join("container/models"), root.join("Projects/models"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![a.clone(), b.clone()]);
    let bk = DirectFsBackend::new(&root).unwrap();
    // "container" is a real parent of only one of them: that one, after a confirm card
    let t = confirm_text(e.handle("make a folder called x in home container models", &bk, false));
    assert!(t.contains(&a.join("x").display().to_string()), "{t}");
    // a parent that no models folder has: ask, listing the choices, never drop the word
    let err = e.handle("make a folder called x in home cages models", &bk, false).err().expect("ask").to_string();
    assert!(err.contains("cages") && err.contains(&a.display().to_string()) && err.contains(&b.display().to_string()), "{err}");
    assert!(!root.join("x").exists());
}

#[test]
fn unmatched_destination_asks_and_quotes_keep_names_literal() {
    let root = sandbox("none");
    let outside = sandbox("outside").join("models");
    std::fs::create_dir_all(&outside).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![outside]);
    let b = DirectFsBackend::new(&root).unwrap();
    let err = e.handle("make a folder called lost in space", &b, false).err().expect("ask").to_string();
    assert!(err.contains("quotes"), "{err}");
    assert!(!root.join("lost in space").exists());
    // a hint outside home is never used
    assert!(e.handle("make a folder called a in models", &b, false).is_err());
    assert!(!root.join("a in models").exists());
    // quoted: literal, as the user said
    let r = e.handle("make a folder called \"lost in space\"", &b, false).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("lost in space").is_dir());
}

#[test]
fn short_words_work_like_zoxide_keywords() {
    let root = sandbox("short");
    let (a, b) = (root.join("container/models"), root.join("Projects/models"));
    let c = root.join("container/models-old");
    for d in [&a, &b, &c] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![b.clone(), c.clone(), a.clone()]);
    let bk = DirectFsBackend::new(&root).unwrap();
    // "ho co models": "ho" is dropped as home, "co" is part of "container", the exact "models" beats "models-old"
    let t = confirm_text(e.handle("make a folder called x in ho co models", &bk, false));
    assert!(t.contains(&a.join("x").display().to_string()), "{t}");
    let t = confirm_text(e.handle("make a folder called x in co models", &bk, false));
    assert!(t.contains(&a.join("x").display().to_string()), "{t}");
    // a part-of-name last word with a single match
    let t = confirm_text(e.handle("make a folder called x in co models-o", &bk, false));
    assert!(t.contains(&c.join("x").display().to_string()), "{t}");
}

#[test]
fn a_full_path_pick_prepares_a_confirmation_with_that_path() {
    let root = sandbox("pick");
    let (a, b) = (root.join("a/models"), root.join("b/models"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut e = Engine::new(root.clone());
    e.set_dir_hints(vec![a.clone(), b.clone()]);
    let bk = DirectFsBackend::new(&root).unwrap();
    match e.handle("make a folder called x in models", &bk, false) {
        Err(PawError::Ambiguous { words, choices }) => {
            assert_eq!(words, "models");
            assert_eq!(choices.len(), 2);
        }
        _ => panic!("should ask which one"),
    }
    e.set_force_confirm(true);
    for cmd in [format!("make a folder called x in {}", b.display()), format!("make a folder in {} called x", b.display())] {
        let t = confirm_text(e.handle(&cmd, &bk, false));
        assert!(t.contains(&b.join("x").display().to_string()), "{cmd}: {t}");
    }
    assert!(!b.join("x").exists());
    // outside home: not usable
    assert!(e.handle("make a folder called x in /etc", &bk, false).is_err());
}
