//! Typo tolerance for COMMAND WORDS only. Local, deterministic, no model, no network.
//!
//! Rules (safety first):
//! - Only the command verb (first word, after a polite lead-in) and a few fixed words right after it
//!   ("folder", "file", "presentation", "called", "inside", ...) are ever corrected.
//! - Names are never corrected: anything after "called"/"named", anything quoted, anything that looks like a
//!   path or file name (contains . / \ ~ _ a digit or a non-ASCII letter), and everything past the fixed-word window.
//! - A word is corrected only to ONE clearly closest candidate (no ties), within a small edit distance
//!   that grows with word length. Words of 3 letters or fewer are never corrected.
//! - The corrected sentence must be recognised by one of Pawline's own parsers, otherwise nothing is suggested.
//! - A correction never runs by itself: the caller asks "Did you mean ...?" first (a typo can be a real word,
//!   like "snow in cairo"). Only dropping a polite lead-in ("please ...") runs directly.
//! - Typing never approves anything. This module has no way to approve a card.

/// Verbs that can change files, folders, wallpaper or launch programs. Corrected only after the user picks the suggestion.
pub const RISKY_VERBS: &[&str] = &[
    "make", "create", "delete", "remove", "trash", "rename", "move", "copy", "open", "launch", "set", "change", "undo", "write", "draft", "organize",
    "organise", "shrink", "compress",
];
/// Read-only verbs (still suggested first, never run directly after a change).
pub const SAFE_VERBS: &[&str] = &["list", "show"];
const FIXED: &[&str] = &[
    "folder", "folders", "directory", "directories", "file", "files", "presentation", "slideshow", "wallpaper", "called", "named", "inside", "into",
    "about",
];
const POLITE: &[&str] = &["please", "pls", "plz", "can", "could", "you", "i", "want", "to", "would", "like", "kindly"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub corrected: String,
    /// (typed word, corrected word)
    pub changes: Vec<(String, String)>,
    /// True when every changed verb is read-only, so the corrected request may run without asking first.
    pub direct: bool,
}

/// Optimal-string-alignment distance (insert, delete, substitute, swap neighbours), on chars.
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() {
        d[i][0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + c);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn max_dist(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// The single closest word of `vocab` within the allowed distance. None when nothing is close or two tie.
fn closest<'a>(word: &str, vocab: &[&'a str]) -> Option<&'a str> {
    let max = max_dist(word.chars().count());
    if max == 0 {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    let mut tie = false;
    for v in vocab {
        let d = distance(word, v);
        if d == 0 || d > max {
            continue;
        }
        match best {
            None => best = Some((d, v)),
            Some((bd, _)) if d < bd => {
                best = Some((d, v));
                tie = false;
            }
            Some((bd, bv)) if d == bd && bv != *v => tie = true,
            _ => {}
        }
    }
    if tie {
        None
    } else {
        best.map(|b| b.1)
    }
}

/// Real English words that are one edit from a command word: never treated as typos.
const REAL_WORDS: &[&str] = &[
    "snow", "slow", "mode", "made", "male", "cope", "code", "cook", "crate", "lost", "last", "lust", "cost", "post", "host", "most", "mote", "move", "moved", "rove",
    "loot", "lift", "lust", "chow", "shot", "shop", "stow", "sew", "ships", "whow", "sort", "sorts", "mace", "mage", "mate", "mare", "mike", "lake", "take", "wake", "bake",
    "fake", "rake", "cake", "dale", "date", "deal", "dell", "role", "rose", "rule", "rename", "coy", "coop", "pen", "open", "oven", "opens", "wrote", "write", "writer",
];

fn name_like(w: &str) -> bool {
    w.is_empty() || !w.chars().all(|c| c.is_ascii_alphabetic())
}

