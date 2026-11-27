//! Experimental Laya intent router (opt-in, Linux only, session-only).
//!
//! A small local encoder model (vrajnotviraj/laya-intent-router-150m-onnx, Apache-2.0,
//! dequantized to fp32 for tract) that suggests a command when the deterministic parser
//! misses. It NEVER runs anything itself: a match becomes a "Did you mean" pick whose
//! accepted text re-enters the normal parser, so every existing gate (confirm cards,
//! validation, ambiguity chooser) applies unchanged. No network: the model files are
//! installed by the user under the Pawline data folder. English only. Rough cost once
//! warm: about a second per fallback on a fast CPU, several seconds on an old one,
//! ~700 MB RAM while loaded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tract_onnx::prelude::*;
use tract_onnx::tract_core::plan::SimplePlan;

type Runnable = std::sync::Arc<SimplePlan<TypedFact, Box<dyn TypedOp>>>;

/// Folder the user installs the model into: <pawline data>/laya with
/// laya_fp32.onnx, laya_fp32.onnx.data and tokenizer.json.
pub fn model_dir() -> Option<PathBuf> {
    pawline_core::platform::data_dir_from(&|k| std::env::var_os(k), cfg!(windows)).map(|d| d.join("laya"))
}

pub fn model_present(dir: &Path) -> bool {
    dir.join("laya_fp32.onnx").is_file()
        && dir.join("laya_fp32.onnx.data").is_file()
        && dir.join("tokenizer.json").is_file()
}

const INTENTS: &[(&str, &[&str])] = &[
    ("make_folder", &["make a folder", "create a new directory"]),
    ("make_file", &["make a file", "create a new file"]),
    ("delete", &["delete a file or folder", "remove something", "trash this file"]),
    ("rename", &["rename a file or folder", "change the name of a file"]),
    ("list_files", &["list the files in a folder", "show me what is in downloads"]),
    ("open_app", &["open an application", "launch firefox", "start a program"]),
    ("weather", &["what is the weather", "will it rain", "weather forecast for a city", "is it going to be hot", "do i need a coat today"]),
    ("math", &["calculate something", "solve an equation", "what is 2 plus 2", "solve for x"]),
    ("search", &["search for information", "look something up", "what is a black hole", "what is a file system", "how do magnets work", "why is the sky blue", "a general knowledge question"]),
    ("remind", &["remind me to do something", "set a reminder", "i keep forgetting to do something", "i need to remember to", "nudge me later"]),
    ("volume", &["turn up the volume", "set volume to 50", "mute the sound", "i cant hear anything", "the sound is too quiet", "make it louder"]),
    ("help", &["what can you do", "show me examples", "help me use you"]),
    // Decoys: absorb not-a-command talk that sits near command words. Treated as no match.
    ("chat_about_words", &["asking what a computing word means", "what does the word delete mean", "talking about a key on the keyboard", "discussing technology in the abstract"]),
    ("just_chatting", &["a story or opinion, not a request", "mentioning something without asking for it to be done", "talking about the weather in books or films"]),
];

/// Cheap keyword-family shortlist: the model only ever sees the few intents a message
/// can plausibly mean, plus both decoys and the none path. Keeps sequences short
/// (faster) and quality high (full phrasings fit the token window). Validated against
/// the 52-line battery: 9 benign misses, zero dangerous false accepts.
const FAMILIES: &[(&str, &[&str])] = &[
    (r"\b(folder|directory|mkdir)\b", &["make_folder"]),
    (r"\b(file|document|note)\b", &["make_file", "list_files"]),
    (r"\b(delet|remov|trash|erase|wipe)\w*", &["delete"]),
    (r"\b(rename|call it|change the name)\b", &["rename"]),
    (r"\b(list|show|what'?s in|downloaded|contents)\b", &["list_files"]),
    (r"\b(open|launch|start|run|firefox|spotify|app)\b", &["open_app"]),
    (r"\b(weather|rain|hot|cold|coat|forecast|umbrella|sunny|cloudy|wether)\b", &["weather"]),
    (r"\b(calculat|solve|equation|plus|minus|times|divided|math)\w*|\d\s*[-+*/^=]\s*\d|\d+x", &["math"]),
    (r"\b(what|why|how|who|explain|mean|means|meaning|is a|are a)\b", &["search", "chat_about_words"]),
    (r"\b(remind|remember|forget|nudge)\w*", &["remind"]),
    (r"\b(volume|sound|hear|loud|quiet|mute|deaf)\b", &["volume"]),
    (r"\b(help|examples|what can you do|use this|how do i use)\b", &["help"]),
];

