//! Search memory (stage 1 of "learn through use"): a small local file of past explicit searches.
//! Not model training: nothing here changes any model. It stores only the normalized query, the source,
//! the answer text, the time it was fetched and a hit count. Secret-like queries are never stored.
//! The file is the user's own, plain JSON, mode 0600: they can read, edit or delete it, or say "forget searches".
use crate::plan::SearchEngine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 200;
const MAX_TEXT: usize = 2000;
const MAX_QUERY: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub engine: String,
    pub query: String,
    pub source: String,
    pub text: String,
    /// Unix seconds when fetched online.
    pub fetched: u64,
    pub hits: u32,
}

pub fn engine_name(e: SearchEngine) -> &'static str {
    match e {
        SearchEngine::Web => "web",
        SearchEngine::Wiki => "wiki",
    }
}

pub fn source_name(e: SearchEngine) -> &'static str {
    match e {
        SearchEngine::Web => "DuckDuckGo instant answers",
        SearchEngine::Wiki => "en.wikipedia.org",
    }
}

/// How long a saved answer counts as fresh. Wikipedia changes slowly, instant answers faster.
pub fn ttl_secs(e: SearchEngine) -> u64 {
    match e {
        SearchEngine::Wiki => 7 * 86_400,
        SearchEngine::Web => 86_400,
    }
}

pub fn normalize(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase().chars().take(MAX_QUERY).collect()
}

/// Where the file lives: $PAWLINE_DATA, else $XDG_DATA_HOME/pawline, else ~/.local/share/pawline.
pub fn default_path() -> Option<PathBuf> {
    let dir = if let Some(d) = std::env::var_os("PAWLINE_DATA") {
        PathBuf::from(d)
    } else if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(d).join("pawline")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/share/pawline")
    };
    Some(dir.join("search-memory.json"))
}

pub fn load(path: &Path) -> Vec<Entry> {
    let Some(Value::Array(a)) = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
        return vec![];
    };
    a.iter()
        .filter_map(|v| {
            Some(Entry {
                engine: v.get("engine")?.as_str()?.to_string(),
                query: v.get("query")?.as_str()?.to_string(),
                source: v.get("source")?.as_str()?.to_string(),
                text: v.get("text")?.as_str()?.to_string(),
                fetched: v.get("fetched")?.as_u64()?,
                hits: v.get("hits")?.as_u64()? as u32,
            })
        })
        .collect()
}

fn save(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    let v: Vec<Value> = entries
        .iter()
        .map(|e| json!({"engine": e.engine, "query": e.query, "source": e.source, "text": e.text, "fetched": e.fetched, "hits": e.hits}))
        .collect();
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path)
}

/// A saved answer for this search, and whether it is still fresh at `now`.
pub fn lookup(path: &Path, engine: SearchEngine, query: &str, now: u64) -> Option<(Entry, bool)> {
    let key = normalize(query);
    let e = load(path).into_iter().find(|e| e.engine == engine_name(engine) && e.query == key)?;
    let fresh = now.saturating_sub(e.fetched) < ttl_secs(engine);
    Some((e, fresh))
}

/// Save (or replace) an answer fetched online. Secret-like queries and empty answers are not stored.
pub fn store(path: &Path, engine: SearchEngine, query: &str, text: &str, now: u64) {
    if crate::textfallback::looks_like_secret(query) || text.trim().is_empty() {
        return;
    }
    let key = normalize(query);
    if key.is_empty() {
        return;
    }
    let mut all = load(path);
    let hits = all.iter().find(|e| e.engine == engine_name(engine) && e.query == key).map_or(0, |e| e.hits);
    all.retain(|e| !(e.engine == engine_name(engine) && e.query == key));
    all.push(Entry {
        engine: engine_name(engine).into(),
        query: key,
        source: source_name(engine).into(),
        text: text.chars().take(MAX_TEXT).collect(),
        fetched: now,
        hits,
    });
    if all.len() > MAX_ENTRIES {
        let drop = all.len() - MAX_ENTRIES;
        all.drain(..drop); // oldest first
    }
    let _ = save(path, &all);
}