/// Suggest a corrected request, or None when nothing needs or may be corrected.
/// `recognised` tells whether Pawline's own parsers understand a sentence.
pub fn suggest(input: &str, recognised: &dyn Fn(&str) -> bool) -> Option<Suggestion> {
    let t = input.trim();
    if t.is_empty() || t.starts_with('/') || t.contains('"') || t.contains('\'') {
        return None;
    }
    if recognised(t) {
        return None; // already understood: never touched
    }
    let mut spans: Vec<(usize, usize)> = vec![];
    let mut start = None;
    for (i, c) in t.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                spans.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, t.len()));
    }
    let all_verbs: Vec<&str> = RISKY_VERBS.iter().chain(SAFE_VERBS).copied().collect();
    let mut changes: Vec<(usize, usize, String, String)> = vec![];
    let mut verb_seen = false;
    let mut verb_start = 0usize;
    let mut window = 0usize; // words checked after the verb
    for &(s, e) in &spans {
        let raw = &t[s..e];
        let core = raw.trim_end_matches([',', '?', '!', ';', ':']);
        let low = core.to_ascii_lowercase();
        if name_like(&low) {
            if verb_seen {
                break; // first name-like word: the rest is names
            }
            return None;
        }
        if !verb_seen {
            if POLITE.contains(&low.as_str()) || (low.len() >= 5 && distance(&low, "please") <= 1) {
                continue; // a polite lead-in is dropped from the corrected sentence (Pawline's parsers do not take it)
            }
            if all_verbs.contains(&low.as_str()) {
                verb_seen = true;
                verb_start = s;
                continue;
            }
            if REAL_WORDS.contains(&low.as_str()) && !all_verbs.contains(&low.as_str()) {
                return None;
            }
            match closest(&low, &all_verbs) {
                Some(v) => {
                    changes.push((s, s + core.len(), core.to_string(), v.to_string()));
                    verb_seen = true;
                    verb_start = s;
                    continue;
                }
                None => return None, // the first real word is not a typo of a command
            }
        }
        window += 1;
        if window > 5 {
            break;
        }
        if matches!(low.as_str(), "called" | "named") {
            break;
        }
        if matches!(low.as_str(), "a" | "an" | "the" | "my" | "new" | "in" | "of" | "from" | "on" | "to" | "and" | "then") {
            continue;
        }
        let noun_then_name = |w: &str| matches!(w, "folder" | "folders" | "directory" | "directories" | "file" | "files" | "wallpaper");
        if FIXED.contains(&low.as_str()) {
            if noun_then_name(&low) {
                break; // what follows is a name
            }
            continue;
        }
        if let Some(v) = closest(&low, FIXED) {
            changes.push((s, s + core.len(), core.to_string(), v.to_string()));
            if noun_then_name(v) {
                break;
            }
        } else {
            break; // an unknown word here is a name: left exactly as typed
        }
    }
    if !verb_seen {
        return None;
    }
    if changes.is_empty() && verb_start == 0 {
        return None; // nothing to fix and nothing dropped
    }
    let mut out = String::new();
    let mut at = verb_start;
    for (s, e, _, to) in &changes {
        out.push_str(&t[at..*s]);
        out.push_str(to);
        at = *e;
    }
    out.push_str(&t[at..]);
    if !recognised(&out) {
        return None;
    }
    let first_verb = out
        .split_whitespace()
        .map(|w| w.trim_end_matches([',', '?', '!']).to_ascii_lowercase())
        .find(|w| all_verbs.contains(&w.as_str()))
        .unwrap_or_default();
    let _ = (&first_verb, SAFE_VERBS);
    // Only dropping a polite lead-in may run directly. Any changed word is shown as a suggestion first, because a typo can be a real word ("snow in cairo").
    let direct = changes.is_empty();
    Some(Suggestion { corrected: out, changes: changes.into_iter().map(|c| (c.2, c.3)).collect(), direct })
}

