//! Weather for a place the user names, from Open-Meteo (free, no account, no key).
//! Two requests: the place name goes to the geocoding API, then the coordinates of the match go to the forecast API.
//! Nothing is guessed: no place, no answer; no network, no answer. The reply always names its source and time.

use crate::error::PawError;
use crate::imagegen::Http;

pub const GEO_HOST: &str = "geocoding-api.open-meteo.com";
pub const FORECAST_HOST: &str = "api.open-meteo.com";
pub const SOURCE_LINK: &str = "https://open-meteo.com/";

/// What the confirmation card says before anything is sent.
pub fn disclosure(place: &str) -> String {
    format!(
        "Get the weather for \"{place}\" from Open-Meteo (online, free, no account). Pawline sends the place name to {GEO_HOST}. If several places match, you pick one first. Then only that place's coordinates go to {FORECAST_HOST}. Each request also carries the usual web details: your IP address and a \"Pawline\" user-agent. Pawline does not send your files, chat history or your own location. What Open-Meteo keeps is its own policy. The answer is only shown."
    )
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

/// WMO weather interpretation codes, as Open-Meteo documents them.
pub fn describe_code(c: i64) -> &'static str {
    match c {
        0 => "clear sky",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 | 63 | 65 => "rain",
        66 | 67 => "freezing rain",
        71 | 73 | 75 | 77 => "snow",
        80..=82 => "rain showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "conditions not described",
    }
}

fn get(http: &dyn Http, url: &str) -> Result<serde_json::Value, PawError> {
    let r = http
        .get(url, &[("User-Agent", "Pawline/0.1 (personal desktop assistant; https://github.com/linuxkiller9000/pawline)")])
        .map_err(|e| PawError::Exec(format!("Could not reach the weather service ({e}). Nothing was shown; check your connection and try again.")))?;
    if r.status != 200 {
        return Err(PawError::Exec(format!("The weather service answered with an error (HTTP {}). Nothing was shown; try again later.", r.status)));
    }
    serde_json::from_slice(&r.body).map_err(|_| PawError::Exec("The weather service sent an answer I could not read. Nothing was shown.".into()))
}

fn num(v: &serde_json::Value, p: &str) -> Option<f64> {
    v.pointer(p).and_then(|x| x.as_f64())
}

/// One geocoder match: what the user sees in the chooser, and the coordinates used only after they pick.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub label: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug)]
pub enum Found {
    One(Place),
    /// Several different places share the name: the user must pick one before any forecast is fetched.
    Many(Vec<Place>),
}

fn label(h: &serde_json::Value) -> String {
    let n = h.get("name").and_then(|x| x.as_str()).unwrap_or("?");
    let a = h.get("admin1").and_then(|x| x.as_str()).filter(|a| *a != n);
    let c = h.get("country").and_then(|x| x.as_str());
    [Some(n), a, c].into_iter().flatten().collect::<Vec<_>>().join(", ")
}

/// Step 1: the place name goes to the geocoder. "Cairo, Egypt" searches "Cairo" and keeps matches whose
/// region/country text contains the rest. Only places with the same name as the best match count as ambiguity.
pub fn geocode(http: &dyn Http, place: &str) -> Result<Found, PawError> {
    let place = place.trim();
    if place.is_empty() {
        return Err(PawError::Exec("Which place? For example: weather in Cairo".into()));
    }
    let mut parts = place.split(',').map(|p| p.trim()).filter(|p| !p.is_empty());
    let name = parts.next().unwrap_or(place).to_string();
    let hints: Vec<String> = parts.map(|p| p.to_lowercase()).collect();
    let g = get(http, &format!("https://{GEO_HOST}/v1/search?name={}&count=10&language=en&format=json", enc(&name)))?;
    let all = g.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let mut hits: Vec<&serde_json::Value> = all
        .iter()
        .filter(|h| {
            let l = label(h).to_lowercase();
            hints.iter().all(|x| l.contains(x.as_str()))
        })
        .collect();
    if let Some(first) = hits.first().map(|h| h.get("name").and_then(|x| x.as_str()).unwrap_or("").to_lowercase()) {
        hits.retain(|h| h.get("name").and_then(|x| x.as_str()).unwrap_or("").to_lowercase() == first);
    }
    let mut seen = std::collections::HashSet::new();
    let mut places = vec![];
    for h in hits {
        let (Some(lat), Some(lon)) = (h.get("latitude").and_then(|x| x.as_f64()), h.get("longitude").and_then(|x| x.as_f64())) else { continue };
        let l = label(h);
        if seen.insert(l.clone()) {
            places.push(Place { label: l, lat, lon });
        }
        if places.len() == 5 {
            break;
        }
    }
    match places.len() {
        0 => Err(PawError::Exec(format!("I could not find a place called \"{place}\". Try the city and country, like \"weather in Cairo, Egypt\"."))),
        1 => Ok(Found::One(places.remove(0))),
        _ => Ok(Found::Many(places)),
    }
}

