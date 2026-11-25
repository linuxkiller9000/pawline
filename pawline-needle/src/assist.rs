//! Bridge from the embedded Needle runtime to pawline-core's `Assist` trait.
//!
//! The model returns tool-call JSON; this module maps it onto core
//! `Directive`s. Anything the model invents that the core validator rejects
//! (escaping paths, unknown shapes) dies here or in validation. The model
//! never sees a path outside the root and never executes anything.

use pawline_core::assist::Assist;
use pawline_core::error::PawError;
use pawline_core::plan::{CreateKind, Directive, LocationRef};
use serde_json::Value;

use crate::Needle;

/// Tool schema handed to the model. Single-purpose, one call per reply:
/// the deterministic engine owns multi-step planning; the assist only
/// helps with single intents the parser did not understand.
pub const PAWLINE_TOOLS: &str = r#"[
  {"name":"create_folder","description":"Create a folder","parameters":{"type":"object","properties":{"path":{"type":"string","description":"folder name or path"}},"required":["path"]}},
  {"name":"create_file","description":"Create a file","parameters":{"type":"object","properties":{"path":{"type":"string"},"extension":{"type":"string","description":"extension like md or txt if the name has none"}},"required":["path"]}},
  {"name":"delete","description":"Delete a file or folder","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
  {"name":"rename","description":"Rename a file or folder","parameters":{"type":"object","properties":{"path":{"type":"string"},"new_name":{"type":"string"}},"required":["path","new_name"]}},
  {"name":"move_item","description":"Move a file or folder","parameters":{"type":"object","properties":{"path":{"type":"string"},"destination":{"type":"string"}},"required":["path","destination"]}},
  {"name":"copy_item","description":"Copy a file or folder","parameters":{"type":"object","properties":{"path":{"type":"string"},"destination":{"type":"string"}},"required":["path","destination"]}},
  {"name":"list_items","description":"List a folder's contents","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
  {"name":"open_app","description":"Open an application","parameters":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}
]"#;

pub const ASSIST_SYSTEM_PROMPT: &str = "You are Pawline's command assist on a Linux desktop. Turn the user's request into exactly one tool call. Use simple relative names unless the user gave a path.";

pub struct NeedleAssist {
    needle: Needle,
}

impl NeedleAssist {
    pub fn embedded() -> Result<Self, crate::NeedleError> {
        Ok(NeedleAssist {
            needle: Needle::embedded(ASSIST_SYSTEM_PROMPT, PAWLINE_TOOLS)?,
        })
    }

    /// Direct access for diagnostics and demos.
    pub fn needle(&self) -> &Needle {
        &self.needle
    }
}

/// A path string from the model becomes a location reference: absolute or
/// ~-relative stays explicit (the validator canonicalizes and confines it);
/// anything else is a bare name resolved against session context.
fn to_location(s: &str) -> LocationRef {
    let t = s.trim();
    if t.starts_with('/') || t.starts_with('~') {
        LocationRef::Explicit(t.to_string())
    } else {
        LocationRef::Named(t.to_string())
    }
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, PawError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PawError::Parse(format!("assist reply missing argument `{key}`")))
}

/// Extract the tool-call object from a reply. The Cactus runtime answers
/// with an envelope: {"type":"call","success":true,"function_calls":[{"name",
/// "arguments"}], "confidence":...}. Also accept a bare {"name","arguments"}
/// object. Tolerate surrounding prose by scanning for the first object.
fn extract_json(reply: &str) -> Result<Value, PawError> {
    let start = reply
        .find('{')
        .ok_or_else(|| PawError::Parse("assist reply had no JSON".into()))?;
    let mut depth = 0i32;
    let mut end = None;
    for (i, c) in reply[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| PawError::Parse("assist reply JSON unterminated".into()))?;
    serde_json::from_str(&reply[start..end])
        .map_err(|e| PawError::Parse(format!("assist reply not JSON: {e}")))
}

/// Every tool call in the reply, in order. An envelope with
/// `"success": false` is an error, calls and all.
pub fn parse_tool_calls(reply: &str) -> Result<Vec<Directive>, PawError> {
    let v = extract_json(reply)?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        let why = v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(PawError::Parse(format!("assist reported failure: {why}")));
    }
    let calls: Vec<&Value> = match v.get("function_calls").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr.iter().collect(),
        _ => vec![&v],
    };
    calls.iter().map(|c| one_call(c)).collect()
}

/// Back-compat wrapper: the first directive only.
pub fn parse_tool_call(reply: &str) -> Result<Directive, PawError> {
    Ok(parse_tool_calls(reply)?.remove(0))
}