const NONE_KEY: &str = "__none__";
const NONE_DESC: &str = "Gibberish, filler words, or a message unrelated to every other path.";
const INSTRUCTIONS: &str = "A user sent this message to a conversational workflow that branches into the paths below. Which path is the message asking for?";
const MAX_LEN: usize = 512;
const HEAD_MAX_LEN: usize = 384;
const OPTION_CAP: usize = 96;
const MIN_HEAD_TOKENS: usize = 16;
const PHRASING_SEP: &str = "\" | \"";
/// Non-destructive intents need this much confidence; destructive ones more (a confirm
/// card still follows either way - the threshold is about not *offering* nonsense).
const THRESHOLD: f32 = 0.80;
const THRESHOLD_DESTRUCTIVE: f32 = 0.90;
const CACHE_MAX: usize = 64;

pub struct Route {
    pub intent: &'static str,
    pub prob: f32,
}

pub struct Laya {
    model: Runnable,
    tok: tokenizers::Tokenizer,
    cache: HashMap<String, Option<Route>>,
    cls: i64,
    sep: i64,
    mask: i64,
}

fn narrow(text: &str) -> Vec<&'static str> {
    let t = text.to_lowercase();
    let mut win: Vec<&'static str> = Vec::new();
    for (pat, intents) in FAMILIES {
        if regex::Regex::new(pat).map(|re| re.is_match(&t)).unwrap_or(false) {
            for i in *intents {
                if !win.contains(i) {
                    win.push(i);
                }
            }
        }
    }
    win
}

/// Temperature table from the model's rl_agent_config.json (choice head, by option count).
fn temperature(n: usize) -> f32 {
    match n {
        0..=2 => 1.1014399528503418,
        3..=5 => 1.1109503507614136,
        6..=10 => 1.1113102436065674,
        _ => 1.0,
    }
}

/// Port of laya_intent_router._water_fill: share the head token budget fairly when the
/// options together do not fit (rare with the narrowed window, kept for parity).
fn water_fill(lengths: &[usize], mut budget: usize, floor: usize) -> Vec<usize> {
    let mut open: Vec<usize> = (0..lengths.len()).collect();
    while !open.is_empty() {
        let fair = budget / open.len();
        let short: Vec<usize> = open.iter().cloned().filter(|&i| lengths[i] <= fair).collect();
        if short.is_empty() {
            break;
        }
        budget -= short.iter().map(|&i| lengths[i]).sum::<usize>();
        open.retain(|&i| lengths[i] > fair);
    }
    let mut alloc = lengths.to_vec();
    for (j, &i) in open.iter().enumerate() {
        alloc[i] = (budget / open.len() + usize::from(j < budget % open.len())).max(floor);
    }
    alloc
}