/// The chooser text. Nothing has been fetched for any of these yet.
pub fn chooser_text(place: &str, options: &[Place]) -> String {
    let mut s = format!("More than one place is called \"{place}\". Type the number of the one you mean (nothing else is sent until you do), or anything else to cancel:");
    for (i, p) in options.iter().enumerate() {
        s.push_str(&format!("\n{}. {} ({:.2}, {:.2})", i + 1, p.label, p.lat, p.lon));
    }
    s
}

/// Step 2: only the chosen place's coordinates go to the forecast service.
pub fn forecast(http: &dyn Http, p: &Place) -> Result<String, PawError> {
    let (lat, lon) = (p.lat, p.lon);
    let f = get(
        http,
        &format!(
            "https://{FORECAST_HOST}/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone=auto&forecast_days=3"
        ),
    )?;
    let (Some(t), Some(code)) = (num(&f, "/current/temperature_2m"), num(&f, "/current/weather_code")) else {
        return Err(PawError::Exec("The forecast had no current conditions. Nothing was shown.".into()));
    };
    let at = f.pointer("/current/time").and_then(|x| x.as_str()).unwrap_or("?");
    let tz = f.get("timezone_abbreviation").and_then(|x| x.as_str()).unwrap_or("");
    let mut out = format!("Weather for {} ({:.2}, {:.2})\nModel estimate for now: {:.0}\u{b0}C, {}", p.label, lat, lon, t, describe_code(code as i64));
    if let Some(a) = num(&f, "/current/apparent_temperature") {
        out.push_str(&format!(", feels like {:.0}\u{b0}C", a));
    }
    if let Some(w) = num(&f, "/current/wind_speed_10m") {
        out.push_str(&format!(", wind {:.0} km/h", w));
    }
    let days = f.pointer("/daily/time").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    for (i, d) in days.iter().enumerate() {
        let at_i = |k: &str| f.pointer(&format!("/daily/{k}/{i}")).and_then(|x| x.as_f64());
        let (Some(hi), Some(lo)) = (at_i("temperature_2m_max"), at_i("temperature_2m_min")) else { continue };
        out.push_str(&format!("\n{}: {:.0} to {:.0}\u{b0}C", d.as_str().unwrap_or("?"), lo, hi));
        if let Some(c) = at_i("weather_code") {
            out.push_str(&format!(", {}", describe_code(c as i64)));
        }
        if let Some(p) = at_i("precipitation_probability_max") {
            out.push_str(&format!(", rain chance {:.0}%", p));
        }
    }
    out.push_str(&format!("\nForecast model time: {at} {tz} (as of, at that place). These are model values, not a reading from a weather station, and not a warning service.\nSource: Open-Meteo.com, {SOURCE_LINK} (data CC BY 4.0, free non-commercial service)"));
    Ok(out)
}

/// A clear single match goes straight to its forecast; several matches return the chooser text instead.
/// `pending` receives the options so the next number the user types can pick one.
pub fn weather(http: &dyn Http, place: &str, pending: &mut Option<Vec<Place>>) -> Result<String, PawError> {
    *pending = None;
    match geocode(http, place)? {
        Found::One(p) => forecast(http, &p),
        Found::Many(v) => {
            let t = chooser_text(place, &v);
            *pending = Some(v);
            Ok(t)
        }
    }
}

