//! Optional online answer for a plain question Pawline can't answer itself.
//! Policy (the user's): automatic ONLY for clearly general questions; anything
//! that might carry private details asks first; nothing is ever attached
//! (no files, no history); the reply is shown, never executed; no payment.

use crate::error::PawError;
use crate::imagegen::Http;

pub const MAX_QUESTION_CHARS: usize = 300;
pub const MAX_ANSWER_CHARS: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Clearly general: may be sent without asking.
    Auto,
    /// Question-shaped but not clearly general or possibly private: ask first.
    Ask,
    /// Not a question, or a failed local command: not sent anywhere.
    No,
    /// Looks like it carries a secret value. Never sent, and there is no way to approve it.
    Block,
}

const AUTO_STARTS: &[&str] = &[
    "what is ", "what are ", "what's ", "what does ", "how does ", "how do ", "why is ", "why do ", "why does ",
    "explain ", "define ", "who invented ", "who discovered ", "when was ",
];
const QUESTION_STARTS: &[&str] = &[
    "what", "who", "whom", "when", "where", "why", "how", "which", "is ", "are ", "was ", "were ", "do ", "does ", "did ",
    "can ", "could ", "should ", "would ", "will ", "explain", "tell me", "define", "describe",
];
const PRIVATE_WORDS: &[&str] = &[
    "my", "mine", "me", "i", "i'm", "i've", "i'd", "our", "ours", "we", "apikey",
    "credit", "card", "bank", "iban", "phone", "ssid", "ssn", "login", "pin", "otp", "salary", "boss",
    "wife", "husband", "girlfriend", "boyfriend", "home", "file", "files", "folder", "folders", "desktop", "documents",
    "downloads", "pictures", "history", "clipboard",
];

/// Words that may appear in an AUTO question's topic. Anything outside this list (company or
/// client names, streets, medicines, people, project names, odd identifiers) is not
/// recognisable as general knowledge, so the question asks first instead.
const GENERAL_WORDS: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "to", "and", "or", "is", "are", "work", "works", "mean", "means", "happen", "happens", "form", "forms",
    "black", "hole", "holes", "star", "stars", "sun", "moon", "earth", "planet", "planets", "galaxy", "universe", "atom", "atoms", "molecule",
    "cell", "cells", "dna", "gene", "genes", "virus", "bacteria", "protein", "energy", "force", "gravity", "light", "sound", "heat", "electricity",
    "magnet", "magnets", "magnetism", "wave", "waves", "radio", "laser", "lasers", "battery", "batteries", "circuit", "transistor", "engine",
    "motor", "rocket", "satellite", "telescope", "microscope", "telephone", "computer", "computers", "internet", "algorithm", "algorithms",
    "recursion", "database", "network", "protocol", "encryption", "compiler", "kernel", "operating", "system", "memory", "processor", "cpu",
    "gpu", "software", "hardware", "program", "programming", "language", "variable", "function", "loop", "array", "binary", "bit", "byte",
    "photosynthesis", "evolution", "gravity", "inflation", "entropy", "democracy", "economy", "economics", "supply", "demand", "market",
    "money", "tax", "taxes", "interest", "climate", "weather", "rain", "snow", "cloud", "clouds", "wind", "storm", "tornado", "hurricane",
    "earthquake", "volcano", "volcanoes", "ocean", "oceans", "sea", "river", "rivers", "mountain", "mountains", "desert", "forest", "ice",
    "glacier", "tide", "tides", "sky", "blue", "red", "green", "sunset", "rainbow", "season", "seasons", "year", "leap", "day", "night",
    "water", "air", "oxygen", "hydrogen", "carbon", "dioxide", "nitrogen", "helium", "iron", "gold", "silver", "copper", "metal", "plastic",
    "glass", "paper", "rock", "rocks", "sand", "soil", "mineral", "crystal", "diamond", "salt", "sugar", "acid", "base", "ph", "element",
    "periodic", "table", "chemistry", "physics", "biology", "geology", "astronomy", "mathematics", "math", "algebra", "geometry", "calculus",
    "prime", "number", "numbers", "fraction", "percent", "probability", "statistics", "triangle", "circle", "square", "pi", "zero", "infinity",
    "clock", "time", "calendar", "history", "ancient", "roman", "empire", "pyramid", "pyramids", "renaissance", "revolution", "war",
    "telescope", "printing", "press", "wheel", "electricity", "radio", "bridge", "bridges", "dam", "tower", "airplane", "plane", "flight",
    "fly", "flies", "bird", "birds", "fish", "whale", "whales", "dolphin", "shark", "sharks", "bee", "bees", "ant", "ants", "spider", "insect",
    "insects", "butterfly", "dinosaur", "dinosaurs", "mammal", "reptile", "animal", "animals", "plant", "plants", "tree", "trees", "flower",
    "seed", "seeds", "fungus", "mushroom", "coral", "reef", "ecosystem", "food", "chain", "web", "digestion", "heart", "blood", "brain", "lung",
    "lungs", "bone", "bones", "muscle", "muscles", "immune", "sleep", "dream", "dreams", "vaccine", "vaccines", "antibiotic", "antibiotics",
    "password", "passwords", "manager", "firewall", "vpn", "phishing", "malware", "backup", "cloud", "wifi", "bluetooth", "gps", "email",
    "linux", "unix", "git", "http", "https", "dns", "ip", "address", "url", "browser", "wayland", "x11", "pixel", "resolution", "opengl",
    "good", "bad", "big", "small", "fast", "slow", "hot", "cold", "old", "new", "strong", "weak", "long", "short", "high", "low", "round",
    "flat", "so", "why", "how", "what", "when", "who", "does", "do", "it", "its", "that", "this", "there", "have", "has", "make", "makes",
    "made", "not", "no", "between", "difference", "meaning", "idea", "theory", "law", "laws", "rule", "rules", "concept", "principle",
];
const LOCAL_WORDS: &[&str] = &[
    "make", "create", "delete", "remove", "rename", "move", "copy", "list", "show", "open", "launch", "run", "set", "change", "wallpaper",
    "undo", "folder", "directory", "file", "files", "app", "apps", "install", "uninstall", "save", "download", "upload", "write", "edit",
];

