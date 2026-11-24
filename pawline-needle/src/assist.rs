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
mod confidence_tests {
    // The gate itself is exercised through NeedleAssist::propose, which needs
    // the model; the envelope parsing it relies on is covered above. This
    // documents the threshold contract.
    #[test]
    fn threshold_is_sane() {
        assert!(super::MIN_CONFIDENCE > 0.0 && super::MIN_CONFIDENCE < 1.0);
    }
}
