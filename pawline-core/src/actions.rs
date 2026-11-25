//! Saved actions: named, user-saved commands that are re-checked and
//! re-confirmed every time they run.
//!
//! What is stored: the ORIGINAL TYPED COMMAND and the plan steps it produced
//! when it was saved (the step text, with the paths it resolved to).
//! Running re-parses the stored command with the deterministic parser only
//! (the AI assist is never consulted), re-validates it against the disk as
//! it is now, and compares the resulting steps with the saved ones. If they
//! differ in any way the run is refused and both plans are shown. If they
//! match, the plan is shown and the usual explicit confirmation is required
//! every time. Nothing runs automatically, on a schedule, or from history.
//!
//! Only plans made of create-folder, create-file and copy steps can be
//! saved for now. Commands that depend on the session ("it", "that folder",
//! a bare folder name) cannot be saved; use a full path.

use crate::error::PawError;
use crate::plan::{Directive, LocationRef, Operation};

pub const ACTIONS: &str = ".local/share/pawline/actions.tsv";
pub const MAX_NAME: usize = 40;
pub const MAX_ACTIONS: usize = 200;
const SEP: char = '\x1f';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub command: String,
    pub steps: Vec<String>,
}

/// What `Engine::handle_with` remembers about the last command that ran,
/// so "save that as NAME" has something exact to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub command: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meta {
    Save(String),
    Run(String),
    List,
    Show(String),
    Delete(String),
}

fn clean_arg(s: &str) -> String {
    s.trim().trim_end_matches(|c| c == '.' || c == '!').trim().to_string()
}

/// Recognise the action commands. The phrases are deliberately specific
/// ("saved action") so they cannot be mistaken for file commands.
pub fn parse_meta(input: &str) -> Option<Meta> {
    let t = input.trim();
    let l = t.to_ascii_lowercase();
    let after = |prefix: &str| -> Option<String> {
        l.strip_prefix(prefix).map(|_| clean_arg(&t[prefix.len()..]))
    };
    if let Some(n) = after("save that as ") {
        return Some(Meta::Save(n));
    }
    if let Some(n) = after("run saved action ") {
        return Some(Meta::Run(n));
    }
    if let Some(n) = after("show saved action ") {
        return Some(Meta::Show(n));
    }
    if let Some(n) = after("delete saved action ") {
        return Some(Meta::Delete(n));
    }
    if matches!(clean_arg(&l).as_str(), "list saved actions" | "show saved actions") {
        return Some(Meta::List);
    }
    None
}

pub fn valid_name(name: &str) -> Result<(), PawError> {
    let ok = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-')
        && name.trim() == name;
    if ok {
        Ok(())
    } else {
        Err(PawError::Validation(format!(
            "an action name must be 1 to {MAX_NAME} letters, digits, spaces, - or _ (got \"{name}\")"
        )))
    }
}

fn plain(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || c == SEP)
}

pub fn parse_file(text: &str) -> Vec<Saved> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut p = line.splitn(3, '\t');
        let (Some(n), Some(c), Some(s)) = (p.next(), p.next(), p.next()) else { continue };
        if valid_name(n).is_err() {
            continue;
        }
        out.push(Saved {
            name: n.to_string(),
            command: c.to_string(),
            steps: s.split(SEP).filter(|x| !x.is_empty()).map(String::from).collect(),
        });
    }
    out
}

pub fn render_file(all: &[Saved]) -> String {
    let mut s = String::new();
    for a in all {
        s.push_str(&format!(
            "{}\t{}\t{}\n",
            a.name,
            a.command,
            a.steps.join(&SEP.to_string())
        ));
    }
    s
}

pub fn find<'a>(all: &'a [Saved], name: &str) -> Option<&'a Saved> {
    all.iter().find(|a| a.name == name)
}

