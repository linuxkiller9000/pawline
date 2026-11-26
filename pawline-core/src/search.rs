//! Explicit online search. Only runs when the user asks by name; offline
//! behavior is otherwise untouched. Results are shown, never acted on.

use crate::error::PawError;
use crate::plan::SearchEngine;

const LIVE_NOTE: &str = "I can't look up live data like current prices, stock, availability or opening hours. Check the shop's or provider's own site for that.";

pub fn run(engine: SearchEngine, query: &str) -> Result<String, PawError> {
    run_with(engine, query, wikipedia_summary, duckduckgo_instant)
}

/// Time-sensitive asks (price, stock, hours, weather...) are never answered
/// with an encyclopedia summary: the best match is often an unrelated article.
/// A web instant answer is kept but labelled as possibly out of date.
fn run_with(
    engine: SearchEngine,
    query: &str,
    wiki: impl FnOnce(&str) -> Result<String, PawError>,
    web: impl FnOnce(&str) -> Result<String, PawError>,
) -> Result<String, PawError> {
    let sensitive = crate::searchmem::time_sensitive(query);
    match engine {
        SearchEngine::Wiki if sensitive => Ok(format!(
            "Wikipedia is a reference, not live data, so I did not show an article for \"{query}\". {LIVE_NOTE}"
        )),
        SearchEngine::Wiki => wiki(query),
        SearchEngine::Web => {
            let r = web(query)?;
            if sensitive {
                Ok(format!("{r}\nNote: this may not be current. {LIVE_NOTE}"))
            } else {
                Ok(r)
            }
        }
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
    crate::netgate::check()?;
    let body = ureq::get(url)
        .header("User-Agent", "Pawline/0.1 (personal desktop assistant; https://github.com/linuxkiller9000/pawline)")
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
        let link = v
            .pointer("/content_urls/desktop/page")
            .and_then(|u| u.as_str())
            .map(|u| format!("\nSource: {u}"))
            .unwrap_or_default();
        return Ok(format!("Wikipedia - {title}: {extract}{link}"));
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
                let link = v
                    .get("AbstractURL")
                    .and_then(|u| u.as_str())
                    .filter(|u| !u.is_empty())
                    .map(|u| format!("\nSource: {u}"))
                    .unwrap_or_default();
                return Ok(format!("{t}{link}"));
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
        let link = v
            .pointer("/RelatedTopics/0/FirstURL")
            .and_then(|u| u.as_str())
            .map(|u| format!("\nSource: {u}"))
            .unwrap_or_default();
        return Ok(format!("{topic}{link}"));
    }
    Ok(format!(
        "DuckDuckGo had no instant answer for \"{query}\". Try rephrasing, or \"wiki {query}\". You can also open https://duckduckgo.com/?q={} yourself.",
        enc_query(query)
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

#[cfg(test)]
mod live_tests {
    use super::*;
    fn boom(_: &str) -> Result<String, PawError> { panic!("must not fetch") }
    #[test]
    fn price_on_wiki_is_not_answered_with_an_article() {
        let r = run_with(SearchEngine::Wiki, "price of milk", boom, boom).unwrap();
        assert!(r.contains("live data") && !r.contains("Wikipedia -"));
    }
    #[test]
    fn price_on_web_is_labelled_and_plain_wiki_unchanged() {
        let r = run_with(SearchEngine::Web, "price of milk", boom, |_| Ok("X".into())).unwrap();
        assert!(r.starts_with("X") && r.contains("may not be current"));
        let w = run_with(SearchEngine::Wiki, "cats", |_| Ok("Wikipedia - Cat".into()), boom).unwrap();
        assert_eq!(w, "Wikipedia - Cat");
    }
}