/// "weather in Cairo", "what's the weather like in New York today", "forecast for Paris". None when it is not a weather ask.
pub fn parse_place(lower: &str) -> Option<String> {
    let t = lower.trim().trim_end_matches(['?', '.', '!', ' ']);
    if !(t.contains("weather") || t.starts_with("forecast")) {
        return None;
    }
    let mut rest = t;
    for p in ["what's the ", "what is the ", "whats the ", "how's the ", "hows the ", "how is the ", "show me the ", "tell me the ", "get the ", "check the ", "check ", "give me the "] {
        if let Some(r) = rest.strip_prefix(p) {
            rest = r;
            break;
        }
    }
    if !(rest.starts_with("weather") || rest.starts_with("forecast")) {
        return None;
    }
    let rest = rest.strip_prefix("weather forecast").or_else(|| rest.strip_prefix("weather")).or_else(|| rest.strip_prefix("forecast")).unwrap_or(rest).trim();
    let rest = rest.strip_prefix("like").unwrap_or(rest).trim();
    let mut place = rest;
    for p in ["in ", "for ", "at ", "of "] {
        if let Some(r) = place.strip_prefix(p) {
            place = r;
            break;
        }
    }
    let mut place = place.trim();
    for s in [" today", " now", " right now", " tomorrow", " tonight", " this week"] {
        if let Some(r) = place.strip_suffix(s) {
            place = r.trim();
        }
    }
    Some(place.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imagegen::HttpReply;
    use std::cell::RefCell;

    struct Fake {
        replies: RefCell<Vec<Result<HttpReply, PawError>>>,
        urls: RefCell<Vec<String>>,
    }
    impl Http for Fake {
        fn get(&self, url: &str, _h: &[(&str, &str)]) -> Result<HttpReply, PawError> {
            self.urls.borrow_mut().push(url.to_string());
            self.replies.borrow_mut().remove(0)
        }
        fn post(&self, _u: &str, _h: &[(&str, &str)], _b: &str) -> Result<HttpReply, PawError> {
            panic!("weather never posts")
        }
    }
    fn ok(s: &str) -> Result<HttpReply, PawError> {
        Ok(HttpReply { status: 200, retry_after: None, content_type: "application/json".into(), body: s.as_bytes().to_vec() })
    }
    const GEO: &str = r#"{"results":[{"name":"Cairo","latitude":30.06263,"longitude":31.24967,"country":"Egypt","admin1":"Cairo Governorate"},{"name":"Cairo","latitude":37.0,"longitude":-89.1,"country":"United States","admin1":"Illinois"}]}"#;
    const FC: &str = r#"{"timezone_abbreviation":"GMT+2","current":{"time":"2026-11-26T02:00","temperature_2m":17.4,"apparent_temperature":16.1,"weather_code":2,"wind_speed_10m":9.2},"daily":{"time":["2026-11-26","2026-11-27"],"weather_code":[2,61],"temperature_2m_max":[24.1,22.0],"temperature_2m_min":[15.0,14.2],"precipitation_probability_max":[0,40]}}"#;

    const GEO1: &str = r#"{"results":[{"name":"Cairo","latitude":30.06263,"longitude":31.24967,"country":"Egypt","admin1":"Cairo Governorate"},{"name":"Cairo Town","latitude":1.0,"longitude":2.0,"country":"Elsewhere"}]}"#;
    fn fake(r: Vec<Result<HttpReply, PawError>>) -> Fake {
        Fake { replies: RefCell::new(r), urls: RefCell::new(vec![]) }
    }

    #[test]
    fn single_clear_match_goes_straight_to_the_forecast_with_model_time_wording() {
        let f = fake(vec![ok(GEO1), ok(FC)]);
        let mut pending = None;
        let r = weather(&f, "Cairo", &mut pending).unwrap();
        assert!(pending.is_none());
        assert!(r.contains("Weather for Cairo, Cairo Governorate, Egypt (30.06, 31.25)") && r.contains("Model estimate for now: 17\u{b0}C, partly cloudy") && r.contains("rain chance 40%"));
        assert!(r.contains("Forecast model time: 2026-11-26T02:00 GMT+2") && r.contains("not a reading from a weather station"));
        assert!(!r.contains("Observed"));
        assert!(r.contains("Source: Open-Meteo.com, https://open-meteo.com/"));
        let u = f.urls.borrow();
        assert!(u[0].starts_with("https://geocoding-api.open-meteo.com/") && u[0].contains("name=Cairo"));
        assert!(u[1].starts_with("https://api.open-meteo.com/") && u[1].contains("latitude=30.0626") && !u[1].contains("Cairo"));
    }

    #[test]
    fn same_name_places_are_a_chooser_and_no_forecast_is_fetched_until_a_pick() {
        let f = fake(vec![ok(GEO)]);
        let mut pending = None;
        let t = weather(&f, "Cairo", &mut pending).unwrap();
        assert_eq!(f.urls.borrow().len(), 1, "only the geocoder was asked");
        let opts = pending.clone().unwrap();
        assert_eq!(opts.len(), 2);
        assert!(t.contains("1. Cairo, Cairo Governorate, Egypt (30.06, 31.25)") && t.contains("2. Cairo, Illinois, United States (37.00, -89.10)") && t.contains("nothing else is sent until you do"));
        // picking 2 fetches exactly that place
        let f2 = fake(vec![ok(FC)]);
        let r = forecast(&f2, &opts[1]).unwrap();
        assert!(r.contains("Weather for Cairo, Illinois, United States"));
        assert!(f2.urls.borrow()[0].contains("latitude=37.0000") && f2.urls.borrow()[0].contains("longitude=-89.1000"));
    }

    #[test]
    fn country_hint_picks_one_place_and_other_names_do_not_count_as_ambiguity() {
        let f = fake(vec![ok(GEO)]);
        match geocode(&f, "Cairo, Egypt").unwrap() {
            Found::One(p) => assert_eq!(p.label, "Cairo, Cairo Governorate, Egypt"),
            Found::Many(_) => panic!("country given"),
        }
        let f = fake(vec![ok(GEO1)]);
        assert!(matches!(geocode(&f, "Cairo").unwrap(), Found::One(_)), "Cairo Town is a different name");
        let f = fake(vec![ok(GEO)]);
        assert!(geocode(&f, "Cairo, Atlantis").unwrap_err().to_string().contains("could not find"));
    }

    #[test]
    fn failures_say_what_happened_and_show_no_weather() {
        let mut pending = None;
        let e = weather(&fake(vec![Err(PawError::Exec("offline".into()))]), "Cairo", &mut pending).unwrap_err().to_string();
        assert!(e.contains("Could not reach") && e.contains("Nothing was shown"));
        assert!(weather(&fake(vec![ok(r#"{"generationtime_ms":0.1}"#)]), "Zzzzzz", &mut pending).unwrap_err().to_string().contains("could not find a place"));
        let bad = fake(vec![Ok(HttpReply { status: 429, retry_after: None, content_type: String::new(), body: vec![] })]);
        assert!(weather(&bad, "Cairo", &mut pending).unwrap_err().to_string().contains("HTTP 429"));
        assert!(weather(&fake(vec![ok(GEO1), ok("not json")]), "Cairo", &mut pending).unwrap_err().to_string().contains("could not read"));
        assert!(weather(&fake(vec![]), "  ", &mut pending).unwrap_err().to_string().contains("Which place"), "no network call without a place");
    }

    #[test]
    fn place_parsing() {
        for (q, p) in [("weather in cairo", "cairo"), ("what's the weather like in new york today?", "new york"), ("forecast for paris", "paris"), ("weather cairo egypt", "cairo egypt"), ("what is the weather", "")] {
            assert_eq!(parse_place(q).as_deref(), Some(p), "{q}");
        }
        assert!(parse_place("make a folder called weather").is_none() && parse_place("delete weather.txt").is_none());
        assert!(parse_place("how do i fix my cpu").is_none());
    }

    #[test]
    fn disclosure_names_hosts_and_limits() {
        let d = disclosure("Cairo");
        assert!(d.contains(GEO_HOST) && d.contains(FORECAST_HOST) && d.contains("IP address") && d.contains("no account") && d.contains("does not send your files") && !d.contains("Nothing else is sent"));
    }
}