/// Python json.dumps(ensure_ascii=False) quoting for one string (the router's state JSON).
fn json_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Laya {
    pub fn load(dir: &Path) -> Result<Laya, String> {
        if !model_present(dir) {
            return Err(format!("model files missing in {}", dir.display()));
        }
        let tok = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| format!("tokenizer: {e}"))?;
        let (cls, sep, mask) = (
            tok.token_to_id("[CLS]").ok_or("no [CLS] token")? as i64,
            tok.token_to_id("[SEP]").ok_or("no [SEP] token")? as i64,
            tok.token_to_id("[MASK]").ok_or("no [MASK] token")? as i64,
        );
        let model = tract_onnx::onnx()
            .model_for_path(dir.join("laya_fp32.onnx"))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("load: {e}"))?;
        Ok(Laya { model, tok, cache: HashMap::new(), cls, sep, mask })
    }

    fn enc(&self, s: &str) -> (Vec<u32>, Vec<(usize, usize)>) {
        let e = self.tok.encode(s.replace("[MASK]", " "), false).unwrap_or_else(|_| self.tok.encode("", false).unwrap());
        (e.get_ids().to_vec(), e.get_offsets().to_vec())
    }

    /// Route one message. None means "no suggestion": no family keyword, model not
    /// confident, a decoy won, or the none path won. Never errors the caller's flow.
    pub fn route(&mut self, message: &str) -> Option<Route> {
        let key = message.trim().to_lowercase();
        if let Some(hit) = self.cache.get(&key) {
            return hit.as_ref().map(|r| Route { intent: r.intent, prob: r.prob });
        }
        let out = self.route_inner(message);
        if self.cache.len() >= CACHE_MAX {
            self.cache.clear();
        }
        self.cache.insert(key, out.as_ref().map(|r| Route { intent: r.intent, prob: r.prob }));
        out
    }

    fn route_inner(&mut self, message: &str) -> Option<Route> {
        let mut win = narrow(message);
        if win.is_empty() {
            return None; // nothing command-like: skip the seconds-long inference entirely
        }
        for d in ["chat_about_words", "just_chatting"] {
            if !win.contains(&d) {
                win.push(d);
            }
        }
        let mut texts: Vec<String> = Vec::new();
        for name in &win {
            let (_, phrasings) = INTENTS.iter().find(|(n, _)| n == name)?;
            let joined = phrasings.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(PHRASING_SEP);
            texts.push(format!(" {name}: {joined}").replace("[MASK]", " "));
        }
        texts.push(format!(" {NONE_KEY}: {NONE_DESC}").replace("[MASK]", " "));
        let encs: Vec<(Vec<u32>, Vec<(usize, usize)>)> = texts.iter().map(|t| self.enc(t)).collect();
        let mut options: Vec<Vec<i64>> = encs
            .iter()
            .map(|(ids, _)| std::iter::once(self.mask).chain(ids.iter().take(OPTION_CAP).map(|&x| x as i64)).collect())
            .collect();
        let opt_total: usize = options.iter().map(|o| o.len()).sum();
        if opt_total > HEAD_MAX_LEN - MIN_HEAD_TOKENS {
            let lengths: Vec<usize> = options.iter().map(|o| o.len()).collect();
            let alloc = water_fill(&lengths, HEAD_MAX_LEN - MIN_HEAD_TOKENS, 4);
            for i in 0..options.len() {
                let a = alloc[i];
                let n = a.saturating_sub(1);
                if n >= options[i].len() - 1 {
                    continue;
                }
                let ends: Vec<usize> = encs[i].1.iter().map(|&(_, e)| e).collect();
                let mut keep = n;
                let t = &texts[i];
                let mut p = t.find(PHRASING_SEP).map(|x| x as i64).unwrap_or(-1);
                while p >= 0 {
                    let b = ends.iter().filter(|&&x| x <= (p as usize) + 1).count();
                    if b > n {
                        break;
                    }
                    if 4 * (n - b) <= a {
                        keep = b;
                    }
                    p = t[(p as usize) + 1..].find(PHRASING_SEP).map(|x| x + (p as usize) + 1).map(|x| x as i64).unwrap_or(-1);
                }
                options[i] = std::iter::once(self.mask).chain(encs[i].0.iter().take(keep).map(|&x| x as i64)).collect();
            }
        }
        let opt_total: usize = options.iter().map(|o| o.len()).sum();
        let head_budget = HEAD_MAX_LEN.saturating_sub(opt_total).max(8);
        let (hids, _) = self.enc(&format!("choice question: {INSTRUCTIONS}"));
        let head_len = hids.len().min(head_budget);
        let mut ids: Vec<i64> = vec![self.cls];
        ids.extend(hids[..head_len].iter().map(|&x| x as i64));
        ids.push(self.sep);
        let mut markers: Vec<i64> = Vec::new();
        for o in &options {
            markers.push(ids.len() as i64);
            ids.extend_from_slice(o);
        }
        ids.push(self.sep);
        let state = format!("{{\"message\": {}}}", json_quote(message));
        let room = MAX_LEN.saturating_sub(ids.len() + 1);
        let (st, _) = self.enc(&state);
        ids.extend(st[..st.len().min(room)].iter().map(|&x| x as i64));
        ids.push(self.sep);
        ids.truncate(MAX_LEN);
        if markers.iter().any(|&m| m as usize >= MAX_LEN) {
            return None;
        }
        let seq = ids.len();
        let nmark = markers.len();
        let maskv: Vec<i64> = vec![1; seq];
        let mmask: Vec<bool> = vec![true; nmark];
        let qt = Tensor::from_shape(&[1usize], &[0i64]).ok()?;
        let out = self
            .model
            .run(tvec![
                Tensor::from_shape(&[1, seq], &ids).ok()?.into(),
                Tensor::from_shape(&[1, seq], &maskv).ok()?.into(),
                Tensor::from_shape(&[1, nmark], &markers).ok()?.into(),
                Tensor::from_shape(&[1, nmark], &mmask).ok()?.into(),
                qt.into(),
            ])
            .ok()?;
        let logits = out[0].to_plain_array_view::<f32>().ok()?;
        let temp = temperature(nmark);
        let mx = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = logits.iter().map(|x| ((x - mx) / temp).exp()).collect();
        let sum: f32 = exps.iter().sum();
        let probs: Vec<f32> = exps.iter().map(|x| x / sum).collect();
        let (bi, bp) = probs.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(i, p)| (i, *p))?;
        if bi == probs.len() - 1 {
            return None; // the none path won
        }
        let intent: &'static str = win[bi];
        if matches!(intent, "chat_about_words" | "just_chatting") {
            return None; // decoys absorb not-a-command talk
        }
        let need = if matches!(intent, "delete" | "rename") { THRESHOLD_DESTRUCTIVE } else { THRESHOLD };
        let none_p = probs[probs.len() - 1];
        if bp <= none_p || bp < need {
            return None;
        }
        Some(Route { intent, prob: bp })
    }
}

