//! Deterministic intent parsing: no model, no network. Hand-written patterns
//! with synonym tables, entity/name extraction and multi-clause splitting.

use crate::error::PawError;
use crate::plan::{CreateKind, Directive, LocationRef};

const CREATE_VERBS: &[&str] = &["make", "create", "add", "new"];
const DELETE_VERBS: &[&str] = &["delete", "remove", "trash"];
const VERBS: &[&str] = &[
    "make", "create", "add", "new", "delete", "remove", "trash", "rename", "move", "copy", "open",
    "launch", "start", "list", "show", "set", "change", "undo",
];

/// Parse a full user message into directives, in order.
pub fn parse(input: &str) -> Result<Vec<Directive>, PawError> {
    let mut out = Vec::new();
    let mut first_err = None;
    for sentence in split_sentences(input) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if let Err(e) = parse_sentence(sentence, &mut out) {
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    }
    if out.is_empty() {
        // Deterministic math answers beat both the parse error and the model:
        // arithmetic is computed, never guessed.
        if let Some(text) = crate::math::try_answer(input) {
            return Ok(vec![Directive::Answer { text }]);
        }
        return Err(first_err.unwrap_or_else(|| {
            PawError::Parse(
                "no action words found. Try things like \"make a folder called x\" or \"delete the file y\"."
                    .into(),
            )
        }));
    }
    // A mix of recognized and unrecognized sentences stays strict: the user
    // must see the failure rather than have half their request silently run.
    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(out)
}

