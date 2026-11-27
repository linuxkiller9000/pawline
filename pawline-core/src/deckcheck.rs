//! "check the draft against <file>": a word-level comparison of a reviewed presentation draft
//! against a text file the USER chose. Local, deterministic, no AI, no network.
//!
//! What this is: each slide title and bullet is broken into content words and numbers;
//! every one of them must appear in the source file for the line to be reported as found.
//! What this is NOT: understanding. A line can match every word and still be wrong, and a
//! correct line can be reported missing when the file words the same fact differently.
//! Tables and charts are not checked in this slice (their numbers come from the user's own
//! outline table); image lines carry no facts. The "Contents" slide is generated and skipped.

/// One checked line of the draft.
#[derive(Debug, PartialEq, Eq)]
pub struct LineCheck {
    /// 1-based slide number in the draft.
    pub slide: usize,
    pub title: bool,
    pub text: String,
    /// Content words and numbers from the line that do NOT appear in the source file.
    pub missing: Vec<String>,
}

impl LineCheck {
    pub fn found(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Small English stopword list. Kept short on purpose: these carry no facts.
const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "in", "is",
    "it", "its", "of", "on", "or", "that", "the", "their", "them", "there", "these", "they", "this", "those", "to", "was", "were", "will", "with",
];

/// Eastern Arabic digits fold to ASCII so "٢٠٢٤" matches "2024".
fn fold_digits(c: char) -> char {
    match c {
        '\u{0660}'..='\u{0669}' => char::from_digit(c as u32 - 0x0660, 10).unwrap(),
        '\u{06F0}'..='\u{06F9}' => char::from_digit(c as u32 - 0x06F0, 10).unwrap(),
        _ => c,
    }
}

/// Content tokens of one text: lowercase words of 2+ letters, plus every number.
/// Words under a stopword or length bar carry no checkable fact and are dropped.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut flush = |cur: &mut String| {
        if !cur.is_empty() {
            let w = cur.to_lowercase();
            let number = w.chars().all(|c| c.is_ascii_digit());
            if number || (w.chars().count() >= 2 && !STOP.contains(&w.as_str())) {
                out.push(w);
            }
            cur.clear();
        }
    };
    for c in text.chars() {
        let c = fold_digits(c);
        if c.is_alphanumeric() {
            cur.push(c);
        } else {
            flush(&mut cur);
        }
    }
    flush(&mut cur);
    out
}

/// Compare draft slides against the source text the user chose.
/// Returns one LineCheck per slide title and per bullet, in slide order.
/// `source_tokens_empty` callers refuse an empty source before calling this.
pub fn check_slides_against_source(
    slides: &[crate::deck::Slide],
    source: &str,
) -> Vec<LineCheck> {
    let have: std::collections::HashSet<String> = tokens(source).into_iter().collect();
    let mut out = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        if s.title == "Contents" {
            continue; // generated overview slide: no facts of its own
        }
        let mut line = |title: bool, text: &str| {
            let missing: Vec<String> = tokens(text)
                .into_iter()
                .filter(|t| !have.contains(t))
                .collect();
            let mut dedup: Vec<String> = Vec::new();
            for m in missing {
                if !dedup.contains(&m) {
                    dedup.push(m);
                }
            }
            out.push(LineCheck { slide: i + 1, title, text: text.to_string(), missing: dedup });
        };
        if !s.title.trim().is_empty() {
            line(true, &s.title);
        }
        for (_, b) in &s.bullets {
            line(false, b);
        }
    }
    out
}

/// The review text shown after a check. Always carries the honest label.
pub fn check_review_text(file: &str, checks: &[LineCheck]) -> String {
    let total = checks.len();
    let not_found = checks.iter().filter(|c| !c.found()).count();
    let mut t = format!(
        "Checked the draft against {file}: {} of {total} lines were NOT found there.\n\
         This is a word-level match, not understanding: a line can match and still be wrong, and a correct line can be flagged when the file words it differently. Tables and charts are not checked. The draft is unchanged.\n",
        not_found
    );
    let mut last_slide = 0usize;
    for c in checks {
        if c.slide != last_slide {
            last_slide = c.slide;
            t.push_str(&format!("\nSlide {}:\n", c.slide));
        }
        if c.found() {
            t.push_str(&format!("  found: {}\n", c.text));
        } else {
            t.push_str(&format!(
                "  NOT found: {} (missing: {})\n",
                c.text,
                c.missing.join(", ")
            ));
        }
    }
    t
}

/// Where the comparison source comes from: a local file (offline, default) or
/// a web page the user explicitly named (online; needs the disclosure card first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTarget {
    File(String),
    Url(String),
}

/// "check the draft against <file-or-url>" / "check this presentation against <...>".
/// Returns the target exactly as typed (surrounding quotes stripped), or None when this is not a check command.
pub fn parse_check_command(input: &str) -> Option<CheckTarget> {
    let t = input.trim();
    for pre in [
        "check the draft against ",
        "check this draft against ",
        "check the presentation against ",
        "check this presentation against ",
        "check draft against ",
    ] {
        if t.to_lowercase().starts_with(pre) {
            let raw = t[pre.len()..].trim().trim_matches(|c| c == '"' || c == '\'');
            if raw.is_empty() {
                return None;
            }
            let l = raw.to_lowercase();
            if l.starts_with("https://") || l.starts_with("http://") {
                return Some(CheckTarget::Url(raw.to_string()));
            }
            return Some(CheckTarget::File(raw.to_string()));
        }
    }
    None
}