/// Build the "Did you mean" suggestion for a routed intent: a normalized command whose
/// pick refills the input and re-enters the normal parser (and its confirm cards).
/// Partial templates end in a space so the user completes the missing piece.
pub fn suggest(intent: &str, message: &str) -> Option<String> {
    let m = message.trim();
    let low = m.to_lowercase();
    let cap = |pats: &[&str]| -> Option<String> {
        for p in pats {
            if let Some(c) = regex::Regex::new(&format!("(?i){p}")).ok().and_then(|re| re.captures(m)) {
                if let Some(g) = c.get(1) {
                    let v: String = g
                        .as_str()
                        .trim()
                        .trim_matches(['"', '\'', '.', '!', '?'])
                        .trim_end_matches(['"', '\''])
                        .trim()
                        .trim_end_matches(" please")
                        .trim_end_matches(" Please")
                        .trim_end_matches(" PLEASE")
                        .trim_end_matches(" pls")
                        .trim_end_matches(" Pls")
                        .trim()
                        .to_string();
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
        None
    };
    let suggestion = match intent {
        "make_folder" => match cap(&[r#"(?:called|named)\s+"([^"]+)""#, r"(?:called|named)\s+(.+?)$"]) {
            Some(n) => format!("make a folder called {n}"),
            None => "make a folder called ".into(),
        },
        "make_file" => match cap(&[r#"(?:called|named)\s+"([^"]+)""#, r"(?:called|named)\s+(.+?)$"]) {
            Some(n) => format!("make a file called {n}"),
            None => "make a file called ".into(),
        },
        "delete" => match cap(&[r#""([^"]+)""#, r"(?:file|folder)\s+([\w.\- ]+?)\s*(?:please|pls)?$"]) {
            Some(n) if low.contains("folder") => format!("delete the folder {n}"),
            Some(n) => format!("delete the file {n}"),
            None => "delete the file ".into(),
        },
        "rename" => "rename ".into(),
        "list_files" => match cap(&[r"\bin\s+([\w.\-/ ]+?)\s*(?:please|pls)?$"]) {
            Some(n) => format!("list files in {n}"),
            None => "list files".into(),
        },
        "open_app" => {
            let ws: Vec<&str> = low.split_whitespace().collect();
            let at = ws.iter().position(|w| matches!(*w, "open" | "launch" | "start" | "run"));
            match at {
                Some(i) if i + 1 < ws.len() => format!("open {}", ws[i + 1..].join(" ").trim_end_matches(" please")),
                _ => "open ".into(),
            }
        }
        "weather" => match cap(&[r"\bin\s+([a-z ]+?)\s*(?:tomorrow|today|please|pls|\?|$)"]) {
            Some(c) => format!("weather in {c}"),
            None => "weather".into(),
        },
        "math" => "what is ".into(),
        "search" => match cap(&[r"^(?:what|why|how|who)\b.+$"]) {
            Some(_) => format!("search for {m}"),
            None => "search for ".into(),
        },
        "remind" => match cap(&[r"(?:remember to|remind me to|nudge me to)\s+(.+?)$"]) {
            Some(t) => format!("remind me to {t}"),
            None => "remind me to ".into(),
        },
        "volume" => {
            if let Some(n) = cap(&[r"(\d{1,3})\s*%"]) {
                format!("set volume to {n}")
            } else if low.contains("mute") {
                "mute the volume".into()
            } else if low.contains("loud") || low.contains("up") || low.contains("hear") {
                "turn the volume up".into()
            } else {
                "turn the volume down".into()
            }
        }
        "help" => "help".into(),
        _ => return None,
    };
    Some(suggestion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_picks_families_plus_nothing_yet() {
        assert_eq!(narrow("please delete the file x"), vec!["make_file", "list_files", "delete"]);
        assert_eq!(narrow("open firefox"), vec!["open_app"]);
        assert_eq!(narrow("wether in cairo"), vec!["weather"]);
        assert_eq!(narrow("what is a black hole"), vec!["search", "chat_about_words"]);
        assert!(narrow("how are you doing").is_empty() || narrow("how are you doing") == vec!["search", "chat_about_words"]);
        assert!(narrow("zzzz qqqq").is_empty());
    }

    #[test]
    fn suggest_extracts_slots_without_touching_case() {
        assert_eq!(suggest("make_folder", "make me a folder called Smoke1 please").as_deref(), Some("make a folder called Smoke1"));
        assert_eq!(suggest("make_folder", "create a folder").as_deref(), Some("make a folder called "));
        assert_eq!(suggest("remind", "i need to remember to drink Water").as_deref(), Some("remind me to drink Water"));
        assert_eq!(suggest("weather", "whats the weather like in Cairo tomorrow").as_deref(), Some("weather in Cairo"));
        assert_eq!(suggest("open_app", "can you open firefox").as_deref(), Some("open firefox"));
        assert_eq!(suggest("volume", "set the volume to 40%").as_deref(), Some("set volume to 40"));
        assert_eq!(suggest("delete", "delete \"old notes.txt\"").as_deref(), Some("delete the file old notes.txt"));
        assert_eq!(suggest("delete", "delete the folder junk").as_deref(), Some("delete the folder junk"));
    }

    #[test]
    fn water_fill_matches_reference_allocations() {
        // 15 options over budget: small ones keep full length, the big ones share the rest.
        let lens = vec![17, 17, 22, 20, 22, 21, 42, 30, 52, 41, 44, 22, 45, 42, 21];
        let alloc = water_fill(&lens, 384 - 16, 4);
        assert_eq!(&alloc[..6], &[17, 17, 22, 20, 22, 21]);
        assert_eq!(alloc[11], 22);
        assert_eq!(alloc[14], 21);
        assert!(alloc.iter().sum::<usize>() <= 384 - 16 + 8); // fair share plus rounding slack
        assert!(alloc[8] < 52);
    }
}