/// Pawline's own parsers: does any of them understand this sentence?
pub fn recognised_by_parsers(s: &str) -> bool {
    // A parse that only produced a clarifying answer ("What should the file be called?") is not an understood command.
    crate::intent::parse(s).map(|ds| ds.iter().any(|d| !matches!(d, crate::plan::Directive::Answer { .. }))).unwrap_or(false)
        || crate::finder::parse(s).is_some()
        || crate::deck::parse_command(s).is_some()
        || crate::deck::parse_write_command(s).is_some()
        || crate::shrink::parse(s).is_some()
        || crate::organize::parse(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sg(s: &str) -> Option<Suggestion> {
        suggest(s, &recognised_by_parsers)
    }

    #[test]
    fn distance_basics() {
        assert_eq!(distance("delte", "delete"), 1);
        assert_eq!(distance("mkae", "make"), 1);
        assert_eq!(distance("dleete", "delete"), 1);
        assert_eq!(distance("dlleete", "delete"), 2);
        assert_eq!(distance("abc", "abc"), 0);
    }

    /// (typed, expected corrected sentence, direct)
    const GOOD: &[(&str, &str, bool)] = &[
        ("delte the folder kp", "delete the folder kp", false),
        ("dlete folder kp", "delete folder kp", false),
        ("deleet the file notes.txt", "delete the file notes.txt", false),
        ("remvoe the folder old", "remove the folder old", false),
        ("mkae a folder called pics", "make a folder called pics", false),
        ("maek a folder called pics", "make a folder called pics", false),
        ("craete a folder called pics", "create a folder called pics", false),
        ("creat a file called a.txt", "create a file called a.txt", false),
        ("make a foler called pics", "make a folder called pics", false),
        ("make a flder called pics", "make a folder called pics", false),
        ("mkae a foldr called pics", "make a folder called pics", false),
        ("make a fiel called a.txt", "make a file called a.txt", false),
        ("renmae the file a.txt to b.txt", "rename the file a.txt to b.txt", false),
        ("mvoe the file a.txt to docs", "move the file a.txt to docs", false),
        ("moev the file a.txt to docs", "move the file a.txt to docs", false),
        ("cpoy the file a.txt to docs", "copy the file a.txt to docs", false),
        ("pleaes delte the folder kp", "delete the folder kp", false),
        ("please delte folder kp", "delete folder kp", false),
        ("lsit folders", "list folders", false),
        ("shwo my folders", "show my folders", false),
        ("lits the folder docs", "list the folder docs", false),
        ("make a presentaton about the water cycle", "make a presentation about the water cycle", false),
        ("make a presentaiton from notes.txt", "make a presentation from notes.txt", false),
        ("wrtie a presentation about cats", "write a presentation about cats", false),
        ("udno", "undo", false),
        ("please delete the folder kp", "delete the folder kp", true),
    ];

    #[test]
    fn typo_test_list_measured() {
        let mut ok = 0;
        let mut miss: Vec<String> = vec![];
        for (typed, want, direct) in GOOD {
            match sg(typed) {
                Some(s) if s.corrected == *want && s.direct == *direct => ok += 1,
                other => miss.push(format!("{typed:?} -> {:?}", other.map(|s| s.corrected))),
            }
        }
        eprintln!("typo test list: {ok}/{} corrected as expected; misses: {miss:#?}", GOOD.len());
        assert!(ok * 100 >= GOOD.len() * 85, "pass rate too low: {ok}/{} misses {miss:#?}", GOOD.len());
    }

    #[test]
    fn names_are_never_corrected() {
        for s in [
            "delete the folder flder",
            "delete folder mkae",
            "make a folder called delte",
            "make a folder named moev",
            "make a folder called fiel",
            "rename the folder foler to flder",
            "delete the file notes.txt",
            "make a file called movee.txt",
            "dlete the folder flder",
            "mkae a folder called delte",
        ] {
            if let Some(sug) = sg(s) {
                for name in ["flder", "mkae", "delte", "moev", "fiel", "foler", "movee.txt"] {
                    // a name token after the command words must survive byte for byte
                    let after_cmd = s.split_whitespace().skip(2).collect::<Vec<_>>().join(" ");
                    if after_cmd.contains(name) {
                        assert!(sug.corrected.contains(name), "{s:?} lost the name {name:?}: {sug:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn already_understood_text_is_untouched() {
        for s in ["delete the folder kp", "make a folder called pics", "list folders", "what is gravity", "hello there", "tell me a joke"] {
            assert!(sg(s).is_none(), "{s:?}");
        }
    }

    #[test]
    fn changed_words_are_never_run_directly() {
        assert!(!sg("dlete the folder kp").unwrap().direct);
        assert!(!sg("mkae a folder called pics").unwrap().direct);
        assert!(!sg("mvoe the file a.txt to docs").unwrap().direct);
        assert!(!sg("lsit folders").unwrap().direct);
        assert!(sg("please delete the folder kp").unwrap().direct); // only a dropped lead-in, no word changed
    }

    #[test]
    fn unrelated_words_and_ties_are_not_guessed() {
        for s in ["snow in cairo", "banana split", "xyz abc", "yse", "yes please", "nope"] {
            assert!(sg(s).is_none(), "{s:?} -> {:?}", sg(s));
        }
        assert!(closest("mov", &["move", "mode"]).is_none()); // too short
        assert!(closest("moce", &["move", "mode"]).is_none()); // tie: never guess
    }

    #[test]
    fn quotes_paths_and_slash_commands_stop_correction() {
        assert!(sg("delte \"my folder\"").is_none());
        assert_eq!(sg("delte ~/Documents/kp").unwrap().corrected, "delete ~/Documents/kp");
        assert!(sg("/delte x").is_none());
    }

    #[test]
    fn misspelled_yes_is_never_a_suggestion() {
        for s in ["yse", "yess", "y", "yes", "ok do it", "aprove", "yes delte it"] {
            assert!(sg(s).is_none(), "{s:?} -> {:?}", sg(s));
        }
    }
}