/// Validate the URL of an online check source. https only (a plain-http page
/// sends the address unencrypted), no credentials in the address, a real host.
pub fn valid_source_url(url: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").ok_or("only https:// addresses can be checked against; a plain http page would send the address unencrypted")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err("that address has no host".into());
    }
    if host.contains('@') || host.contains(':') && !host.starts_with('[') {
        return Err("addresses with a username, password or port are not accepted".into());
    }
    if url.chars().count() > 500 {
        return Err("that address is longer than 500 characters".into());
    }
    Ok(())
}

/// Largest page body used for a check (text after decoding).
pub const MAX_PAGE: usize = 256 * 1024;

/// Deterministic HTML-to-text for the check: script/style/noscript blocks and
/// all tags are dropped, a small entity set is decoded, whitespace collapses.
/// Not a browser: malformed pages degrade to their visible words, never to code.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    loop {
        let lower = rest.to_lowercase();
        let mut cut = None;
        for tag in ["<script", "<style", "<noscript"] {
            if let Some(i) = lower.find(tag) {
                let close = tag.trim_start_matches('<');
                if let Some(j) = lower[i..].find(&format!("</{close}")) {
                    let cand = (i, i + j + close.len() + 3);
                    if cut.map_or(true, |(a, _)| cand.0 < a) {
                        cut = Some(cand);
                    }
                }
            }
        }
        match cut {
            Some((a, b)) => {
                out.push_str(&rest[..a]);
                rest = &rest[b..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    let mut text = String::with_capacity(out.len());
    let mut in_tag = false;
    for c in out.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What an approved URL check fetched and compared.
pub struct UrlCheck {
    pub checks: Vec<LineCheck>,
    pub review: String,
}

/// Fetch the page through the injected Http and run the word-level check.
/// The page text never leaves memory; callers must not write it anywhere.
pub fn fetch_and_check(
    http: &dyn crate::imagegen::Http,
    url: &str,
    slides: &[crate::deck::Slide],
) -> Result<UrlCheck, String> {
    valid_source_url(url)?;
    let r = http
        .get(url, &[])
        .map_err(|e| format!("the page could not be fetched: {e}"))?;
    if r.status != 200 {
        return Err(format!("the page answered with status {}, not 200", r.status));
    }
    let ct = r.content_type.to_lowercase();
    let is_html = ct.contains("text/html");
    if !is_html && !ct.contains("text/plain") && !ct.is_empty() {
        return Err(format!("the page is '{}' ({}), not a text or html page", r.content_type, r.status));
    }
    let body = String::from_utf8_lossy(&r.body);
    let body: String = body.chars().take(MAX_PAGE).collect();
    let text = if is_html { html_to_text(&body) } else { body.split_whitespace().collect::<Vec<_>>().join(" ") };
    if text.trim().is_empty() {
        return Err("the page had no readable text to compare against".into());
    }
    let checks = check_slides_against_source(slides, &text);
    let review = check_review_text(url, &checks);
    Ok(UrlCheck { checks, review })
}

/// Reads a UTF-8 text file the user chose, confined to `home` the same way image: reads are.
/// Refuses paths outside home, non-files, non-UTF-8 and anything over the outline size cap.
pub fn read_source_text(home: &std::path::Path, raw: &str) -> Result<String, String> {
    use std::io::Read;
    let full = if let Some(r) = raw.strip_prefix("~/") {
        home.join(r)
    } else if raw.starts_with('/') {
        std::path::PathBuf::from(raw)
    } else {
        home.join(raw)
    };
    let rel = full
        .strip_prefix(home)
        .map_err(|_| format!("{} is outside your home folder, so Pawline will not read it", full.display()))?;
    let dir = cap_std::fs::Dir::open_ambient_dir(home, cap_std::ambient_authority()).map_err(|e| e.to_string())?;
    let mut f = dir
        .open(rel)
        .map_err(|_| format!("{} can't be opened (missing, or it points outside your home folder)", full.display()))?
        .into_std();
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err(format!("{} is not a plain file", full.display()));
    }
    if m.len() > crate::deck::MAX_SOURCE {
        return Err("that file is bigger than 256 KB, the outline size limit".into());
    }
    let mut d = vec![];
    Read::take(&mut f, crate::deck::MAX_SOURCE + 1)
        .read_to_end(&mut d)
        .map_err(|e| e.to_string())?;
    String::from_utf8(d).map_err(|_| format!("{} is not plain UTF-8 text, so it cannot be compared", full.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::Slide;

    fn slide(title: &str, bullets: &[&str]) -> Slide {
        Slide {
            title: title.into(),
            bullets: bullets.iter().map(|b| (0, b.to_string())).collect(),
            table: vec![],
            chart: None,
            image: None,
            source: None,
        }
    }

    #[test]
    fn lines_whose_words_and_numbers_are_in_the_source_are_found() {
        let src = "Cairo is the capital of Egypt. About 10 million people live in the city.";
        let slides = vec![
            slide("Cairo", &["Cairo is the capital of Egypt", "10 million people live there"]),
        ];
        let r = check_slides_against_source(&slides, src);
        assert_eq!(r.len(), 3);
        assert!(r.iter().all(|c| c.found()), "{r:?}");
    }

    #[test]
    fn invented_numbers_and_words_are_named_missing() {
        let src = "Cairo is the capital of Egypt.";
        let slides = vec![slide("Cairo", &["Cairo has 22 million people and a monorail"])];
        let r = check_slides_against_source(&slides, src);
        let b = r.iter().find(|c| !c.title).unwrap();
        assert!(!b.found());
        assert!(b.missing.contains(&"22".to_string()));
        assert!(b.missing.contains(&"million".to_string()));
        assert!(b.missing.contains(&"monorail".to_string()));
        assert!(!b.missing.contains(&"cairo".to_string()));
    }

    #[test]
    fn eastern_arabic_digits_match_ascii() {
        let src = "the history of the city: founded in 969";
        let slides = vec![slide("History", &["founded in \u{0669}\u{0666}\u{0669}"])];
        let r = check_slides_against_source(&slides, src);
        assert!(r.iter().all(|c| c.found()), "{r:?}");
    }

    #[test]
    fn url_targets_are_split_from_files_and_validated() {
        assert_eq!(parse_check_command("check the draft against facts.txt"), Some(CheckTarget::File("facts.txt".into())));
        assert_eq!(parse_check_command("check the draft against https://example.com/a"), Some(CheckTarget::Url("https://example.com/a".into())));
        assert_eq!(parse_check_command("check the draft against HTTP://example.com"), Some(CheckTarget::Url("HTTP://example.com".into())));
        assert!(valid_source_url("https://example.com/page?x=1").is_ok());
        assert!(valid_source_url("http://example.com").is_err());
        assert!(valid_source_url("https://user:pw@example.com/").is_err());
        assert!(valid_source_url("https://").is_err());
    }

    #[test]
    fn html_to_text_drops_scripts_styles_and_decodes_entities() {
        let h = "<html><head><style>body{color:red}</style><script>var x=1;</script></head><body><h1>Rain &amp; Sun</h1><p>Water&nbsp;falls</p></body></html>";
        let t = html_to_text(h);
        assert!(t.contains("Rain & Sun"), "{t}");
        assert!(t.contains("Water falls"), "{t}");
        assert!(!t.contains("color") && !t.contains("var") && !t.contains("<"), "{t}");
    }

    struct FakeHttp { status: u16, ct: &'static str, body: &'static str }
    impl crate::imagegen::Http for FakeHttp {
        fn get(&self, _url: &str, _headers: &[(&str, &str)]) -> Result<crate::imagegen::HttpReply, crate::error::PawError> {
            Ok(crate::imagegen::HttpReply { status: self.status, retry_after: None, content_type: self.ct.into(), body: self.body.as_bytes().to_vec() })
        }
        fn post(&self, _url: &str, _headers: &[(&str, &str)], _body: &str) -> Result<crate::imagegen::HttpReply, crate::error::PawError> {
            Err(crate::error::PawError::Exec("not used".into()))
        }
    }

    #[test]
    fn fetch_and_check_compares_against_the_page_text() {
        let slides = vec![slide("Rain", &["Rain falls from clouds", "Rain is purple"])];
        let ok = FakeHttp { status: 200, ct: "text/html; charset=utf-8", body: "<p>Rain falls from clouds.</p><script>evil()</script>" };
        let r = fetch_and_check(&ok, "https://example.com/rain", &slides).unwrap();
        assert_eq!(r.checks.iter().filter(|c| !c.found()).count(), 1, "{:?}", r.checks);
        assert!(r.review.contains("word-level match, not understanding"));
        // non-200, wrong type and empty text are all refused with reasons
        assert!(fetch_and_check(&FakeHttp { status: 404, ..ok }, "https://example.com/rain", &slides).is_err());
        assert!(fetch_and_check(&FakeHttp { status: 200, ct: "application/pdf", ..ok }, "https://example.com/rain", &slides).is_err());
        assert!(fetch_and_check(&FakeHttp { status: 200, ct: "text/html", body: "<script>only()</script>" }, "https://example.com/rain", &slides).is_err());
    }

    #[test]
    fn contents_slide_is_skipped_and_review_text_labels_itself() {
        let src = "alpha beta";
        let mut slides = vec![slide("Contents", &["Alpha"]), slide("Alpha", &["beta gamma"])];
        slides[0].title = "Contents".into();
        let r = check_slides_against_source(&slides, src);
        assert!(r.iter().all(|c| c.slide == 2), "{r:?}");
        let t = check_review_text("facts.txt", &r);
        assert!(t.contains("word-level match, not understanding"));
        assert!(t.contains("NOT found: beta gamma (missing: gamma)"));
        assert!(t.contains("The draft is unchanged."));
    }
}
