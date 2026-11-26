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
         This is a word-level match against your file, not understanding: a line can match and still be wrong, and a correct line can be flagged when the file words it differently. Tables and charts are not checked. The draft is unchanged.\n",
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

/// "check the draft against <file>" / "check this presentation against <file>".
/// Returns the path exactly as typed (surrounding quotes stripped), or None when this is not a check command.
pub fn parse_check_command(input: &str) -> Option<String> {
    let t = input.trim();
    for pre in [
        "check the draft against ",
        "check this draft against ",
        "check the presentation against ",
        "check this presentation against ",
        "check draft against ",
    ] {
        if let Some(rest) = t.to_lowercase().strip_prefix(pre) {
            let _ = rest;
            let raw = t[pre.len()..].trim().trim_matches(|c| c == '"' || c == '\'');
            if !raw.is_empty() {
                return Some(raw.to_string());
            }
            return None;
        }
    }
    None
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
    fn contents_slide_is_skipped_and_review_text_labels_itself() {
        let src = "alpha beta";
        let mut slides = vec![slide("Contents", &["Alpha"]), slide("Alpha", &["beta gamma"])];
        slides[0].title = "Contents".into();
        let r = check_slides_against_source(&slides, src);
        assert!(r.iter().all(|c| c.slide == 2), "{r:?}");
        let t = check_review_text("facts.txt", &r);
        assert!(t.contains("word-level match against your file, not understanding"));
        assert!(t.contains("NOT found: beta gamma (missing: gamma)"));
        assert!(t.contains("The draft is unchanged."));
    }
}