pub fn bump(path: &Path, engine: SearchEngine, query: &str) {
    let key = normalize(query);
    let mut all = load(path);
    if let Some(e) = all.iter_mut().find(|e| e.engine == engine_name(engine) && e.query == key) {
        e.hits += 1;
        let _ = save(path, &all);
    }
}

pub fn forget_all(path: &Path) -> usize {
    let n = load(path).len();
    let _ = std::fs::remove_file(path);
    n
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "YYYY-MM-DD" (UTC) for a unix time, no date library needed.
pub fn date_of(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// The text shown for the saved-answers list.
pub fn describe_all(path: &Path) -> String {
    let all = load(path);
    if all.is_empty() {
        return "Search memory is empty.".into();
    }
    let mut out = format!("Search memory: {} saved answer(s) (file: {}).\n", all.len(), path.display());
    for e in all.iter().rev().take(15) {
        out.push_str(&format!("- \"{}\" ({}, {}, used {}x)\n", e.query, e.source, date_of(e.fetched), e.hits));
    }
    out.push_str("Say \"forget searches\" to delete all of it.");
    out
}

/// Questions whose answer changes (prices, stock, hours, news, policies...). A saved answer is never shown
/// as current for these: they are always fetched again, and a saved copy is only a labelled fallback.
pub fn time_sensitive(query: &str) -> bool {
    let q = format!(" {} ", normalize(query));
    const WORDS: &[&str] = &[
        "price", "prices", "cost", "costs", "cheap", "cheapest", "how much", "in stock", "stock", "available", "availability",
        "open now", "opening hours", "hours", "weather", "forecast", "score", "scores", "news", "today", "tonight", "tomorrow",
        "latest", "current", "currently", "now", "policy", "policies", "deadline", "schedule", "flight", "flights", "rate",
        "rates", "exchange", "deal", "deals", "sale", "discount", "release date", "version", "ticket", "tickets", "booking",
    ];
    WORDS.iter().any(|w| q.contains(&format!(" {w} ")))
}

/// Answer a search from memory or the network. `fetch` is only called when needed.
/// Fresh saved answers are shown with their date and source (never for time-sensitive questions).
/// If the fetch fails, a saved copy is shown labelled as possibly out of date.
pub fn answer<E>(path: &Path, engine: SearchEngine, query: &str, now: u64, force: bool, fetch: impl FnOnce() -> Result<String, E>) -> Result<String, E> {
    let saved = lookup(path, engine, query, now);
    let sensitive = time_sensitive(query);
    if let (Some((e, true)), false, false) = (&saved, force, sensitive) {
        bump(path, engine, query);
        return Ok(format!(
            "From memory (saved {} from {}):\n{}\n(From memory, saved {}.) To fetch it again, type: refresh: <the same request>",
            date_of(e.fetched),
            e.source,
            e.text,
            date_of(e.fetched)
        ));
    }
    match fetch() {
        Ok(t) => {
            store(path, engine, query, &t, now);
            Ok(t)
        }
        Err(err) => match saved {
            Some((e, _)) => Ok(format!(
                "Couldn't reach the source, so this is the saved answer from {} ({}), it may be out of date:\n{}\n(Saved {}. Don't rely on it for prices, availability, policies or anything that changes.)",
                date_of(e.fetched),
                e.source,
                e.text,
                date_of(e.fetched)
            )),
            None => Err(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-sm-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("search-memory.json")
    }

    #[test]
    fn store_lookup_freshness_and_hits() {
        let p = tmp("a");
        assert!(lookup(&p, SearchEngine::Wiki, "Cats", 1000).is_none());
        store(&p, SearchEngine::Wiki, "  Cats  ", "Cats are small.", 1000);
        let (e, fresh) = lookup(&p, SearchEngine::Wiki, "cats", 1000 + 86_400).unwrap();
        assert!(fresh && e.text == "Cats are small." && e.hits == 0 && e.source == "en.wikipedia.org");
        let (_, fresh) = lookup(&p, SearchEngine::Wiki, "cats", 1000 + 8 * 86_400).unwrap();
        assert!(!fresh, "wiki answers go stale after a week");
        let (_, fresh) = lookup(&p, SearchEngine::Web, "cats", 1000).map_or((e.clone(), false), |x| x);
        assert!(!fresh, "web and wiki are separate entries");
        bump(&p, SearchEngine::Wiki, "cats");
        assert_eq!(lookup(&p, SearchEngine::Wiki, "cats", 1000).unwrap().0.hits, 1);
        store(&p, SearchEngine::Wiki, "cats", "Newer text.", 5000);
        let (e, _) = lookup(&p, SearchEngine::Wiki, "cats", 5000).unwrap();
        assert_eq!((e.text.as_str(), e.hits), ("Newer text.", 1), "refresh replaces text and keeps the count");
        assert_eq!(load(&p).len(), 1);
    }

    #[test]
    fn secrets_empty_and_caps() {
        let p = tmp("b");
        store(&p, SearchEngine::Web, "my password is hunter2", "x", 1);
        store(&p, SearchEngine::Web, "ok query", "   ", 1);
        assert!(load(&p).is_empty());
        for i in 0..210 {
            store(&p, SearchEngine::Web, &format!("q{i}"), "t", i as u64);
        }
        let all = load(&p);
        assert_eq!(all.len(), MAX_ENTRIES);
        assert!(all.iter().all(|e| e.query != "q0") && all.iter().any(|e| e.query == "q209"), "oldest dropped");
        store(&p, SearchEngine::Web, "long", &"a".repeat(5000), 1);
        assert_eq!(lookup(&p, SearchEngine::Web, "long", 1).unwrap().0.text.len(), MAX_TEXT);
    }

    #[test]
    fn file_is_private_forgettable_and_bad_json_is_ignored() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("c");
        store(&p, SearchEngine::Web, "one", "t", 1);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(describe_all(&p).contains("\"one\""));
        assert_eq!(forget_all(&p), 1);
        assert!(!p.exists() && describe_all(&p) == "Search memory is empty.");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "not json").unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn dates() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_700_000_000), "2023-11-14");
        assert_eq!(date_of(1_788_000_000), "2026-08-29");
    }

    #[test]
    fn answer_paths_are_deterministic() {
        let p = tmp("ans");
        let boom = || -> Result<String, String> { panic!("must not fetch") };
        let down = || -> Result<String, String> { Err("offline".into()) };
        // nothing saved + offline = the error, nothing invented
        assert_eq!(answer(&p, SearchEngine::Wiki, "cats", 1000, false, down), Err("offline".into()));
        // fetch stores
        assert_eq!(answer(&p, SearchEngine::Wiki, "cats", 1000, false, || Ok::<_, String>("Cats text".into())).unwrap(), "Cats text");
        // fresh: from memory, no fetch, dated header and trailing label
        let r = answer(&p, SearchEngine::Wiki, "cats", 2000, false, boom).unwrap();
        assert!(r.starts_with("From memory (saved ") && r.contains("(From memory, saved ") && r.contains("refresh:"));
        // refresh forces a fetch
        assert_eq!(answer(&p, SearchEngine::Wiki, "cats", 2000, true, || Ok::<_, String>("New".into())).unwrap(), "New");
        // expired + offline = labelled stale copy
        let r = answer(&p, SearchEngine::Wiki, "cats", 2000 + 30 * 86_400, false, down).unwrap();
        assert!(r.contains("may be out of date") && r.contains("Don't rely on it") && r.contains("New"));
    }

    #[test]
    fn time_sensitive_questions_are_never_served_as_current() {
        let p = tmp("ts");
        for q in ["price of rtx 3060", "is the shop open now", "weather in cairo", "latest news on arch linux", "return policy for dell", "flight to paris"] {
            assert!(time_sensitive(q), "{q}");
        }
        for q in ["cats", "capital of france", "who wrote hamlet"] {
            assert!(!time_sensitive(q), "{q}");
        }
        store(&p, SearchEngine::Web, "price of milk", "Milk is 2.50", 1000);
        // fresh entry, but the question is time-sensitive: fetch again
        let r = answer(&p, SearchEngine::Web, "price of milk", 1001, false, || Ok::<_, String>("Milk is 3.10".into())).unwrap();
        assert_eq!(r, "Milk is 3.10");
        // offline: the old copy is shown only with the warning
        let r = answer(&p, SearchEngine::Web, "price of milk", 1002, false, || Err::<String, String>("offline".into())).unwrap();
        assert!(r.contains("may be out of date") && r.contains("prices, availability"));
    }
}