fn one_call(v: &Value) -> Result<Directive, PawError> {
    let name = v
        .get("name")
        .or_else(|| v.get("function"))
        .and_then(Value::as_str)
        .ok_or_else(|| PawError::Parse("assist reply missing tool name".into()))?;
    let args = v
        .get("arguments")
        .or_else(|| v.get("parameters"))
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let args = match args {
        Value::String(s) => serde_json::from_str(&s)
            .map_err(|e| PawError::Parse(format!("assist arguments not JSON: {e}")))?,
        other => other,
    };
    match name {
        "create_folder" => Ok(Directive::Create {
            kind: CreateKind::Dir,
            names: vec![arg(&args, "path")?.to_string()],
            parent: LocationRef::CurrentDir,
        }),
        "create_file" => Ok(Directive::Create {
            kind: CreateKind::File {
                ext_hint: args
                    .get("extension")
                    .and_then(Value::as_str)
                    .map(|s| s.trim_start_matches('.').to_string()),
            },
            names: vec![arg(&args, "path")?.to_string()],
            parent: LocationRef::CurrentDir,
        }),
        "delete" => Ok(Directive::Delete {
            target: to_location(arg(&args, "path")?),
        }),
        "rename" => Ok(Directive::Rename {
            target: to_location(arg(&args, "path")?),
            new_name: arg(&args, "new_name")?.to_string(),
        }),
        "move_item" => Ok(Directive::Move {
            target: to_location(arg(&args, "path")?),
            dest: to_location(arg(&args, "destination")?),
        }),
        "copy_item" => Ok(Directive::Copy {
            target: to_location(arg(&args, "path")?),
            dest: to_location(arg(&args, "destination")?),
        }),
        "list_items" => Ok(Directive::List {
            target: to_location(arg(&args, "path")?),
        }),
        "open_app" => Ok(Directive::OpenApp {
            name: arg(&args, "name")?.to_string(),
        }),
        other => Err(PawError::Parse(format!("assist proposed unknown tool `{other}`"))),
    }
}

/// Replies below this confidence are refused. Observed calibration on
/// needle3: 0.71 and 0.39 replies were correct tool picks; a 0.22 reply
/// mapped "create a directory" to rename (garbage). 0.3 sits between the
/// observed good and bad cases; tunable as more data lands.
pub const MIN_CONFIDENCE: f64 = 0.3;

/// Position of the first whole-token occurrence of `needle` in `hay`
/// (case-insensitive), as a byte offset into the lowercased text. A token
/// boundary is a character that is not a letter or digit and does not
/// extend a path or file name (`_ - / ~`, or a `.` followed by a letter or
/// digit). Unicode names and names with spaces work; a name that is only
/// part of a longer word, path or file name does not match.
fn find_token(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.to_lowercase();
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    let mut from = 0;
    while let Some(i) = h[from..].find(&n) {
        let a = from + i;
        let b = a + n.len();
        let joins = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '~');
        let before_ok = h[..a].chars().next_back().map_or(true, |c| !joins(c) && c != '.');
        let mut after = h[b..].chars();
        let after_ok = match after.next() {
            None => true,
            Some('.') => after.next().map_or(true, |c| !c.is_alphanumeric()),
            Some(c) => !joins(c),
        };
        if before_ok && after_ok {
            return Some(a);
        }
        from = a + h[a..].chars().next().map_or(1, |c| c.len_utf8());
    }
    None
}


fn words(input: &str) -> Vec<String> {
    input
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Operation classes the user's own words can express.
fn classes_in(input: &str) -> Vec<&'static str> {
    let w = words(input);
    let has = |l: &[&str]| w.iter().any(|x| l.contains(&x.as_str()));
    let mut out = Vec::new();
    if has(&["make", "create", "new", "mkdir", "add", "build", "whip", "generate", "setup", "creating", "making", "created", "made"])
        || (has(&["set"]) && has(&["up"]))
    {
        out.push("create");
    }
    if has(&["delete", "remove", "trash", "erase", "rm", "destroy", "deleting", "removing", "deleted", "removed"]) {
        out.push("delete");
    }
    if has(&["rename", "renaming", "renamed"]) {
        out.push("rename");
    }
    if has(&["move", "mv", "put", "relocate", "moving", "moved", "putting"]) {
        out.push("move");
    }
    if has(&["copy", "cp", "duplicate", "copying", "copied"]) {
        out.push("copy");
    }
    if has(&["list", "ls", "show", "listing", "showing"]) {
        out.push("list");
    }
    if has(&["open", "launch", "start", "run", "opening", "starting"]) {
        out.push("open");
    }
    out
}