fn topic_is_general(lower: &str, starter_len: usize) -> bool {
    let rest = &lower[starter_len..];
    let w = words(rest);
    !w.is_empty() && w.len() <= 5 && w.iter().all(|x| GENERAL_WORDS.contains(&x.as_str()))
}

/// Does the text carry something that looks like a secret value (not a question ABOUT passwords)?
pub fn looks_like_secret(input: &str) -> bool {
    let lower = input.to_lowercase();
    if lower.contains("-----begin") || lower.contains("bearer ") {
        return true;
    }
    for pre in ["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "akia", "eyj", "glpat-", "aiza"] {
        if lower.split_whitespace().any(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').starts_with(pre)) {
            return true;
        }
    }
    for pat in ["password is", "passcode is", "pin is", "token is", "api key is", "secret is", "key is", "password:", "passcode:", "token:", "secret:", "api key:", "api_key", "password=", "passwd", "pwd=", "token="] {
        if lower.contains(pat) {
            return true;
        }
    }
    for t in input.split_whitespace() {
        let t = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '+' && c != '/' && c != '=');
        let n = t.chars().count();
        let has_l = t.chars().any(|c| c.is_ascii_alphabetic());
        let has_d = t.chars().any(|c| c.is_ascii_digit());
        if n >= 32 || (n >= 20 && has_l && has_d) {
            return true;
        }
    }
    let digits: String = input.chars().filter(|c| c.is_ascii_digit()).collect();
    let only_sep = input.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-' || c.is_alphabetic() || c.is_ascii_punctuation());
    only_sep && digits.len() >= 13 && input.split(|c: char| !(c.is_ascii_digit() || c == ' ' || c == '-')).any(|seg| seg.chars().filter(|c| c.is_ascii_digit()).count() >= 13)
}

fn words(lower: &str) -> Vec<String> {
    lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '_'))
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

fn has_private_marker(input: &str) -> bool {
    private_marker(input, false)
}

