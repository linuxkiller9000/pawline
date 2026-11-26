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
        "Get the weather for \"{place}\" from Open-Meteo (online): this sends the place name to {GEO_HOST}, then that place's coordinates to {FORECAST_HOST}, plus your IP address (free, no account). Nothing else is sent: no files, no history, no exact location of yours. The answer is only shown."
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
        .get(url, &[("User-Agent", "Pawline/0.1 (personal desktop assistant)")])
        .map_err(|e| PawError::Exec(format!("Could not reach the weather service ({e}). Nothing was shown; check your connection and try again.")))?;
    if r.status != 200 {
        return Err(PawError::Exec(format!("The weather service answered with an error (HTTP {}). Nothing was shown; try again later.", r.status)));
    }
    serde_json::from_slice(&r.body).map_err(|_| PawError::Exec("The weather service sent an answer I could not read. Nothing was shown.".into()))
}

fn num(v: &serde_json::Value, p: &str) -> Option<f64> {
    v.pointer(p).and_then(|x| x.as_f64())
}

pub fn weather(http: &dyn Http, place: &str) -> Result<String, PawError> {
    let place = place.trim();
    if place.is_empty() {
        return Err(PawError::Exec("Which place? For example: weather in Cairo".into()));
    }
    let g = get(http, &format!("https://{GEO_HOST}/v1/search?name={}&count=3&language=en&format=json", enc(place)))?;
    let hits = g.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let Some(first) = hits.first() else {
        return Err(PawError::Exec(format!("I could not find a place called \"{place}\". Try the city and country, like \"weather in Cairo, Egypt\".")));
    };
    let name = |h: &serde_json::Value| {
        let n = h.get("name").and_then(|x| x.as_str()).unwrap_or("?");
        let a = h.get("admin1").and_then(|x| x.as_str()).filter(|a| *a != n);
        let c = h.get("country").and_then(|x| x.as_str());
        [Some(n), a, c].into_iter().flatten().collect::<Vec<_>>().join(", ")
    };
    let (Some(lat), Some(lon)) = (first.get("latitude").and_then(|x| x.as_f64()), first.get("longitude").and_then(|x| x.as_f64())) else {
        return Err(PawError::Exec("The place search gave no coordinates. Nothing was shown.".into()));
    };
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
    let mut out = format!("Weather for {}\nNow: {:.0}\u{b0}C, {}", name(first), t, describe_code(code as i64));
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
    if hits.len() > 1 {
        let others: Vec<String> = hits.iter().skip(1).map(name).collect();
        out.push_str(&format!("\nOther places with this name: {}. Say the country to pick another.", others.join("; ")));
    }
    out.push_str(&format!("\nObserved {at} {tz} (place time). Forecasts change; this is a model forecast, not a warning service.\nSource: Open-Meteo.com, {SOURCE_LINK} (data CC BY 4.0)"));
    Ok(out)
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

    #[test]
    fn answer_names_place_values_time_and_source() {
        let f = Fake { replies: RefCell::new(vec![ok(GEO), ok(FC)]), urls: RefCell::new(vec![]) };
        let r = weather(&f, "Cairo").unwrap();
        assert!(r.contains("Weather for Cairo, Cairo Governorate, Egypt") && r.contains("Now: 17\u{b0}C, partly cloudy") && r.contains("rain chance 40%"));
        assert!(r.contains("Other places with this name: Cairo, Illinois, United States"));
        assert!(r.contains("Source: Open-Meteo.com, https://open-meteo.com/") && r.contains("Observed 2026-11-26T02:00"));
        let u = f.urls.borrow();
        assert!(u[0].starts_with("https://geocoding-api.open-meteo.com/") && u[0].contains("name=Cairo"));
        assert!(u[1].starts_with("https://api.open-meteo.com/") && u[1].contains("latitude=30.0626"), "coordinates of the first match only: {}", u[1]);
        assert!(!u[1].contains("Cairo"), "forecast request carries no place text");
    }

    #[test]
    fn failures_say_what_happened_and_show_no_weather() {
        let down = Fake { replies: RefCell::new(vec![Err(PawError::Exec("offline".into()))]), urls: RefCell::new(vec![]) };
        let e = weather(&down, "Cairo").unwrap_err().to_string();
        assert!(e.contains("Could not reach") && e.contains("Nothing was shown"));
        let none = Fake { replies: RefCell::new(vec![ok(r#"{"generationtime_ms":0.1}"#)]), urls: RefCell::new(vec![]) };
        assert!(weather(&none, "Zzzzzz").unwrap_err().to_string().contains("could not find a place"));
        let bad = Fake { replies: RefCell::new(vec![Ok(HttpReply { status: 429, retry_after: None, content_type: String::new(), body: vec![] })]), urls: RefCell::new(vec![]) };
        assert!(weather(&bad, "Cairo").unwrap_err().to_string().contains("HTTP 429"));
        let junk = Fake { replies: RefCell::new(vec![ok(GEO), ok("not json")]), urls: RefCell::new(vec![]) };
        assert!(weather(&junk, "Cairo").unwrap_err().to_string().contains("could not read"));
        let empty = Fake { replies: RefCell::new(vec![]), urls: RefCell::new(vec![]) };
        assert!(weather(&empty, "  ").unwrap_err().to_string().contains("Which place"), "no network call without a place");
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
        assert!(d.contains(GEO_HOST) && d.contains(FORECAST_HOST) && d.contains("IP address") && d.contains("no account") && d.contains("no files"));
    }
}