fn tool_class(tool: &str) -> &'static str {
    match tool {
        "create_folder" | "create_file" => "create",
        "delete" => "delete",
        "rename" => "rename",
        "move_item" => "move",
        "copy_item" => "copy",
        "list_items" => "list",
        _ => "open",
    }
}

fn ext_in_input(input: &str, ext: &str) -> bool {
    let w = words(input);
    let aliases: &[&str] = match ext {
        "md" => &["md", "markdown"],
        "txt" => &["txt", "text"],
        "py" => &["py", "python"],
        "rs" => &["rs", "rust"],
        "js" => &["js", "javascript"],
        "sh" => &["sh", "shell", "bash"],
        "json" => &["json"],
        "csv" => &["csv"],
        "html" => &["html"],
        _ => &[],
    };
    w.iter().any(|x| x == ext || aliases.contains(&x.as_str()))
}

fn refuse(msg: String) -> PawError {
    PawError::Parse(msg)
}

/// The model must not invent, clip or re-role what the user wrote. For every
/// proposed call: (1) the operation must be one the user's words express and
/// no other operation may appear in them; (2) every name or path must appear
/// as a whole token in the user's words; (3) a stem is accepted only when
/// the user also named the extension; (4) for rename, move and copy the
/// source must come before the target in the user's words. Anything that
/// cannot be grounded is refused. Nothing is corrected or guessed here.
pub fn check_grounding(input: &str, reply: &str) -> Result<(), PawError> {
    let v = extract_json(reply)?;
    let calls: Vec<&Value> = match v.get("function_calls").and_then(Value::as_array) {
        Some(arr) if !arr.is_empty() => arr.iter().collect(),
        _ => vec![&v],
    };
    let classes = classes_in(input);
    for c in calls {
        let tool = c
            .get("name")
            .or_else(|| c.get("function"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let args = match c.get("arguments").or_else(|| c.get("parameters")) {
            Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
            Some(o) => o.clone(),
            None => Value::Null,
        };
        let want = tool_class(tool);
        if tool != "open_app" && (classes.len() != 1 || classes[0] != want) {
            return Err(refuse(format!(
                "assist proposed `{tool}` but your words do not clearly ask for exactly that one operation. Nothing was planned; please rephrase or use a plain command like: make a folder called <name>"
            )));
        }
        let ext = args
            .get("extension")
            .and_then(Value::as_str)
            .map(|e| e.trim_start_matches('.').to_lowercase());
        let mut positions: Vec<(&str, usize)> = Vec::new();
        for key in ["path", "new_name", "destination", "name"] {
            if let Some(val) = args.get(key).and_then(Value::as_str) {
                let mut cand = val.to_string();
                if key == "path" && !cand.is_empty() {
                    if let Some(e) = &ext {
                        let suffix = format!(".{e}");
                        if cand.to_lowercase().ends_with(&suffix)
                            && find_token(input, &cand).is_none()
                        {
                            if !ext_in_input(input, e) {
                                return Err(refuse(format!(
                                    "assist added the file type \".{e}\" but you did not name it. Nothing was planned; please say the full file name, like: make a file called {}.{e}",
                                    &cand[..cand.len() - suffix.len()]
                                )));
                            }
                            cand.truncate(cand.len() - suffix.len());
                        }
                    }
                }
                match find_token(input, &cand) {
                    Some(p) => positions.push((key, p)),
                    None => {
                        return Err(refuse(format!(
                            "assist proposed the name \"{val}\" but that is not what you wrote. Nothing was planned; please rephrase or use a plain command like: make a folder called <name>"
                        )))
                    }
                }
            }
        }
        let pos = |k: &str| positions.iter().find(|(x, _)| *x == k).map(|(_, p)| *p);
        let second = if tool == "rename" { "new_name" } else { "destination" };
        if matches!(tool, "rename" | "move_item" | "copy_item") {
            match (pos("path"), pos(second)) {
                (Some(a), Some(b)) if a < b => {}
                _ => {
                    return Err(refuse(
                        "assist could not confirm which name is the source and which is the target from your words. Nothing was planned; please rephrase or use a plain command like: move <source> to <target>".into(),
                    ))
                }
            }
        }
    }
    Ok(())
}

impl Assist for NeedleAssist {
    fn propose(&self, input: &str) -> Result<Vec<Directive>, PawError> {
        // Fresh session per user input: cactus keeps conversation state and
        // an earlier low-quality turn measurably contaminates later replies.
        self.needle.reset();
        let reply = self
            .needle
            .propose(input, 128)
            .map_err(|e| PawError::Parse(format!("assist inference failed: {}", e.0)))?;
        let v = extract_json(&reply)?;
        if let Some(c) = v.get("confidence").and_then(Value::as_f64) {
            if c < MIN_CONFIDENCE {
                return Err(PawError::Parse(format!(
                    "assist not confident enough ({c:.2} < {MIN_CONFIDENCE}); ask the user to rephrase"
                )));
            }
        }
        check_grounding(input, &reply)?;
        parse_tool_calls(&reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_tool_call() {
        let d = parse_tool_call(r#"{"name":"create_folder","arguments":{"path":"kitten paws"}}"#)
            .unwrap();
        match d {
            Directive::Create { kind, names, .. } => {
                assert_eq!(kind, CreateKind::Dir);
                assert_eq!(names, vec!["kitten paws".to_string()]);
            }
            other => panic!("wrong directive: {other:?}"),
        }
    }

    #[test]
    fn tolerates_prose_around_json() {
        let d = parse_tool_call(
            "Sure! {\"name\":\"delete\",\"arguments\":{\"path\":\"/etc/passwd\"}} done.",
        )
        .unwrap();
        match d {
            Directive::Delete { target } => {
                assert_eq!(target, LocationRef::Explicit("/etc/passwd".into()));
            }
            other => panic!("wrong directive: {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_tool() {
        assert!(parse_tool_call(r#"{"name":"run_shell","arguments":{"cmd":"rm -rf /"}}"#).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_tool_call("no json here").is_err());
    }

    #[test]
    fn arguments_may_be_a_stringified_object() {
        let d = parse_tool_call(
            r#"{"name":"open_app","arguments":"{\"name\":\"firefox\"}"}"#,
        )
        .unwrap();
        match d {
            Directive::OpenApp { name } => assert_eq!(name, "firefox"),
            other => panic!("wrong directive: {other:?}"),
        }
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    #[test]
    fn parses_cactus_envelope() {
        let reply = r#"{"type":"call","success":true,"error":null,"function_calls":[{"name":"create_folder","arguments":{"path":"kitten paws"}}],"confidence":0.71}"#;
        let ds = parse_tool_calls(reply).unwrap();
        assert_eq!(ds.len(), 1);
        match &ds[0] {
            Directive::Create { kind, names, .. } => {
                assert_eq!(*kind, CreateKind::Dir);
                assert_eq!(names, &vec!["kitten paws".to_string()]);
            }
            other => panic!("wrong directive: {other:?}"),
        }
    }

    #[test]
    fn multi_call_envelope_yields_multiple_directives() {
        let reply = r#"{"type":"call","success":true,"function_calls":[{"name":"create_folder","arguments":{"path":"a"}},{"name":"create_file","arguments":{"path":"a/b.md","extension":"md"}}]}"#;
        let ds = parse_tool_calls(reply).unwrap();
        assert_eq!(ds.len(), 2);
    }

    #[test]
    fn success_false_is_an_error() {
        let reply = r#"{"type":"call","success":false,"error":"cannot ground entity","function_calls":[]}"#;
        let e = parse_tool_calls(reply).unwrap_err().to_string();
        assert!(e.contains("cannot ground entity"), "got: {e}");
    }

    #[test]
    fn empty_function_calls_falls_back_to_bare_object() {
        let d = parse_tool_call(r#"{"name":"list_items","arguments":{"path":"."}}"#).unwrap();
        assert!(matches!(d, Directive::List { .. }));
    }
}

#[cfg(test)]
mod grounding_tests {
    use super::*;
    fn call(tool: &str, args: &str) -> String {
        format!(r#"{{"success":true,"function_calls":[{{"name":"{tool}","arguments":{args}}}],"confidence":0.5}}"#)
    }
    fn mk(path: &str) -> String {
        call("create_folder", &format!(r#"{{"path":"{path}"}}"#))
    }
    #[test]
    fn rejects_clipped_name() {
        assert!(check_grounding("could you please set up a new directory named pets for me", &mk("ets")).is_err());
        assert!(check_grounding("make a directory named pets", &mk("ets")).is_err());
        for (inp, bad) in [("create a folder called reports", "eports"), ("make a folder called invoices", "nvoices"), ("make a folder called taxes", "axes")] {
            assert!(check_grounding(inp, &mk(bad)).is_err(), "{inp}");
        }
    }
    #[test]
    fn accepts_exact_names() {
        assert!(check_grounding("set up a new directory named pets for me", &mk("pets")).is_ok());
        assert!(check_grounding("set up a new directory named Dogs", &mk("dogs")).is_ok());
        assert!(check_grounding("set up a new directory named kitten paws for me", &mk("kitten paws")).is_ok());
        assert!(check_grounding("create a folder called pets.", &mk("pets")).is_ok());
        assert!(check_grounding("make a folder called \"my pets\"", &mk("my pets")).is_ok());
        assert!(check_grounding("make a folder called caf\u{e9}s", &mk("caf\u{e9}s")).is_ok());
        assert!(check_grounding("make a folder called \u{627}\u{644}\u{642}\u{637}\u{629}", &mk("\u{627}\u{644}\u{642}\u{637}\u{629}")).is_ok());
        assert!(check_grounding("make a file called notes.md here", &call("create_file", r#"{"path":"notes.md"}"#)).is_ok());
    }
    #[test]
    fn rejects_invented_or_partial_names() {
        assert!(check_grounding("set up a new directory named dogs", &mk("cats")).is_err());
        assert!(check_grounding("set up a new directory named dogs", &mk("dogs2")).is_err());
        assert!(check_grounding("set up a new directory named dogs", &mk("dog")).is_err());
        // part of a longer file name or path is not the whole name
        assert!(check_grounding("make a folder called pets.bak", &mk("pets")).is_err());
        assert!(check_grounding("make a folder called docs/pets", &mk("pets")).is_err());
        assert!(check_grounding("make a folder called my-pets", &mk("pets")).is_err());
        assert!(check_grounding("make a folder called pets_old", &mk("pets")).is_err());
    }
    #[test]
    fn extension_hint_cannot_invent_a_type() {
        let r = call("create_file", r#"{"path":"ideas.md","extension":"md"}"#);
        assert!(check_grounding("whip up a markdown file called ideas", &r).is_ok());
        assert!(check_grounding("whip up a md file called ideas", &r).is_ok());
        assert!(check_grounding("whip up a file called ideas", &r).is_err());
        assert!(check_grounding("whip up a file called plans", &r).is_err());
        let r2 = call("create_file", r#"{"path":"ideas.exe","extension":"exe"}"#);
        assert!(check_grounding("whip up a file called ideas", &r2).is_err());
    }
    #[test]
    fn operation_must_match_the_users_words() {
        // same name, wrong operation
        let del = call("delete", r#"{"path":"pets"}"#);
        assert!(check_grounding("make a folder called pets", &del).is_err());
        assert!(check_grounding("please delete pets", &mk("pets")).is_err());
        assert!(check_grounding("please delete pets", &del).is_ok());
        // user words with two operations cannot be grounded to one call
        assert!(check_grounding("make pets and delete pets", &mk("pets")).is_err());
        assert!(check_grounding("make pets and delete pets", &del).is_err());
        // no operation word at all
        assert!(check_grounding("pets", &mk("pets")).is_err());
    }
    #[test]
    fn inflected_verbs_are_recognised() {
        let ok = call("move_item", r#"{"path":"a.txt","destination":"docs"}"#);
        assert!(check_grounding("would you mind moving a.txt into docs", &ok).is_ok());
        let del = call("delete", r#"{"path":"pets"}"#);
        assert!(check_grounding("deleting pets", &mk("pets")).is_err());
        assert!(check_grounding("deleting pets", &del).is_ok());
    }
    #[test]
    fn source_and_destination_roles_cannot_be_swapped() {
        let ok = call("move_item", r#"{"path":"a.txt","destination":"docs"}"#);
        let rev = call("move_item", r#"{"path":"docs","destination":"a.txt"}"#);
        assert!(check_grounding("move a.txt to docs", &ok).is_ok());
        assert!(check_grounding("move a.txt to docs", &rev).is_err());
        let cp = call("copy_item", r#"{"path":"docs","destination":"a.txt"}"#);
        assert!(check_grounding("copy a.txt into docs", &cp).is_err());
        let rn = call("rename", r#"{"path":"old","new_name":"new1"}"#);
        let rn_rev = call("rename", r#"{"path":"new1","new_name":"old"}"#);
        assert!(check_grounding("rename old to new1", &rn).is_ok());
        assert!(check_grounding("rename old to new1", &rn_rev).is_err());
        // a name in only one slot, or a clipped one, is refused
        let clip = call("move_item", r#"{"path":"a.txt","destination":"ocs"}"#);
        assert!(check_grounding("move a.txt to docs", &clip).is_err());
    }
}

#[cfg(test)]
mod confidence_tests {
    // The gate itself is exercised through NeedleAssist::propose, which needs
    // the model; the envelope parsing it relies on is covered above. This
    // documents the threshold contract.
    #[test]
    fn threshold_is_sane() {
        assert!(super::MIN_CONFIDENCE > 0.0 && super::MIN_CONFIDENCE < 1.0);
    }
}