/// `chat` = a casual chat line: personal pronouns and names are allowed (the exact text is shown on a card first); paths, addresses,
/// numbers, files and account words still block.
/// True when the part after the last dot looks like a real file extension.
fn known_ext(t: &str) -> bool {
    const EXT: &[&str] = &["txt", "md", "pdf", "doc", "docx", "odt", "xls", "xlsx", "ods", "ppt", "pptx", "odp", "csv", "json", "xml", "html", "htm", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp3", "wav", "ogg", "mp4", "mkv", "zip", "tar", "gz", "7z", "rs", "py", "js", "ts", "c", "cpp", "h", "sh", "toml", "yaml", "yml", "log", "conf", "ini", "deb", "iso", "exe", "bin", "key", "pem", "db", "sql"];
    t.rsplit('.').next().is_some_and(|e| EXT.contains(&e.to_lowercase().as_str()))
}

fn private_marker(input: &str, chat: bool) -> bool {
    let lower = input.to_lowercase();
    if lower.contains('/') || lower.contains('\\') || lower.contains('~') || lower.contains('@') || lower.contains("://") || lower.contains("www.") {
        return true;
    }
    // file-like token: word.ext
    if input.split_whitespace().any(|t| {
        let t = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '.');
        t.contains('.') && !t.starts_with('.') && !t.ends_with('.') && t.rsplit('.').next().is_some_and(|e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
            // chat: decimals (3.5), abbreviations (e.g.) and run-together sentences are not file names
            && !(chat && (t.rsplit('.').next().is_some_and(|e| e.chars().all(|c| c.is_ascii_digit())) || t.split('.').any(|p| p.len() <= 1) || !known_ext(t)))
    }) {
        return true;
    }
    let mut run = 0;
    for c in input.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= 5 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    let w = words(&lower);
    // chat: only credential and account words block; pronouns, family, and plain nouns like "file" or "history" are the user's own talk
    const CHAT_BLOCK: &[&str] = &["apikey", "credit", "card", "bank", "iban", "ssid", "ssn", "login", "pin", "otp", "salary", "clipboard", "password", "passwords", "passcode"];
    if chat {
        if w.iter().any(|x| CHAT_BLOCK.contains(&x.as_str())) {
            return true;
        }
    } else if w.iter().any(|x| PRIVATE_WORDS.contains(&x.as_str())) {
        return true;
    }
    if chat {
        return input.contains('`');
    }
    // quoted text and mid-sentence capitalised words (names, places) may be private
    if input.contains('"') || input.contains('`') {
        return true;
    }
    input.split_whitespace().skip(1).any(|t| t.chars().next().is_some_and(|c| c.is_uppercase()) && t != "I")
}

pub fn classify(input: &str) -> Class {
    let t = input.trim();
    if t.is_empty() {
        return Class::No;
    }
    let lower = t.to_lowercase();
    let questiony = t.ends_with('?') || QUESTION_STARTS.iter().any(|p| lower.starts_with(p));
    if !questiony {
        return Class::No;
    }
    // A secret value is never sent, whatever else the text says.
    if looks_like_secret(t) {
        return Class::Block;
    }
    // A failed local command is not a question for the internet.
    if words(&lower).iter().any(|w| LOCAL_WORDS.contains(&w.as_str())) {
        return Class::No;
    }
    if t.chars().count() > MAX_QUESTION_CHARS || t.chars().any(|c| c.is_control()) {
        return Class::Ask;
    }
    if has_private_marker(t) {
        return Class::Ask;
    }
    match AUTO_STARTS.iter().find(|p| lower.starts_with(**p)) {
        Some(p) if topic_is_general(&lower, p.len()) => Class::Auto,
        _ => Class::Ask,
    }
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn pollinations_text_url(question: &str) -> String {
    format!("https://text.pollinations.ai/{}", enc(question))
}

/// Ask Pollinations. Only the question text is sent. `commit` is claimed just before the answer is returned.
pub fn ask(http: &dyn Http, question: &str, cancelled: &dyn Fn() -> bool, commit: &dyn Fn() -> bool) -> Result<String, PawError> {
    let q: String = question.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() || q.chars().count() > MAX_QUESTION_CHARS {
        return Err(PawError::Validation(format!("A question must be 1 to {MAX_QUESTION_CHARS} characters. I won't cut it for you. Nothing was sent.")));
    }
    let r = http.get(&pollinations_text_url(&q), &[]).map_err(|_| {
        PawError::Exec("Pollinations could not be reached right now (no connection, or the service is down). There is no answer, and Pawline did not retry. Check your connection and ask again later.".into())
    })?;
    if cancelled() || !commit() {
        return Err(PawError::Exec("Cancelled. Nothing was kept, and a late answer will be discarded.".into()));
    }
    match r.status {
        200 => {
            let text = String::from_utf8_lossy(&r.body);
            let clean: String = text.chars().filter(|c| !c.is_control() || *c == '\n').collect();
            let clean = clean.trim();
            if clean.is_empty() {
                return Err(PawError::Exec("Pollinations sent an empty answer.".into()));
            }
            let n = clean.chars().count();
            let shown: String = clean.chars().take(MAX_ANSWER_CHARS).collect();
            Ok(if n > MAX_ANSWER_CHARS { format!("{shown}\n(shortened: {n} characters received)") } else { shown })
        }
        402 | 429 => Err(PawError::Exec(format!(
            "Pollinations said its free limit is reached right now (HTTP {}).{} I never pay or upgrade, so there is no answer.",
            r.status,
            r.retry_after.map(|s| format!(" Try again in about {s} seconds.")).unwrap_or_default()
        ))),
        s => Err(PawError::Exec(format!("Pollinations answered HTTP {s}; no answer."))),
    }
}

pub const CHAT_SYSTEM: &str = "You are Pawline, a friendly desktop helper. Reply briefly in plain text. You cannot run commands or access files.";
pub const CHAT_LINE_CHARS: usize = 300;
/// Earlier lines of the session that are sent along (3 exchanges at most).
pub const CHAT_CONTEXT_LINES: usize = 6;

/// Why a chat message must not be sent (None = may be offered for sending after the user confirms).
pub fn chat_block_reason(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.is_empty() { return Some("empty"); }
    if t.chars().count() > CHAT_LINE_CHARS { return Some("longer than 300 characters"); }
    if t.chars().any(|c| c.is_control()) { return Some("contains control characters"); }
    if looks_like_secret(t) { return Some("looks like a password or secret"); }
    if private_marker(t, true) { return Some("looks like it names a path, address, number or private detail"); }
    // A local command that failed to parse is not chat: an action word at the start with a file-ish target.
    let lw = words(&t.to_lowercase());
    const TARGETS: &[&str] = &["file", "files", "folder", "folders", "directory", "directories", "app", "apps", "wallpaper", "document", "documents"];
    if lw.iter().take(3).any(|w| LOCAL_WORDS.contains(&w.as_str())) && lw.iter().any(|w| TARGETS.contains(&w.as_str())) {
        return Some("sounds like a local file command");
    }
    None
}

/// Cut a stored context line to the sent size (never mid-way past the limit).
pub fn chat_clip(s: &str) -> String { s.chars().take(CHAT_LINE_CHARS).collect() }

/// One chat turn: POST the exact messages shown on the card. Nothing else is sent.
/// The provider (chosen with "use ollama model ..." etc., default Pollinations)
/// decides the endpoint, the model name and whether text leaves the machine.
pub fn chat(http: &dyn Http, provider: &crate::providers::ChatProvider, context: &[String], text: &str, cancelled: &dyn Fn() -> bool, commit: &dyn Fn() -> bool) -> Result<String, PawError> {
    if let Some(why) = chat_block_reason(text) {
        return Err(PawError::Validation(format!("I did not send that: it {why}. Nothing was sent.")));
    }
    let mut msgs = vec![serde_json::json!({"role": "system", "content": CHAT_SYSTEM})];
    for (i, c) in context.iter().take(CHAT_CONTEXT_LINES).enumerate() {
        msgs.push(serde_json::json!({"role": if i % 2 == 0 { "user" } else { "assistant" }, "content": chat_clip(c)}));
    }
    msgs.push(serde_json::json!({"role": "user", "content": text.trim()}));
    let body = serde_json::json!({"model": provider.model, "messages": msgs}).to_string();
    let owned = provider.headers();
    let headers: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let r = http.post(&provider.url, &headers, &body).map_err(|_| provider.unreachable_error())?;
    if cancelled() || !commit() {
        return Err(PawError::Exec("Cancelled. Nothing was kept, and a late answer will be discarded.".into()));
    }
    match r.status {
        200 => {
            let raw = String::from_utf8_lossy(&r.body).to_string();
            let content = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| v["choices"][0]["message"]["content"].as_str().map(|s| s.to_string()))
                .ok_or_else(|| PawError::Exec(format!("{} sent a reply Pawline could not read; no answer.", provider.name)))?;
            let clean: String = content.chars().filter(|c| !c.is_control() || *c == '\n').collect();
            let clean = clean.trim();
            if clean.is_empty() {
                return Err(PawError::Exec(format!("{} sent an empty answer.", provider.name)));
            }
            let n = clean.chars().count();
            let shown: String = clean.chars().take(MAX_ANSWER_CHARS).collect();
            Ok(if n > MAX_ANSWER_CHARS { format!("{shown}\n(shortened: {n} characters received)") } else { shown })
        }
        402 | 429 => Err(PawError::Exec(format!(
            "{} said its free limit is reached right now (HTTP {}).{} I never pay or upgrade, so there is no answer.",
            provider.name,
            r.status,
            r.retry_after.map(|s| format!(" Try again in about {s} seconds.")).unwrap_or_default()
        ))),
        404 if provider.local => Err(PawError::Exec(format!(
            "{} answered HTTP 404: the model \"{}\" is probably not loaded on the server (with Ollama, pull it first; with llama-server or LM Studio, load it there). Nothing was sent.",
            provider.name, provider.model
        ))),
        413 => Err(PawError::Exec(format!("{} said the chat was too long (HTTP 413). Say \"forget our chat\" and try again.", provider.name))),
        s => Err(PawError::Exec(format!("{} answered HTTP {s}; no answer.", provider.name))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imagegen::HttpReply;

    #[test]
    fn auto_only_for_clearly_general_questions() {
        for q in ["what is a black hole", "how does photosynthesis work?", "explain recursion", "why is the sky blue", "define entropy", "what is a strong password", "how do vaccines work"] {
            assert_eq!(classify(q), Class::Auto, "{q}");
        }
    }
    #[test]
    fn private_or_uncertain_asks_first() {
        for q in [
            "what is my wifi password",
            "what does ~/notes.txt say",
            "who is Ahmed Hassan",
            "can you email bob@example.com",
            "what is 5551234567 worth",
            "how do I fix report.docx",
            "is it going to rain",
            "what is the \"secret\" ingredient",
            "how does Alice feel",
        ] {
            assert_eq!(classify(q), Class::Ask, "{q}");
        }
        assert_eq!(classify(&format!("what is {}", "ab ".repeat(150))), Class::Ask);
        assert_eq!(classify(&format!("what is {}", "x".repeat(400))), Class::Block);
    }
    #[test]
    fn adversarial_lowercase_private_context_asks_never_auto() {
        for q in [
            "what is the acme merger status",
            "what does contoso owe",
            "what is the client budget",
            "how does project falcon work",
            "what is the layoff plan",
            "where is baker street",
            "what is the dose for metformin",
            "what is a normal blood pressure for janet",
            "explain the diagnosis of the patient",
            "what is a1b2c3",
            "define zxqv",
            "what is the internal roadmap",
            "how does our billing work",
            "what is the key",
        ] {
            assert_ne!(classify(q), Class::Auto, "{q}");
        }
    }
    #[test]
    fn secret_values_are_blocked_not_asked() {
        for q in [
            "what is 8f14e45fceea167a5a36dedd4bea2543",
            "how does sk-proj-abcdefgh12345678 work",
            "what is ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "what is my password is hunter2",
            "what is the token: abc123",
            "why is 4111 1111 1111 1111 declined",
            "what is -----BEGIN PRIVATE KEY-----",
            "what is AKIAIOSFODNN7EXAMPLE",
            "what does eyJhbGciOiJIUzI1NiJ9 mean",
        ] {
            assert_eq!(classify(q), Class::Block, "{q}");
        }
        // a question ABOUT passwords is not a secret value
        assert_eq!(classify("what is a strong password"), Class::Auto);
        assert_ne!(classify("what is a password"), Class::Block);
    }
    #[test]
    fn failed_local_commands_do_not_fall_back_online() {
        for q in ["what is in my downloads folder", "can you make a folder called x?", "how do i delete the file report", "what does open spotify do", "could you list the files in downloads"] {
            assert_eq!(classify(q), Class::No, "{q}");
        }
    }
    #[test]
    fn non_questions_are_not_sent() {
        for q in ["frobnicate the widget", "make a folder", "hello big world", ""] {
            assert_eq!(classify(q), Class::No, "{q}");
        }
    }
    struct F(u16, &'static [u8]);
    impl Http for F {
        fn get(&self, _u: &str, _h: &[(&str, &str)]) -> Result<HttpReply, PawError> {
            Ok(HttpReply { status: self.0, retry_after: Some(45), content_type: "text/plain".into(), body: self.1.to_vec() })
        }
        fn post(&self, _u: &str, _h: &[(&str, &str)], _b: &str) -> Result<HttpReply, PawError> {
            unreachable!()
        }
    }
    struct Down;
    impl Http for Down {
        fn get(&self, _u: &str, _h: &[(&str, &str)]) -> Result<HttpReply, PawError> {
            Err(PawError::Exec("network problem: io: Connection refused (os error 111)".into()))
        }
        fn post(&self, _u: &str, _h: &[(&str, &str)], _b: &str) -> Result<HttpReply, PawError> {
            unreachable!()
        }
    }
    #[test]
    fn unreachable_service_is_one_plain_sentence_without_retry_or_raw_error() {
        let e = ask(&Down, "what is a cat", &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("could not be reached") && e.contains("did not retry") && !e.contains("os error") && !e.contains("io:"), "{e}");
    }
    #[test]
    fn url_has_only_the_question() {
        assert_eq!(pollinations_text_url("what is a cat?"), "https://text.pollinations.ai/what%20is%20a%20cat%3F");
    }
    #[test]
    fn answer_is_plain_text_and_402_is_plain_failure() {
        assert_eq!(ask(&F(200, b"A cat is a small mammal.\x07"), "what is a cat", &|| false, &|| true).unwrap(), "A cat is a small mammal.");
        let e = ask(&F(402, b""), "what is a cat", &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("never pay") && e.contains("45 seconds"), "{e}");
        let long = "z".repeat(2000);
        let long: &'static [u8] = Box::leak(long.into_bytes().into_boxed_slice());
        let a = ask(&F(200, long), "what is z", &|| false, &|| true).unwrap();
        assert!(a.contains("shortened: 2000"));
    }
    #[test]
    fn cancel_wins_and_overlong_is_refused() {
        assert!(ask(&F(200, b"x"), "what is a cat", &|| true, &|| true).unwrap_err().to_string().contains("Cancelled"));
        assert!(ask(&F(200, b"x"), "what is a cat", &|| false, &|| false).unwrap_err().to_string().contains("Cancelled"));
        assert!(ask(&F(200, b"x"), &"q".repeat(301), &|| false, &|| true).unwrap_err().to_string().contains("won't cut"));
    }

    #[test]
    fn chat_posts_exactly_the_shown_messages_and_blocks_private_text() {
        let seen = std::cell::RefCell::new(vec![]);
        struct P<'a>(&'a std::cell::RefCell<Vec<String>>, u16);
        impl Http for P<'_> {
            fn get(&self, u: &str, _: &[(&str, &str)]) -> Result<HttpReply, PawError> { self.0.borrow_mut().push(format!("GET {u}")); Err(PawError::Exec("no".into())) }
            fn post(&self, u: &str, h: &[(&str, &str)], b: &str) -> Result<HttpReply, PawError> {
                let hs: Vec<String> = h.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                self.0.borrow_mut().push(format!("POST {u} [{hs}] {b}", hs = hs.join(", ")));
                Ok(HttpReply { status: self.1, retry_after: None, content_type: String::new(), body: br#"{"choices":[{"message":{"content":"Hi there"}}]}"#.to_vec() })
            }
        }
        let prov = crate::providers::ChatProvider::pollinations();
        let ctx = vec!["hello".to_string(), "hi".to_string()];
        let r = chat(&P(&seen, 200), &prov, &ctx, "how are you today", &|| false, &|| true).unwrap();
        assert_eq!(r, "Hi there");
        let s = seen.borrow();
        assert_eq!(s.len(), 1);
        assert!(s[0].starts_with("POST https://text.pollinations.ai/openai ") && s[0].contains("how are you today") && s[0].contains("hello"), "{}", s[0]);
        drop(s);
        for bad in ["my password is hunter2", "read ~/notes.txt to me", "delete the folder kp", "email bob@example.com", &"x".repeat(301)] {
            let before = seen.borrow().len();
            assert!(chat(&P(&seen, 200), &prov, &[], bad, &|| false, &|| true).is_err(), "{bad}");
            assert_eq!(seen.borrow().len(), before, "nothing sent for {bad}");
        }
        assert!(chat(&P(&seen, 402), &prov, &[], "tell me more", &|| false, &|| true).is_err());
        let n = seen.borrow().len();
        assert!(chat(&P(&seen, 200), &prov, &[], "tell me more", &|| true, &|| true).is_err());
        assert_eq!(seen.borrow().len(), n + 1);
    }

    #[test]
    fn chat_uses_the_configured_provider_endpoint_model_and_key() {
        let seen = std::cell::RefCell::new(vec![]);
        struct P<'a>(&'a std::cell::RefCell<Vec<String>>, u16);
        impl Http for P<'_> {
            fn get(&self, u: &str, _: &[(&str, &str)]) -> Result<HttpReply, PawError> { self.0.borrow_mut().push(format!("GET {u}")); Err(PawError::Exec("no".into())) }
            fn post(&self, u: &str, h: &[(&str, &str)], b: &str) -> Result<HttpReply, PawError> {
                let hs: Vec<String> = h.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                self.0.borrow_mut().push(format!("POST {u} [{hs}] {b}", hs = hs.join(", ")));
                Ok(HttpReply { status: self.1, retry_after: None, content_type: String::new(), body: br#"{"choices":[{"message":{"content":"local hi"}}]}"#.to_vec() })
            }
        }
        // Ollama on this machine: loopback URL, chosen model in the body, no key header.
        let prov = crate::providers::ChatProvider::ollama("llama3.2");
        let r = chat(&P(&seen, 200), &prov, &[], "hi there", &|| false, &|| true).unwrap();
        assert_eq!(r, "local hi");
        let s = seen.borrow();
        assert!(s[0].starts_with("POST http://127.0.0.1:11434/v1/chat/completions "), "{}", s[0]);
        assert!(s[0].contains(r#""model":"llama3.2""#), "{}", s[0]);
        assert!(!s[0].contains("Authorization"), "no key configured, no auth header: {}", s[0]);
        drop(s);
        // A keyed custom endpoint sends the key in the header and nowhere else.
        let prov = crate::providers::ChatProvider::openai_compatible("Hosted", "https://llm.example.com", "m", Some("sk-secret-999".into()));
        let _ = chat(&P(&seen, 200), &prov, &[], "hi there", &|| false, &|| true).unwrap();
        let s = seen.borrow();
        assert!(s[1].contains("Authorization: Bearer sk-secret-999"), "{}", s[1]);
        assert!(s[1].starts_with("POST https://llm.example.com/v1/chat/completions "), "{}", s[1]);
        drop(s);
        // Local 404 names the missing model; unreachable local server names the endpoint.
        let e = chat(&P(&seen, 404), &crate::providers::ChatProvider::ollama("llama3.2"), &[], "hi there", &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("llama3.2") && e.contains("not loaded"), "{e}");
        struct Down;
        impl Http for Down {
            fn get(&self, _: &str, _: &[(&str, &str)]) -> Result<HttpReply, PawError> { Err(PawError::Exec("no".into())) }
            fn post(&self, _: &str, _: &[(&str, &str)], _: &str) -> Result<HttpReply, PawError> { Err(PawError::Exec("connection refused".into())) }
        }
        let e = chat(&Down, &crate::providers::ChatProvider::ollama("llama3.2"), &[], "hi there", &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("probably not running") && e.contains("11434"), "{e}");
    }

    /// Written after the filter, not used to tune it: ordinary chat must be offered, private or command-like text must not.
    #[test]
    fn chat_filter_heldout_offers_ordinary_talk_and_blocks_private() {
        let ok = [
            "Really? That's cool, but why!", "well... maybe so", "it's 3.5 degrees out today", "e.g. cats are fun", "I think clouds look like sheep",
            "what do you think about the moon?", "can you explain it more simply?", "ok, and then what happened next", "I run every day and feel great",
            "tell me more", "why is that?", "I'm bored; what should we talk about?", "my favourite colour is blue", "He said \"hello\" and left",
            "I deleted a file yesterday and I was sad", "do you like music? I do!", "what's the difference between a frog and a toad", "in 2026 things changed",
            "show me what a haiku looks like", "so... are you real?", "thanks, that helps (a lot)", "Is it true that octopuses have three hearts?",
        ];
        for t in ok { assert_eq!(chat_block_reason(t), None, "should be offered: {t}"); }
        let bad = [
            "my password is hunter2", "read ~/notes.txt to me", "delete the folder kp", "make a folder called cats", "email bob@example.com", "open https://example.com",
            "call 5551234567", "move report.pdf to documents", "rename the file a.txt", "my pin is 4821 ok", "list files in /etc", "set a wallpaper",
            "the key is `abc123`", "api key sk-abcdefghijklmnop", "install the app firefox",
        ];
        for t in bad { assert!(chat_block_reason(t).is_some(), "should be blocked: {t}"); }
    }
}
