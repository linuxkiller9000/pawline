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
    /// Not a question: not sent anywhere.
    No,
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
    "my", "mine", "me", "i", "i'm", "i've", "i'd", "our", "ours", "we", "password", "passcode", "token", "secret", "apikey",
    "credit", "card", "bank", "iban", "address", "phone", "wifi", "ssid", "ssn", "login", "pin", "otp", "salary", "boss",
    "wife", "husband", "girlfriend", "boyfriend", "home", "file", "files", "folder", "folders", "desktop", "documents",
    "downloads", "pictures", "history", "clipboard",
];

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
    if t.is_empty() || t.chars().count() > MAX_QUESTION_CHARS || t.chars().any(|c| c.is_control()) {
        return if t.is_empty() { Class::No } else { Class::Ask };
    }
    let lower = t.to_lowercase();
    let questiony = t.ends_with('?') || QUESTION_STARTS.iter().any(|p| lower.starts_with(p));
    if !questiony {
        return Class::No;
    }
    if has_private_marker(t) {
        return Class::Ask;
    }
    if AUTO_STARTS.iter().any(|p| lower.starts_with(p)) {
        Class::Auto
    } else {
        Class::Ask
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
        for q in ["what is a black hole", "how does photosynthesis work?", "explain recursion", "why is the sky blue", "define entropy", "when was the telescope invented"] {
            assert_eq!(classify(q), Class::Auto, "{q}");
        }
    }
    #[test]
    fn private_or_uncertain_asks_first() {
        for q in [
            "what is my wifi password",
            "what does ~/notes.txt say",
            "what is in my downloads folder",
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
        assert_eq!(classify(&format!("what is {}", "x".repeat(400))), Class::Ask);
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
