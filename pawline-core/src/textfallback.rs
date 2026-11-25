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
    let lower = input.to_lowercase();
    if lower.contains('/') || lower.contains('\\') || lower.contains('~') || lower.contains('@') || lower.contains("://") || lower.contains("www.") {
        return true;
    }
    // file-like token: word.ext
    if input.split_whitespace().any(|t| {
        let t = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '.');
        t.contains('.') && !t.starts_with('.') && !t.ends_with('.') && t.rsplit('.').next().is_some_and(|e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()))
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
    if w.iter().any(|x| PRIVATE_WORDS.contains(&x.as_str())) {
        return true;
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
    let r = http.get(&pollinations_text_url(&q), &[])?;
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
}
