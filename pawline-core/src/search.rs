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
    get_json_within(url, 20)
}

/// JSON API replies are kilobytes in practice; 2 MB is generous and stops a hostile or
/// broken server from filling memory (ureq's 10 MB default is more than we need).
const MAX_SEARCH_BYTES: u64 = 2 * 1024 * 1024;

fn get_json_within(url: &str, secs: u64) -> Result<serde_json::Value, PawError> {
    // A global timeout so a server that never answers cannot hang the engine thread (audit item 21).
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            // See imagegen.rs: keep redirect chains short (audit item 21).
            .max_redirects(3)
            .timeout_global(Some(std::time::Duration::from_secs(secs)))
            .build(),
    );
    let body = agent
        .get(url)
        .header("User-Agent", "Pawline/0.1 (personal desktop assistant; https://github.com/linuxkiller9000/pawline)")
        .call()
        .map_err(|e| PawError::Exec(format!("search request failed: {e}")))?
        .body_mut()
        .with_config()
        .limit(MAX_SEARCH_BYTES)
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
    fn a_server_that_never_answers_times_out_instead_of_hanging() {
        // Behavioural test on a real local socket (author-written): accepts, then stays silent.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let c = l.accept();
            std::thread::sleep(std::time::Duration::from_secs(4));
            drop(c);
        });
        let t = std::time::Instant::now();
        let r = super::get_json_within(&format!("http://127.0.0.1:{port}/x"), 1);
        assert!(r.is_err());
        assert!(t.elapsed() < std::time::Duration::from_secs(3), "took {:?}", t.elapsed());
        let _ = h.join();
    }

    fn serve_once(port_ready: std::net::TcpListener, response: String) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            if let Ok((mut c, _)) = port_ready.accept() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                use std::io::Write;
                let _ = c.write_all(response.as_bytes());
            }
        })
    }

    #[test]
    fn an_oversized_reply_is_refused_instead_of_filling_memory() {
        // Behavioural test on a real local socket (author-written): a valid header, then 3 MB of body.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            if let Ok((mut c, _)) = l.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                let body = "x".repeat(3 * 1024 * 1024);
                let _ = c.write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes(),
                );
            }
        });
        let r = super::get_json_within(&format!("http://127.0.0.1:{port}/x"), 5);
        let e = r.err().expect("3 MB reply must fail").to_string();
        assert!(e.contains("search read failed"), "{e}");
        let _ = h.join();
    }

    #[test]
    fn a_long_redirect_chain_is_refused() {
        // Behavioural (author-written): 4 redirects, one past the cap of 3, must error rather than follow forever.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            use std::io::{Read, Write};
            for _ in 0..5 {
                if let Ok((mut c, _)) = l.accept() {
                    let mut buf = [0u8; 4096];
                    let _ = c.read(&mut buf);
                    let _ = c.write_all(format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/next\r\nContent-Length: 0\r\n\r\n").as_bytes());
                }
            }
        });
        let r = super::get_json_within(&format!("http://127.0.0.1:{port}/start"), 5);
        assert!(r.is_err(), "an endless redirect chain must not be followed");
        drop(h);
    }

    #[test]
    fn one_redirect_is_followed() {
        // Behavioural (author-written): a single hop is legitimate (Wikipedia canonicalisation) and must work.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h1 = serve_once(
            l.try_clone().unwrap(),
            format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/real\r\nContent-Length: 0\r\n\r\n"),
        );
        let h2 = serve_once(
            l,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}".to_string(),
        );
        let v = super::get_json_within(&format!("http://127.0.0.1:{port}/start"), 5).expect("one redirect must be followed");
        assert_eq!(v.get("a").and_then(|x| x.as_i64()), Some(1));
        let _ = h1.join();
        let _ = h2.join();
    }

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
