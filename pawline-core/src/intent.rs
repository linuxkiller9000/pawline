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
        let lower = rest.to_ascii_lowercase();
        let stripped = lower.strip_prefix("inside ").or_else(|| lower.strip_prefix("in "));
        let Some(after) = stripped else { break };
        // find where the location words end: a comma, or the next command verb.
        let prefix_len = rest.len() - after.len();
        let body = &rest[prefix_len..];
        let (loc_words, remainder) = split_location_prefix(body);
        if loc_words.is_empty() {
            break;
        }
        let loc_text = clean_loc_words(&loc_words.join(" "));
        let loc = parse_location(&loc_text).unwrap_or_else(|| LocationRef::Named(loc_text.clone()));
        base = Some(loc);
        rest = remainder;
        if rest.is_empty() {
            out.push(Directive::Answer { text: nothing_after_location(&loc_words) });
            return Ok(());
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

/// "inside of pawline folder i want to have 3 files.py": the location words swallowed the
/// whole sentence. Ask a specific question instead of guessing names or counts.
fn nothing_after_location(loc_words: &[String]) -> String {
    let joined = loc_words.join(" ");
    let lw = joined.to_lowercase();
    if let Some(i) = lw.find(" i want") .or_else(|| lw.find(" i need")).or_else(|| lw.find(" i would like")) {
        let place = clean_loc_words(&joined[..i]);
        return format!(
            "I need the exact file names. \"{}\" doesn't say what to call them or what 3 means here (how many, or part of a name). Tell me the names, e.g. \"make files a.py, b.py, c.py inside {place}\", and I'll show them to you before creating anything. I create empty files only.",
            joined[i..].trim()
        );
    }
    "Tell me what to do inside that folder, e.g. \"make a file called notes.txt\".".into()
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
        // Names may hold non-ASCII letters: never slice inside a character.
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        for sep in [" then ", " and then ", " and "] {
            if s[i..].to_ascii_lowercase().starts_with(sep) {
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

/// Built-in skills only. Learned skills are not built yet; when they are, they will be
/// reviewed, validated, scoped data listed here, never downloaded commands.
const SKILLS_TEXT: &str = "Skills (built in):\n/files: make, delete, rename, move, copy, list\n/open: open an installed app (exact name, asks first)\n/wallpaper: set a wallpaper (asks first, undoable)\n/search: search the web or Wikipedia\n/math: simple math and solve for x\n/picture: make a picture with Pollinations or AI Horde (you pick, asks first)\n/ask: ask an online model a question (asks first unless it is clearly general)\n/undo: undo the last change\nLearned skills: none yet.";

fn parse_command(cmd: &str, base: Option<LocationRef>) -> Result<Directive, PawError> {
    // ASCII lowercase keeps byte offsets identical to `cmd`, so slices of
    // `lower` can be mapped back to the user's exact letters.
    let lower = cmd.to_ascii_lowercase();
    if matches!(lower.trim().trim_end_matches(['?', '.', '!']), "/skills" | "/skill" | "skills" | "my skills" | "list skills" | "what skills do you have") {
        return Ok(Directive::Answer { text: SKILLS_TEXT.into() });
    }
    // Greetings, thanks and "what can you do" get a plain reply, never a refusal.
    if let Some(text) = small_talk(&lower) {
        return Ok(Directive::Answer { text });
    }
    if let Some(place) = crate::weather::parse_place(&lower) {
        if place.is_empty() {
            return Ok(Directive::Answer { text: "Which place? For example: weather in Cairo. Pawline never guesses your location.".into() });
        }
        // Keep the user's own capitalisation for the place name.
        let place = match lower.rfind(&place) {
            Some(i) if cmd.to_lowercase() == lower => {
                let start = lower[..i].chars().count();
                cmd.chars().skip(start).take(place.chars().count()).collect()
            }
            _ => place,
        };
        return Ok(Directive::Weather { place });
    }
    for (pre, engine) in [
        ("search wikipedia for ", crate::plan::SearchEngine::Wiki),
        ("search wikipedia ", crate::plan::SearchEngine::Wiki),
        ("search the web for ", crate::plan::SearchEngine::Web),
        ("search the web ", crate::plan::SearchEngine::Web),
        ("search online ", crate::plan::SearchEngine::Web),
        ("search for ", crate::plan::SearchEngine::Web),
        ("search up ", crate::plan::SearchEngine::Web),
        ("look up ", crate::plan::SearchEngine::Web),
        ("lookup ", crate::plan::SearchEngine::Web),
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
    let lower = cmd.to_ascii_lowercase();
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
            target: place_ref(original_case(cmd, &lower, target_words)),
        });
    }
    if first == "rename" {
        // "rename <target> to <new name>"
        let body = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        let (t, n) = body
            .split_once(" to ")
            .ok_or_else(|| PawError::Parse("rename needs \"... to <new name>\"".into()))?;
        return Ok(Directive::Rename {
            target: place_ref(t),
            new_name: n.trim().to_string(),
        });
    }
    if first == "move" || first == "copy" {
        let body = cmd.splitn(2, char::is_whitespace).nth(1).unwrap_or("").trim();
        let (t, d) = body
            .split_once(" to ")
            .ok_or_else(|| PawError::Parse(format!("{first} needs \"... to <place>\"")))?;
        let target = place_ref(t);
        let dest = parse_location(d).unwrap_or_else(|| place_ref(d));
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
            let b = original_case(cmd, &lower, b);
            parse_location(b).unwrap_or_else(|| place_ref(b))
        };
        return Ok(Directive::List { target });
    }
    if matches!(first, "generate" | "draw" | "paint" | "imagine" | "render")
        || ((lower.contains("image of") || lower.contains("picture of")) && !lower.contains("wallpaper"))
    {
        return Ok(image_request(cmd, &lower, first));
    }
    Err(PawError::Parse(format!("I don't know how to \"{first}\" yet. Say \"help\" to see what I can do.")))
}

/// A picture request. The provider must be named; nothing is sent until the
/// user confirms the card. Without a provider the user gets a plain choice.
fn image_request(cmd: &str, lower: &str, first: &str) -> Directive {
    use crate::imagegen::ImageProvider;
    let mut end = lower.len();
    let mut provider = None;
    for (pat, p) in [
        (" with ai horde", ImageProvider::Horde),
        (" using ai horde", ImageProvider::Horde),
        (" via ai horde", ImageProvider::Horde),
        (" with horde", ImageProvider::Horde),
        (" using horde", ImageProvider::Horde),
        (" via horde", ImageProvider::Horde),
        (" with pollinations", ImageProvider::Pollinations),
        (" using pollinations", ImageProvider::Pollinations),
        (" via pollinations", ImageProvider::Pollinations),
    ] {
        let t = lower.trim_end_matches(|c: char| c == '.' || c == '!' || c.is_whitespace());
        if t.ends_with(pat) {
            end = t.len() - pat.len();
            provider = Some(p);
            break;
        }
    }
    let mut start = 0;
    if matches!(first, "generate" | "draw" | "paint" | "imagine" | "render") {
        start = first.len();
    }
    let mut body = lower[start..end].trim_start();
    let mut off = lower[start..end].len() - body.len() + start;
    for pre in ["me ", "an image of ", "a picture of ", "image of ", "picture of ", "an image ", "a picture ", "a image "] {
        if let Some(rest) = body.strip_prefix(pre) {
            off += pre.len();
            body = rest;
        }
    }
    let prompt = cmd.get(off..end).unwrap_or("").trim().trim_end_matches(['.', '!']).trim();
    if prompt.is_empty() || prompt == "an image" || prompt == "a picture" || prompt == "image" || prompt == "picture" {
        return Directive::Answer { text: "What should the picture show? For example: generate a picture of a lighthouse with pollinations".into() };
    }
    match provider {
        Some(p) => Directive::GenerateImage { provider: p, prompt: prompt.to_string() },
        None => Directive::Answer {
            text: format!(
                "Pictures come from a free online service, so I need you to pick one. Nothing was sent.\n- Pollinations: quick, no account, sends your description and IP address.\n- AI Horde: volunteer-run, can take minutes, sends your description and IP address.\nSay: {first} {prompt} with pollinations   (or: with horde)"
            ),
        },
    }
}

/// Plain replies for greetings, thanks and help. Pure text; touches nothing.
fn small_talk(lower: &str) -> Option<String> {
    let t: String = lower.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.strip_suffix(" pawline").unwrap_or(&t).to_string();
    const HELLO: &[&str] = &["hello", "hi", "hey", "hiya", "yo", "good morning", "good evening", "good afternoon", "hello there", "hi there", "hey there", "salam", "assalamu alaikum"];
    const THANKS: &[&str] = &["thanks", "thank you", "thx", "ty"];
    const HELP: &[&str] = &["help", "what can you do", "what do you do", "how do i use this", "how does this work", "commands", "examples"];
    const HELP_TEXT: &str = "## What I can do\n- **Files and folders**: `make a folder called kitten paws`. I can also delete, rename, move, copy and list them (deleting and moving ask first).\n- **Apps**: `open mpv`. I show the exact command and ask first.\n- **Wallpaper**: `set wallpaper to ~/Pictures/a.png`\n- **Search**: `search for what is a cat`, or `search wikipedia for cats`\n- **Math**: `what is 12 * 7`\n- **Weather**: `weather in Cairo`. A card asks first. If several places match, your pick sends only that place's coordinates for the forecast (open-meteo.com, covered by the same card).\n- **Presentations**: `make a presentation from notes.md` (local .pptx, you review it first). `write a presentation about the water cycle` asks an online model and shows a consent card first. The text is AI-written and not fact-checked.\n- **Pictures**: `generate a picture of a lighthouse with pollinations` (online, asks first).\n- **Chat**: small talk and jokes work offline, in English. `chat online` turns on online chat for this session only, and every turn shows a card with the exact text that would be sent.\n- **Arabic**: a fixed set of Arabic commands works (type `مساعدة`). Replies and cards are mostly English.\n- **Undo** the last change: `undo`\n> Tip: Ctrl+C / Ctrl+V copy and paste, click a reply to copy it.\nLimits: I am not a general chat assistant, voice is English only, and the online routes I know of (weather, search, questions, presentations, pictures, chat) show a card first and send nothing until you say yes.";
    if HELLO.contains(&t.as_str()) {
        return Some("Hi! I can work with files and folders, open apps, search and do math. Say \"help\" for examples.".into());
    }
    if THANKS.contains(&t.as_str()) {
        return Some("You're welcome.".into());
    }
    if HELP.contains(&t.as_str()) {
        return Some(HELP_TEXT.into());
    }
    None
}

/// "a.py inside pawline", "notes in ~", "x inside the pawline folder" -> (names, location).
/// "inside" is always a location; a bare "in" only when the tail is a well-known place,
/// so names like "lost in space" stay names.
fn split_trailing_location(names_part: String) -> (String, Option<LocationRef>) {
    let lower = names_part.to_ascii_lowercase();
    if let Some(i) = lower.rfind(" inside ") {
        let loc = clean_loc_words(&names_part[i + " inside ".len()..]);
        if !loc.is_empty() && !loc.contains(',') {
            return (names_part[..i].to_string(), Some(name_to_location(&loc)));
        }
    }
    if let Some((n, l)) = names_part.rsplit_once(" in ") {
        if parse_location(l).is_some() {
            return (n.to_string(), parse_location(l));
        }
    }
    (names_part, None)
}

/// "of the pawline folder" -> "pawline"
fn clean_loc_words(s: &str) -> String {
    let mut w: Vec<&str> = s.split_whitespace().collect();
    if w.first().is_some_and(|x| x.eq_ignore_ascii_case("of")) {
        w.remove(0);
    }
    if w.first().is_some_and(|x| x.eq_ignore_ascii_case("the")) && w.len() > 1 {
        w.remove(0);
    }
    if w.len() > 1 && w.last().is_some_and(|x| x.eq_ignore_ascii_case("folder") || x.eq_ignore_ascii_case("directory")) {
        w.pop();
    }
    w.join(" ")
}

/// "make a file inside pawline", "create files a.py, b.py in pawline": no "called".
/// Explicit dotted file names are taken as typed. Anything else asks a specific question;
/// names and contents are never invented.
fn parse_create_bare(cmd: &str, base: Option<LocationRef>) -> Result<Directive, PawError> {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let mut i = 1;
    while i < words.len() && matches!(words[i].to_lowercase().as_str(), "a" | "an" | "the" | "new") {
        i += 1;
    }
    let is_file_kind = words.get(i).is_some_and(|w| matches!(w.to_lowercase().as_str(), "file" | "files"));
    let rest = if is_file_kind { words[i + 1..].join(" ") } else { String::new() };
    let (names_part, loc) = split_trailing_location(rest.clone());
    let dotted_all = |t: &str| {
        let n = split_name_list(t);
        !n.is_empty() && n.iter().all(|n| n.contains('.') && !n.starts_with('.') && !n.ends_with('.') && !n.contains(' '))
    };
    let lower_np = names_part.to_ascii_lowercase();
    let (names_part, loc) = if loc.is_some() {
        (names_part, loc)
    } else if let Some(t) = lower_np.strip_prefix("in ").or_else(|| lower_np.strip_prefix("inside ")) {
        // "make a file in pawline": a place and no name
        let at = names_part.len() - t.len();
        (String::new(), Some(name_to_location(&clean_loc_words(&names_part[at..]))))
    } else if let Some((h, t)) = names_part.rsplit_once(" in ") {
        if dotted_all(h) && !t.contains(',') && !t.trim().is_empty() {
            (h.to_string(), Some(name_to_location(&clean_loc_words(t))))
        } else {
            (names_part, None)
        }
    } else {
        (names_part, None)
    };
    let names = split_name_list(&names_part);
    let dotted = !names.is_empty()
        && names.iter().all(|n| n.contains('.') && !n.starts_with('.') && !n.ends_with('.') && !n.contains(' '));
    if is_file_kind && dotted {
        return Ok(Directive::Create {
            kind: CreateKind::File { ext_hint: None },
            names,
            parent: loc.or(base).unwrap_or(LocationRef::CurrentDir),
        });
    }
    let place = match loc.as_ref().or(base.as_ref()) {
        Some(LocationRef::Named(n)) => format!(" inside {n}"),
        Some(LocationRef::LastCreated) => " inside that folder".to_string(),
        _ => String::new(),
    };
    Ok(Directive::Answer { text: format!(
        "What should the file be called? Give me the exact name, e.g. \"make a file called notes.txt{place}\". For several: \"make files a.py, b.py{place}\"."
    ) })
}

/// Parse a create clause: "<verb> [a] [kind] [in <loc>] called|named <names>"
fn parse_create(cmd: &str, base: Option<LocationRef>) -> Result<Directive, PawError> {
    let lc = cmd.to_ascii_lowercase();
    // Split on the keyword found case-insensitively, but keep the user's own
    // letters on both sides (a folder called "Pets" is not "pets").
    let split_at = |kw: &str| lc.find(kw).map(|i| (cmd[..i].to_string(), cmd[i + kw.len()..].to_string()));
    let (names_part, before_names) = match split_at(" called ").or_else(|| split_at(" named ")) {
        Some((b, n)) => (n, b),
        None => return parse_create_bare(cmd, base),
    };

    // Kind and optional inline location live in the part before "called".
    // A location may also trail the names: "create a directory named notes in ~".
    let (names_part, mut trailing_loc) = split_trailing_location(names_part);
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


/// Names and paths are case-sensitive on Linux: "Pets" and "pets" are
/// different files. Every name keeps exactly the letters the user typed and
/// is looked up that way; there is no case-insensitive nearest match.
fn place_ref(s: &str) -> LocationRef {
    name_to_location(s)
}

/// `piece` was cut from `lower` (the lowercased `cmd`); give back the same
/// text with the user's own letter case when the two lengths line up.
fn original_case<'a>(cmd: &'a str, lower: &str, piece: &'a str) -> &'a str {
    if cmd.len() == lower.len() {
        if let Some(i) = lower.find(piece) {
            if cmd.is_char_boundary(i) && cmd.is_char_boundary(i + piece.len()) {
                return &cmd[i..i + piece.len()];
            }
        }
    }
    piece
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
        let mut w = words.trim().trim_end_matches('.').to_string();
        // A name in matching quotes is the literal name (spaces and a trailing "please" included); the quotes are not part of it.
        for q in ['"', '\''] {
            if w.chars().count() >= 3 && w.starts_with(q) && w.ends_with(q) {
                w = w[1..w.len() - 1].to_string();
                break;
            }
        }
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
    let mut words: Vec<&str> = cmd.split_whitespace().collect();
    // Explicit acknowledgment that the current wallpaper cannot be backed up.
    let mut no_backup_ok = false;
    let joined = words.join(" ").to_lowercase();
    for suffix in [" anyway", " without backup", " with no backup", " no backup"] {
        if joined.ends_with(suffix) {
            words.truncate(words.len() - suffix.split_whitespace().count());
            no_backup_ok = true;
            break;
        }
    }
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
    Some(Ok(Directive::SetWallpaper { image: name_to_location(target), via, no_backup_ok }))
}

#[cfg(test)]
mod case_tests {
    use super::*;
    fn one(s: &str) -> Directive {
        parse(s).unwrap().remove(0)
    }
    #[test]
    fn explicit_paths_keep_their_case() {
        match one("copy /Tmp/Foo/Src.TXT to /Tmp/Bar") {
            Directive::Copy { target, dest } => {
                assert_eq!(target, LocationRef::Explicit("/Tmp/Foo/Src.TXT".into()));
                assert_eq!(dest, LocationRef::Explicit("/Tmp/Bar".into()));
            }
            d => panic!("{d:?}"),
        }
        match one("move ~/Docs/A.md to ~/Docs/Old") {
            Directive::Move { target, .. } => assert_eq!(target, LocationRef::Explicit("~/Docs/A.md".into())),
            d => panic!("{d:?}"),
        }
        match one("rename /Tmp/Old.TXT to New.TXT") {
            Directive::Rename { target, new_name } => {
                assert_eq!(target, LocationRef::Explicit("/Tmp/Old.TXT".into()));
                assert_eq!(new_name, "New.TXT");
            }
            d => panic!("{d:?}"),
        }
        match one("delete /Tmp/Foo/Bar") {
            Directive::Delete { target } => assert_eq!(target, LocationRef::Explicit("/Tmp/Foo/Bar".into())),
            d => panic!("{d:?}"),
        }
        match one("list /Tmp/Foo") {
            Directive::List { target } => assert_eq!(target, LocationRef::Explicit("/Tmp/Foo".into())),
            d => panic!("{d:?}"),
        }
    }
    #[test]
    fn bare_names_keep_their_exact_case() {
        match one("delete Pics") {
            Directive::Delete { target } => assert_eq!(target, LocationRef::Named("Pics".into())),
            d => panic!("{d:?}"),
        }
        match one("DELETE Pics") {
            Directive::Delete { target } => assert_eq!(target, LocationRef::Named("Pics".into())),
            d => panic!("{d:?}"),
        }
        match one("copy Pics to Documents") {
            Directive::Copy { target, dest } => {
                assert_eq!(target, LocationRef::Named("Pics".into()));
                assert_eq!(dest, LocationRef::Documents);
            }
            d => panic!("{d:?}"),
        }
    }
}

#[cfg(test)]
mod file_request_tests {
    use super::*;
    fn q(s: &str) -> String { match parse(s) { Ok(v) => match &v[..] { [Directive::Answer { text }] => text.clone(), o => panic!("{o:?}") }, o => panic!("{o:?}") } }

    #[test]
    fn explicit_names_inside_a_folder() {
        for s in ["make a file called a.py inside pawline", "inside pawline make a file called a.py", "make a file a.py in pawline"] {
            assert_eq!(parse(s).unwrap(), vec![Directive::Create { kind: CreateKind::File { ext_hint: None }, names: vec!["a.py".into()], parent: LocationRef::Named("pawline".into()) }], "{s}");
        }
        assert_eq!(parse("create files a.py, b.py in pawline").unwrap(), vec![Directive::Create { kind: CreateKind::File { ext_hint: None }, names: vec!["a.py".into(), "b.py".into()], parent: LocationRef::Named("pawline".into()) }]);
        assert_eq!(parse("make files a.py, b.py inside the pawline folder").unwrap().len(), 1);
    }
    #[test]
    fn names_with_in_stay_names() {
        assert_eq!(parse("make a folder called lost in space").unwrap(), vec![Directive::Create { kind: CreateKind::Dir, names: vec!["lost in space".into()], parent: LocationRef::CurrentDir }]);
    }
    #[test]
    fn nameless_file_asks_and_invents_nothing() {
        let m = q("make a file inside pawline");
        assert!(m.starts_with("What should the file be called?") && m.contains("inside pawline"), "{m}");
        let m = q("inside of pawline folder i want to have 3 files.py");
        assert!(m.contains("exact file names") && m.contains("what 3 means") && m.contains("inside pawline"), "{m}");
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    fn one(s: &str) -> Directive { parse(s).unwrap().remove(0) }
    #[test]
    fn provider_named_gives_a_directive_with_exact_prompt() {
        use crate::imagegen::ImageProvider::*;
        assert_eq!(one("generate a picture of a Red Lighthouse with pollinations"), Directive::GenerateImage { provider: Pollinations, prompt: "a Red Lighthouse".into() });
        assert_eq!(one("draw a cat using horde."), Directive::GenerateImage { provider: Horde, prompt: "a cat".into() });
        assert_eq!(one("generate an image of a fox via AI Horde"), Directive::GenerateImage { provider: Horde, prompt: "a fox".into() });
    }
    #[test]
    fn no_provider_asks_and_sends_nothing() {
        match one("generate an image of a dog") {
            Directive::Answer { text } => assert!(text.contains("pick one") && text.contains("Nothing was sent")),
            o => panic!("{o:?}"),
        }
        match one("generate a picture with horde") {
            Directive::Answer { text } => assert!(text.contains("What should the picture show")),
            o => panic!("{o:?}"),
        }
    }
    #[test]
    fn wallpaper_and_other_words_are_untouched() {
        assert!(!matches!(one("set wallpaper to ~/Pictures/a.png"), Directive::GenerateImage { .. }));
    }
}

#[cfg(test)]
mod skills_tests {
    use super::*;
    #[test]
    fn skills_lists_builtins_and_touches_nothing() {
        for q in ["/skills", "skills", "What skills do you have?"] {
            match parse(q).unwrap().remove(0) {
                Directive::Answer { text } => assert!(text.contains("Learned skills: none yet") && text.contains("/picture")),
                o => panic!("{o:?}"),
            }
        }
    }
}
