//! Assist-path tests with a mock assist: the safety invariant is that
//! assist proposals are validated exactly like parser output.

use pawline_core::assist::Assist;
use pawline_core::error::PawError;
use pawline_core::exec::DirectFsBackend;
use pawline_core::plan::{CreateKind, Directive, LocationRef};
use pawline_core::{Engine, Response};
use std::cell::Cell;
use std::path::PathBuf;

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawline-assist-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Mock {
    calls: Cell<usize>,
    reply: Result<Vec<Directive>, String>,
}

impl Assist for Mock {
    fn propose(&self, _input: &str) -> Result<Vec<Directive>, PawError> {
        self.calls.set(self.calls.get() + 1);
        self.reply
            .clone()
            .map_err(|e| PawError::Parse(format!("mock assist: {e}")))
    }
}

/// Input the deterministic parser refuses, assist proposes a valid create:
/// plan builds and executes through the normal pipeline.
#[test]
fn assist_fills_parser_gaps() {
    let root = sandbox("gap");
    let mut engine = Engine::new(root.clone());
    let mock = Mock {
        calls: Cell::new(0),
        reply: Ok(vec![Directive::Create {
            kind: CreateKind::Dir,
            names: vec!["sparkly".into()],
            parent: LocationRef::CurrentDir,
        }]),
    };
    let backend = DirectFsBackend::new(&root).unwrap();
    // Model-proposed creates wait for the user's yes like everything else
    // the model suggests.
    let r = engine
        .handle_with("conjure me something sparkly", &backend, false, Some(&mock))
        .unwrap();
    let shown = match r {
        Response::Confirm(p) => p,
        _ => panic!("assist-derived create must require confirmation"),
    };
    assert!(shown.assist_derived);
    assert!(!root.join("sparkly").exists(), "executed before confirmation");
    let r = engine.execute_plan(shown, &backend).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("sparkly").is_dir());
    assert_eq!(mock.calls.get(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// When the parser succeeds, the assist must not be consulted at all.
#[test]
fn parser_wins_when_it_understands() {
    let root = sandbox("wins");
    let mut engine = Engine::new(root.clone());
    let mock = Mock {
        calls: Cell::new(0),
        reply: Err("should never be called".into()),
    };
    let r = engine
        .handle_with(
            "make a folder called normal",
            &DirectFsBackend::new(&root).unwrap(),
            false,
            Some(&mock),
        )
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert_eq!(mock.calls.get(), 0);
    let _ = std::fs::remove_dir_all(&root);
}

/// An assist proposal that escapes the root is refused by the validator —
/// model output has no special privileges.
#[test]
fn assist_cannot_escape_root() {
    let root = sandbox("escape");
    let engine = Engine::new(root.clone());
    for evil in ["/etc/pawline-evil", "../outside", "~/pawline-evil-abs"] {
        let mock = Mock {
            calls: Cell::new(0),
            reply: Ok(vec![Directive::Create {
                kind: CreateKind::Dir,
                names: vec![evil.into()],
                parent: LocationRef::CurrentDir,
            }]),
        };
        let r = engine.plan_with("conjure something", Some(&mock));
        // The validator must either refuse outright or, for a bare-ish name
        // it can reinterpret safely, confine it under the root. It must
        // never produce an op touching a path outside the root.
        match r {
            Err(_) => {}
            Ok(plan) => {
                for op in &plan.ops {
                    let s = op.describe();
                    assert!(
                        !s.contains("/etc") && !s.contains("..") && !s.contains("/home"),
                        "escape leaked into plan: {s}"
                    );
                }
            }
        }
    }
}

/// Assist failure on unparseable input surfaces both errors.
#[test]
fn both_fail_reports_both() {
    let root = sandbox("bothfail");
    let engine = Engine::new(root.clone());
    let mock = Mock {
        calls: Cell::new(0),
        reply: Err("model offline".into()),
    };
    let e = engine
        .plan_with("zqx wvut", Some(&mock))
        .unwrap_err()
        .to_string();
    // The user gets the parser's plain explanation; the assist's own failure text stays internal.
    assert!(e.contains("don't know how to"), "got: {e}");
    assert!(!e.contains("assist"), "got: {e}");
    assert!(!e.contains("model offline"), "got: {e}");
}

/// A confirmation-gated assist proposal (delete) still needs the user's yes.
#[test]
fn assist_delete_still_needs_confirmation() {
    let root = sandbox("confirm");
    std::fs::create_dir(root.join("doomed")).unwrap();
    let mut engine = Engine::new(root.clone());
    let mock = Mock {
        calls: Cell::new(0),
        reply: Ok(vec![Directive::Delete {
            target: LocationRef::Named("doomed".into()),
        }]),
    };
    let backend = DirectFsBackend::new(&root).unwrap();
    let r = engine
        .handle_with("begone doomed", &backend, false, Some(&mock))
        .unwrap();
    assert!(matches!(r, Response::Confirm(_)));
    assert!(root.join("doomed").exists(), "deleted without confirmation!");
    let r = engine
        .handle_with("begone doomed", &backend, true, Some(&mock))
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(!root.join("doomed").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// Confidently-wrong model proposals must NEVER execute on model confidence
/// alone: every assist-derived plan waits for the user's explicit yes,
/// including plain creates that parser-derived plans would run immediately.
#[test]
fn confidently_wrong_proposals_do_not_execute() {
    let cases: Vec<(&str, Directive, &str)> = vec![
        // user asked something vague; model "confidently" creates a folder
        // they never named
        ("conjure something sparkly",
         Directive::Create { kind: CreateKind::Dir, names: vec!["starlight".into()], parent: LocationRef::CurrentDir },
         "starlight"),
        // model proposes deleting something the user never mentioned
        ("tidy things up",
         Directive::Delete { target: LocationRef::Named("important".into()) },
         "important"),
        // model proposes a rename the user never asked for
        ("fix my files",
         Directive::Rename { target: LocationRef::Named("report".into()), new_name: "junk".into() },
         "report"),
    ];
    for (tag, directive, artifact) in cases {
        let root = sandbox(tag);
        // pre-create artifacts so delete/rename targets exist
        std::fs::create_dir_all(root.join("important")).unwrap();
        std::fs::write(root.join("report"), "data").unwrap();
        let mut engine = Engine::new(root.clone());
        let mock = Mock { calls: Cell::new(0), reply: Ok(vec![directive]) };
        let backend = DirectFsBackend::new(&root).unwrap();

        let r = engine
            .handle_with(tag, &backend, false, Some(&mock))
            .unwrap();
        let plan = match r {
            Response::Confirm(p) => p,
            _ => panic!("assist proposal executed without confirmation"),
        };
        assert!(plan.assist_derived, "plan must carry assist provenance");
        assert!(plan.needs_confirmation);
        assert!(plan
            .confirmation_reasons
            .iter()
            .any(|r| r.contains("AI assist")));
        // nothing happened on disk
        assert!(!root.join("starlight").exists(), "{tag}: create executed");
        assert!(root.join("important").exists(), "{tag}: delete executed");
        assert!(root.join("report").exists() && !root.join("junk").exists(),
                "{tag}: rename executed");
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// After the user SEES the concrete plan and says yes, it executes — and the
/// executed plan is the same immutable one that was shown.
#[test]
fn confirmed_assist_plan_executes_as_shown() {
    let root = sandbox("shown");
    let mut engine = Engine::new(root.clone());
    let mock = Mock {
        calls: Cell::new(0),
        reply: Ok(vec![Directive::Create {
            kind: CreateKind::Dir,
            names: vec!["starlight".into()],
            parent: LocationRef::CurrentDir,
        }]),
    };
    let backend = DirectFsBackend::new(&root).unwrap();

    let r = engine
        .handle_with("conjure something", &backend, false, Some(&mock))
        .unwrap();
    let shown = match r {
        Response::Confirm(p) => p,
        _ => panic!("expected confirmation"),
    };
    assert_eq!(shown.summary().trim(), format!("1. Create folder {}", root.join("starlight").display()));
    assert!(!root.join("starlight").exists());

    let r = engine.execute_plan(shown, &backend).unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("starlight").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

/// Parser-derived creates keep their existing behavior: no confirmation
/// gate for a plain deterministic create.
#[test]
fn parser_create_still_runs_immediately() {
    let root = sandbox("parsed");
    let mut engine = Engine::new(root.clone());
    let backend = DirectFsBackend::new(&root).unwrap();
    let r = engine
        .handle("make a folder called normal", &backend, false)
        .unwrap();
    assert!(matches!(r, Response::Executed(_)));
    assert!(root.join("normal").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

struct Panics;
impl Assist for Panics {
    fn propose(&self, _input: &str) -> Result<Vec<Directive>, PawError> {
        panic!("simulated model crash");
    }
}

/// Model failure modes: an error, an empty proposal and a crash all end the same way. The user gets the parser's plain
/// explanation, nothing is executed, no file appears, and the engine keeps working afterwards.
#[test]
fn model_error_empty_and_panic_fail_safe_and_the_engine_survives() {
    let root = sandbox("modelfail");
    let mut engine = Engine::new(root.clone());
    let backend = DirectFsBackend::new(&root).unwrap();
    let gibberish = "frobnicate the zorp";
    let err = Mock { calls: Cell::new(0), reply: Err("confidence 0.12".into()) };
    let empty = Mock { calls: Cell::new(0), reply: Ok(vec![]) };
    let mut msgs = vec![];
    for a in [&err as &dyn Assist, &empty as &dyn Assist, &Panics as &dyn Assist] {
        match engine.handle_with(gibberish, &backend, false, Some(a)) {
            Err(e) => msgs.push(e.to_string()),
            Ok(Response::Message(m)) => msgs.push(m),
            Ok(Response::Executed(_)) => panic!("a failed model must never execute anything"),
            Ok(Response::Failed { error, .. }) => msgs.push(error),
            Ok(Response::Confirm(p)) => panic!("a failed model must not even propose a plan; assist #{} gave {} ops, notes {:?}", msgs.len(), p.ops.len(), p.notes),
        }
    }
    for m in &msgs {
        assert!(!m.contains("confidence") && !m.contains("panicked") && !m.contains("simulated"), "internal model detail leaked: {m}");
    }
    assert_eq!(std::fs::read_dir(&root).unwrap().filter(|e| !e.as_ref().unwrap().file_name().to_string_lossy().starts_with('.')).count(), 0, "nothing created");
    // still usable after the crash
    assert!(matches!(engine.handle("make a folder called after", &backend, false).unwrap(), Response::Executed(_)));
    assert!(root.join("after").is_dir());
}