/// Add a new action. Never overwrites: a name that exists must be deleted first.
pub fn add(all: &mut Vec<Saved>, name: &str, c: &Candidate) -> Result<(), PawError> {
    valid_name(name)?;
    if find(all, name).is_some() {
        return Err(PawError::Validation(format!(
            "a saved action named \"{name}\" already exists. Delete it first (\"delete saved action {name}\") if you want to replace it."
        )));
    }
    if all.len() >= MAX_ACTIONS {
        return Err(PawError::Validation(format!("the limit of {MAX_ACTIONS} saved actions is reached")));
    }
    if !plain(&c.command) || c.command.trim().is_empty() || c.steps.iter().any(|s| !plain(s)) {
        return Err(PawError::Validation(
            "that command has characters that cannot be saved safely".into(),
        ));
    }
    all.push(Saved { name: name.to_string(), command: c.command.clone(), steps: c.steps.clone() });
    Ok(())
}

pub fn remove(all: &mut Vec<Saved>, name: &str) -> Result<(), PawError> {
    let before = all.len();
    all.retain(|a| a.name != name);
    if all.len() == before {
        return Err(PawError::Validation(format!("no saved action named \"{name}\"")));
    }
    Ok(())
}

fn loc_is_session_bound(l: &LocationRef) -> bool {
    matches!(l, LocationRef::LastCreated | LocationRef::Named(_))
}

fn directive_session_bound(d: &Directive) -> bool {
    match d {
        Directive::Create { parent, .. } => loc_is_session_bound(parent),
        Directive::Copy { target, dest } | Directive::Move { target, dest } => {
            loc_is_session_bound(target) || loc_is_session_bound(dest)
        }
        _ => true, // any other directive kind is not saveable yet
    }
}

/// Whether a command's directives and plan may be saved, with the reason if not.
pub fn saveable(directives: &[Directive], ops: &[Operation]) -> Result<(), String> {
    if ops.is_empty() {
        return Err("it did nothing".into());
    }
    if !ops
        .iter()
        .all(|o| matches!(o, Operation::CreateDir { .. } | Operation::CreateFile { .. } | Operation::Copy { .. }))
    {
        return Err("only create-folder, create-file and copy actions can be saved for now".into());
    }
    if directives.iter().any(directive_session_bound) {
        return Err("it refers to \"it\", \"that\" or a bare folder name, which depends on this session. Use a full path".into());
    }
    Ok(())
}

pub fn describe_saved(a: &Saved) -> String {
    let mut s = format!("{}: {}\n", a.name, a.command);
    for (i, st) in a.steps.iter().enumerate() {
        s.push_str(&format!("  {}. {}\n", i + 1, st));
    }
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cand() -> Candidate {
        Candidate { command: "make a folder called x".into(), steps: vec!["Create folder /r/x".into()] }
    }
    #[test]
    fn meta_phrases_are_specific() {
        assert_eq!(parse_meta("save that as Setup"), Some(Meta::Save("Setup".into())));
        assert_eq!(parse_meta("Run saved action Setup."), Some(Meta::Run("Setup".into())));
        assert_eq!(parse_meta("list saved actions"), Some(Meta::List));
        assert_eq!(parse_meta("delete saved action a b"), Some(Meta::Delete("a b".into())));
        // plain file commands are not captured
        assert_eq!(parse_meta("delete action movie"), None);
        assert_eq!(parse_meta("show action x"), None);
        assert_eq!(parse_meta("run the thing"), None);
    }
    #[test]
    fn names_are_checked() {
        assert!(valid_name("My setup-1").is_ok());
        assert!(valid_name("").is_err());
        assert!(valid_name("a/b").is_err());
        assert!(valid_name("a\tb").is_err());
        assert!(valid_name(&"x".repeat(41)).is_err());
    }
    #[test]
    fn roundtrip_and_no_overwrite() {
        let mut all = Vec::new();
        add(&mut all, "one", &cand()).unwrap();
        assert!(add(&mut all, "ONE", &cand()).is_err());
        let text = render_file(&all);
        assert_eq!(parse_file(&text), all);
        remove(&mut all, "one").unwrap();
        assert!(remove(&mut all, "one").is_err());
    }
    #[test]
    fn control_characters_cannot_be_saved() {
        let mut all = Vec::new();
        let mut c = cand();
        c.command = "make a folder called x\ty".into();
        assert!(add(&mut all, "bad", &c).is_err());
        c.command = "ok".into();
        c.steps = vec!["a\u{1f}b".into()];
        assert!(add(&mut all, "bad", &c).is_err());
    }
}