/// Split on sentence-ending ". " — never on dots inside names like "a.md",
/// which are not followed by a space.
fn split_sentences(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '.' && chars.peek() == Some(&' ') {
            parts.push(cur.clone());
            cur.clear();
            chars.next(); // eat the space
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

/// True when `word` starts a new command clause.
fn is_verb(word: &str) -> bool {
    VERBS.contains(&word)
}

fn parse_sentence(s: &str, out: &mut Vec<Directive>) -> Result<(), PawError> {
    let mut rest = s.trim().trim_end_matches('.');
    // "inside <ref>, <command>" / "in <ref> <command>" prefixes re-target the clause.
    let mut base: Option<LocationRef> = None;
    loop {
        let lower = rest.to_lowercase();
        let stripped = lower.strip_prefix("inside ").or_else(|| lower.strip_prefix("in "));
        let Some(after) = stripped else { break };
        // find where the location words end: a comma, or the next command verb.
        let prefix_len = rest.len() - after.len();
        let body = &rest[prefix_len..];
        let (loc_words, remainder) = split_location_prefix(body);
        if loc_words.is_empty() {
            break;
        }
        let loc = parse_location(&loc_words.join(" "))
            .unwrap_or_else(|| LocationRef::Named(loc_words.join(" ")));
        base = Some(loc);
        rest = remainder;
        if rest.is_empty() {
            return Err(PawError::Parse("nothing to do after the location".into()));
        }
    }

    // Split the remainder into command clauses on conjunctions that are
    // followed by a verb — so "a, b, and c" stays a list.
    for clause in split_clauses(rest) {
        let d = parse_command(clause.trim(), base.clone())?;
        out.push(d);
    }
    Ok(())
}

/// Take leading words of `body` as the location until a comma or a verb.
/// Returns (location words, remainder).
fn split_location_prefix(body: &str) -> (Vec<String>, &str) {
    let mut words = Vec::new();
    let mut byte_pos = 0;
    for (i, tok) in body.split_whitespace().enumerate() {
        let clean = tok.trim_end_matches(',');
        if i > 0 && (is_verb(clean) || tok.ends_with(',')) {
            break;
        }
        words.push(clean.to_string());
        byte_pos += tok.len() + 1;
        if tok.ends_with(',') {
            break;
        }
    }
    let remainder = body.get(byte_pos..).unwrap_or("").trim_start_matches([',', ' ']);
    (words, remainder)
}

/// Split on " then " and on " and " when the next word is a command verb.
fn split_clauses(s: &str) -> Vec<&str> {
    let mut cuts: Vec<usize> = Vec::new(); // byte offsets where a new clause starts
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        for sep in [" then ", " and then ", " and "] {
            if s[i..].to_lowercase().starts_with(sep) {
                let after = &s[i + sep.len()..];
                let next_word = after.split_whitespace().next().unwrap_or("").to_lowercase();
                if is_verb(&next_word) {
                    cuts.push(i);
                    i += sep.len();
                    cuts.push(i); // mark resume point via paired entries
                }
                break;
            }
        }
        i += 1;
    }
    if cuts.is_empty() {
        return vec![s];
    }
    let mut spans = Vec::new();
    let mut start = 0;
    let mut k = 0;
    while k < cuts.len() {
        let cut_at = cuts[k];
        let resume = cuts[k + 1];
        spans.push(&s[start..cut_at]);
        start = resume;
        k += 2;
    }
    spans.push(&s[start..]);
    spans
}

fn parse_command(cmd: &str, base: Option<LocationRef>) -> Result<Directive, PawError> {
    let lower = cmd.to_lowercase();
    for (pre, engine) in [
        ("search the web for ", crate::plan::SearchEngine::Web),
        ("search online for ", crate::plan::SearchEngine::Web),
        ("web search ", crate::plan::SearchEngine::Web),
        ("search wikipedia for ", crate::plan::SearchEngine::Wiki),
        ("wikipedia ", crate::plan::SearchEngine::Wiki),
        ("wiki ", crate::plan::SearchEngine::Wiki),
    ] {
        if let Some(q) = lower.strip_prefix(pre) {
            let q = q.trim().trim_end_matches(['?', ' ']).trim();
            if !q.is_empty() {
                return Ok(Directive::Search {
                    engine,
                    query: q.to_string(),
                });
            }
        }
    }
    if matches!(
        lower.trim_end_matches(|c| c == '.' || c == '!'),
        "undo" | "undo that" | "undo it" | "undo the last change" | "undo last change"
    ) {
        return Ok(Directive::Undo);
    }
    if let Some(d) = parse_wallpaper(cmd) {
        return d;
    }
    let lower = cmd.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");

    if CREATE_VERBS.contains(&first) || lower.starts_with("create ") || lower.starts_with("make ") {
        return parse_create(cmd, base);
    }
    if DELETE_VERBS.contains(&first) {
        let target_words = lower
            .splitn(2, char::is_whitespace)
            .nth(1)
            .unwrap_or("")
            .trim_start_matches("the ")
            .trim_start_matches("folder ")
            .trim_start_matches("file ")
            .trim();
        if target_words.is_empty() {
            return Err(PawError::Parse("delete what, exactly?".into()));
        }
        let target_words = target_words
            .trim_start_matches("called ")
            .trim_start_matches("named ")
            .trim();
        return Ok(Directive::Delete {
            target: name_to_location(target_words),
        });
    }
    if first == "rename" {
        // "rename <target> to <new name>"
        let body = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        let (t, n) = body
            .split_once(" to ")
            .ok_or_else(|| PawError::Parse("rename needs \"... to <new name>\"".into()))?;
        return Ok(Directive::Rename {
            target: name_to_location(&t.to_lowercase()),
            new_name: n.trim().to_string(),
        });
    }
    if first == "move" || first == "copy" {
        let body = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        let (t, d) = body
            .split_once(" to ")
            .ok_or_else(|| PawError::Parse(format!("{first} needs \"... to <place>\"")))?;
        let target = name_to_location(&t.to_lowercase());
        let dest = parse_location(d).unwrap_or_else(|| name_to_location(&d.to_lowercase()));
        return Ok(if first == "move" {
            Directive::Move { target, dest }
        } else {
            Directive::Copy { target, dest }
        });
    }
    if first == "open" || first == "launch" || first == "start" {
        let name = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        if name.is_empty() {
            return Err(PawError::Parse("open what?".into()));
        }
        return Ok(Directive::OpenApp { name: name.to_string() });
    }
    if first == "list" || first == "show" {
        let body = lower.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        let target = if body.is_empty() {
            LocationRef::CurrentDir
        } else {
            let b = body.trim_start_matches("me ").trim_start_matches("the contents of ").trim();
            parse_location(b).unwrap_or_else(|| name_to_location(b))
        };
        return Ok(Directive::List { target });
    }
    Err(PawError::Parse(format!(
        "\"{first}\" isn't an action I know. I can make, delete, rename, move, copy, open and list."
    )))
}

/// Parse a create clause: "<verb> [a] [kind] [in <loc>] called|named <names>"
fn parse_create(cmd: &str, base: Option<LocationRef>) -> Result<Directive, PawError> {
    let (names_part, before_names) = match cmd.to_lowercase().split_once(" called ") {
        Some((b, n)) => (n.to_string(), b.to_string()),
        None => match cmd.to_lowercase().split_once(" named ") {
            Some((b, n)) => (n.to_string(), b.to_string()),
            None => {
                return Err(PawError::Parse(
                    "tell me what to call it, e.g. \"make a folder called kitten paws\"".into(),
                ))
            }
        },
    };

    // Kind and optional inline location live in the part before "called".
    // A location may also trail the names: "create a directory named notes in ~".
    let (names_part, mut trailing_loc) = match names_part.rsplit_once(" in ") {
        Some((n, l)) if parse_location(l).is_some() => (n.to_string(), parse_location(l)),
        _ => (names_part, None),
    };
    let mut kind = CreateKind::Dir;
    let words: Vec<&str> = before_names.split_whitespace().collect();
    let mut loc: Option<LocationRef> = None;
    let mut i = 1; // skip the verb
    let mut plural = false;
    while i < words.len() {
        let w = words[i].to_lowercase();
        match w.as_str() {
            "a" | "an" | "the" => {}
            "folder" | "directory" => kind = CreateKind::Dir,
            "folders" | "directories" => {
                kind = CreateKind::Dir;
                plural = true;
            }
            "file" => {
                if matches!(kind, CreateKind::Dir) {
                    kind = CreateKind::File { ext_hint: None };
                }
            }
            "files" => {
                if matches!(kind, CreateKind::Dir) {
                    kind = CreateKind::File { ext_hint: None };
                }
                plural = true;
            }
            "markdown" | "md" => kind = CreateKind::File { ext_hint: Some("md".into()) },
            "text" | "txt" => kind = CreateKind::File { ext_hint: Some("txt".into()) },
            "in" | "inside" => {
                let loc_words = words[i + 1..].join(" ");
                if loc_words.is_empty() {
                    return Err(PawError::Parse("in where, exactly?".into()));
                }
                loc = Some(
                    parse_location(&loc_words).unwrap_or_else(|| name_to_location(&loc_words)),
                );
                break;
            }
            other => {
                return Err(PawError::Parse(format!(
                    "\"{other}\" — I only make folders and files"
                )))
            }
        }
        i += 1;
    }

    let names = split_name_list(&names_part);
    if names.is_empty() {
        return Err(PawError::Parse("no name given after \"called\"".into()));
    }
    if !plural && names.len() > 1 {
        // "create a folder called a, b, and c" — accept, but as separate folders.
    }
    Ok(Directive::Create {
        kind,
        names,
        parent: loc.or(trailing_loc.take()).or(base).unwrap_or(LocationRef::CurrentDir),
    })
}

/// "gumpus, katkota, and pics" -> ["gumpus", "katkota", "pics"];
/// 'a folder called "my thing"' keeps quotes out of names.
fn split_name_list(s: &str) -> Vec<String> {
    s.split(',')
        .flat_map(|piece| {
            let p = piece.trim().trim_start_matches("and ").trim().to_string();
            // "x and y" tails split into two names
            match p.rsplit_once(" and ") {
                Some((a, b)) => vec![a.to_string(), b.to_string()],
                None => vec![p],
            }
        })
        .map(|n| n.trim().trim_matches('"').trim_matches('\'').trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// Well-known location words.
fn parse_location(s: &str) -> Option<LocationRef> {
    let l = s.trim().trim_end_matches('.').to_lowercase();
    match l.as_str() {
        "my home directory" | "home directory" | "the home directory" | "home" | "~" => {
            Some(LocationRef::Home)
        }
        "the desktop" | "desktop" | "my desktop" => Some(LocationRef::Desktop),
        "documents" | "my documents" | "the documents folder" => Some(LocationRef::Documents),
        "downloads" | "my downloads" | "the downloads folder" => Some(LocationRef::Downloads),
        "it" | "that" | "that folder" | "the folder" | "this folder" => {
            Some(LocationRef::LastCreated)
        }
        _ => {
            if l.starts_with('/') || l.starts_with("~/") || l.contains('/') {
                Some(LocationRef::Explicit(s.trim().trim_end_matches('.').to_string()))
            } else {
                None
            }
        }
    }
}

fn name_to_location(words: &str) -> LocationRef {
    parse_location(words).unwrap_or_else(|| {
        let w = words.trim().trim_end_matches('.').to_string();
        if w.starts_with('/') || w.starts_with('~') {
            LocationRef::Explicit(w)
        } else {
            LocationRef::Named(w)
        }
    })
}

/// "set wallpaper to X", "change my background to X", "set X as wallpaper".
/// Returns None when the clause is not about the wallpaper.
fn parse_wallpaper(cmd: &str) -> Option<Result<Directive, PawError>> {
    let lower = cmd.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    if first != "set" && first != "change" {
        return None;
    }
    let is_wp = |w: &str| matches!(w, "wallpaper" | "background" | "backdrop");
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let lw: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    // "<verb> [the|my] <wallpaper-word> [to] <path...>"
    let mut i = 1;
    if i < lw.len() && (lw[i] == "the" || lw[i] == "my") {
        i += 1;
    }
    let path_words: Vec<&str> = if i < lw.len() && is_wp(&lw[i]) {
        i += 1;
        if i < lw.len() && lw[i] == "to" {
            i += 1;
        }
        words[i..].to_vec()
    } else if first == "set" && lw.len() >= 3 {
        // "set <path...> as [the|my] <wallpaper-word>"
        let n = lw.len();
        let tail = if n >= 2 && is_wp(&lw[n - 1]) { n - 1 } else { return None };
        let mut as_at = tail;
        if as_at >= 1 && (lw[as_at - 1] == "the" || lw[as_at - 1] == "my") {
            as_at -= 1;
        }
        if as_at >= 2 && lw[as_at - 1] == "as" {
            words[1..as_at - 1].to_vec()
        } else {
            return None;
        }
    } else {
        return None;
    };
    let mut path_words = path_words;
    let mut via: Option<String> = None;
    if path_words.len() >= 3 {
        let n = path_words.len();
        let kw = path_words[n - 2].to_lowercase();
        if kw == "using" || kw == "with" || kw == "via" {
            via = Some(path_words[n - 1].to_string());
            path_words.truncate(n - 2);
        }
    }
    let target = path_words.join(" ");
    let target = target.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if target.is_empty() {
        return Some(Err(PawError::Parse("which picture should be the wallpaper?".into())));
    }
    Some(Ok(Directive::SetWallpaper { image: name_to_location(target), via }))
}
