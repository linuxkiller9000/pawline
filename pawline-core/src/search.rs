//! Explicit online search. Only runs when the user asks by name; offline
//! behavior is otherwise untouched. Results are shown, never acted on.

use crate::error::PawError;
use crate::plan::SearchEngine;

pub fn run(engine: SearchEngine, query: &str) -> Result<String, PawError> {
    match engine {
        SearchEngine::Wiki => wikipedia_summary(query),
        SearchEngine::Web => duckduckgo_instant(query),
    }
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('_');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn enc_query(s: &str) -> String {
    enc(s).replace('_', "+")
}

fn get_json(url: &str) -> Result<serde_json::Value, PawError> {
    let body = ureq::get(url)
        .header("User-Agent", "Pawline/0.1 (personal desktop assistant)")
        .call()
        .map_err(|e| PawError::Exec(format!("search request failed: {e}")))?
        .body_mut()
        .read_to_string()
        .map_err(|e| PawError::Exec(format!("search read failed: {e}")))?;
    serde_json::from_str(&body).map_err(|e| PawError::Exec(format!("search parse failed: {e}")))
}

fn wikipedia_summary(query: &str) -> Result<String, PawError> {
    let url = format!(
        "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
        enc(query)
    );
    let v = get_json(&url)?;
    if let Some(extract) = v.get("extract").and_then(|e| e.as_str()) {
        let title = v.get("title").and_then(|t| t.as_str()).unwrap_or(query);
        return Ok(format!("Wikipedia - {title}: {extract}"));
    }
    Ok(format!("Wikipedia has no article summary for \"{query}\"."))
}

fn duckduckgo_instant(query: &str) -> Result<String, PawError> {
    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        enc_query(query)
    );
    let v = get_json(&url)?;
    for key in ["AbstractText", "Answer"] {
        if let Some(t) = v.get(key).and_then(|t| t.as_str()) {
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
    }
    if let Some(topic) = v
        .get("RelatedTopics")
        .and_then(|t| t.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("Text"))
        .and_then(|t| t.as_str())
    {
        return Ok(topic.to_string());
    }
    Ok(format!(
        "DuckDuckGo had no instant answer for \"{query}\". Try rephrasing, or \"wiki {query}\"."
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_wiring() {
        use crate::intent::parse;
        use crate::plan::{Directive, SearchEngine};
        assert_eq!(
            parse("wiki tux the penguin").unwrap(),
            vec![Directive::Search {
                engine: SearchEngine::Wiki,
                query: "tux the penguin".into()
            }]
        );
        assert_eq!(
            parse("search the web for niri compositor").unwrap(),
            vec![Directive::Search {
                engine: SearchEngine::Web,
                query: "niri compositor".into()
            }]
        );
    }

    #[test]
    fn encoding() {
        assert_eq!(super::enc("linus torvalds"), "linus_torvalds");
        assert_eq!(super::enc_query("linus torvalds"), "linus+torvalds");
        assert_eq!(super::enc("a/b?c"), "a%2Fb%3Fc");
    }
}
