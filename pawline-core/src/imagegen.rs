//! Explicit, free, online image generation. Only runs after the user named a
//! provider and confirmed the card. Sends the prompt text (and, as with any
//! web request, the IP address) and nothing else: no files, no history.
//! There is no payment path: a "payment required"/rate-limit answer is
//! reported as a plain failure.

use crate::error::PawError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProvider {
    /// pollinations.ai "Classic" anonymous tier.
    Pollinations,
    /// AI Horde: volunteer-run, anonymous key, may queue for minutes.
    Horde,
}

impl ImageProvider {
    pub fn label(self) -> &'static str {
        match self {
            ImageProvider::Pollinations => "Pollinations",
            ImageProvider::Horde => "AI Horde",
        }
    }
    /// What the user is told leaves the machine, shown on the confirm card.
    pub fn disclosure(self) -> &'static str {
        match self {
            ImageProvider::Pollinations => "sends your prompt text and your IP address to pollinations.ai (free, no account; results can be cached under their URL; one request at a time)",
            ImageProvider::Horde => "sends your prompt text and your IP address to aihorde.net (free, anonymous, run by volunteers; prompts may be logged; anonymous requests are lowest priority and can take minutes or time out)",
        }
    }
}

pub const MAX_PROMPT_CHARS: usize = 300;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const UA: &str = "Pawline/0.2 (personal desktop assistant; https://github.com/linuxkiller9000/pawline)";
const HORDE_BASE: &str = "https://aihorde.net/api/v2";
const HORDE_ANON_KEY: &str = "0000000000";

pub struct HttpReply {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Network seam so the logic is testable without live calls.
pub trait Http {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpReply, PawError>;
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpReply, PawError>;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new(timeout: Duration) -> Self {
        let cfg = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build();
        UreqHttp { agent: ureq::Agent::new_with_config(cfg) }
    }
    fn finish(mut resp: ureq::http::Response<ureq::Body>) -> Result<HttpReply, PawError> {
        let status = resp.status().as_u16();
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok());
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_IMAGE_BYTES)
            .read_to_vec()
            .map_err(|e| PawError::Exec(format!("could not read the reply: {e}")))?;
        Ok(HttpReply { status, retry_after, content_type, body })
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpReply, PawError> {
        let mut r = self.agent.get(url).header("User-Agent", UA);
        for (k, v) in headers {
            r = r.header(*k, *v);
        }
        Self::finish(r.call().map_err(|e| PawError::Exec(format!("network problem: {e}")))?)
    }
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpReply, PawError> {
        let mut r = self.agent.post(url).header("User-Agent", UA).header("Content-Type", "application/json");
        for (k, v) in headers {
            r = r.header(*k, *v);
        }
        Self::finish(r.send(body).map_err(|e| PawError::Exec(format!("network problem: {e}")))?)
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

/// Clean a user prompt: single line, no control characters, capped.
pub fn clean_prompt(p: &str) -> Result<String, PawError> {
    let s: String = p.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return Err(PawError::Validation("What should the picture show?".into()));
    }
    let n = s.chars().count();
    if n > MAX_PROMPT_CHARS {
        return Err(PawError::Validation(format!("That description is {n} characters; the limit is {MAX_PROMPT_CHARS}. I won't cut it for you. Shorten it and ask again. Nothing was sent.")));
    }
    Ok(s)
}

pub fn pollinations_url(prompt: &str) -> String {
    format!("https://image.pollinations.ai/prompt/{}?width=512&height=512", enc(prompt))
}

fn image_ext(body: &[u8]) -> Option<&'static str> {
    if body.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if body.len() > 12 && &body[0..4] == b"RIFF" && &body[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn limit_failure(provider: ImageProvider, r: &HttpReply) -> PawError {
    let wait = r.retry_after.map(|s| format!(" Try again in about {s} seconds.")).unwrap_or_default();
    PawError::Exec(format!(
        "{} said its free limit is reached right now (HTTP {}).{wait} I never pay or upgrade, so nothing was made.",
        provider.label(),
        r.status
    ))
}

fn save(dir: &Path, provider: ImageProvider, body: &[u8]) -> Result<PathBuf, PawError> {
    let ext = image_ext(body).ok_or_else(|| PawError::Exec(format!("{} sent something that is not a PNG, JPEG or WebP image; nothing was saved.", provider.label())))?;
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let tag = match provider { ImageProvider::Pollinations => "pollinations", ImageProvider::Horde => "horde" };
    let mut n = 0u32;
    loop {
        let name = if n == 0 { format!("pawline-{stamp}-{tag}.{ext}") } else { format!("pawline-{stamp}-{tag}-{n}.{ext}") };
        let p = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(body)?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 50 => n += 1,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Run one generation with the chosen provider. `poll_every` and `give_up`
/// only matter for the Horde queue.
pub fn generate(
    http: &dyn Http,
    provider: ImageProvider,
    prompt: &str,
    dir: &Path,
    poll_every: Duration,
    give_up: Duration,
    cancelled: &dyn Fn() -> bool,
    commit: &dyn Fn() -> bool,
) -> Result<PathBuf, PawError> {
    let prompt = clean_prompt(prompt)?;
    let stop = || PawError::Exec("Cancelled. Nothing was saved.".into());
    match provider {
        ImageProvider::Pollinations => {
            let r = http.get(&pollinations_url(&prompt), &[])?;
            if cancelled() {
                return Err(stop());
            }
            match r.status {
                200 => {
                    if !commit() {
                        return Err(stop());
                    }
                    save(dir, provider, &r.body)
                }
                402 | 429 => Err(limit_failure(provider, &r)),
                s => Err(PawError::Exec(format!("Pollinations answered HTTP {s}; nothing was made."))),
            }
        }
        ImageProvider::Horde => {
            let body = serde_json::json!({
                "prompt": prompt,
                "params": {"width": 512, "height": 512, "steps": 20, "n": 1},
                "nsfw": false,
                "censor_nsfw": true,
                "r2": true
            })
            .to_string();
            let key = [("apikey", HORDE_ANON_KEY)];
            let r = http.post(&format!("{HORDE_BASE}/generate/async"), &key, &body)?;
            if r.status == 402 || r.status == 429 {
                return Err(limit_failure(provider, &r));
            }
            if r.status != 202 && r.status != 200 {
                let msg = serde_json::from_slice::<serde_json::Value>(&r.body).ok().and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from)).unwrap_or_default();
                return Err(PawError::Exec(format!("AI Horde refused the request (HTTP {}). {msg}", r.status)));
            }
            let id = serde_json::from_slice::<serde_json::Value>(&r.body)
                .ok()
                .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
                .ok_or_else(|| PawError::Exec("AI Horde did not return a request id.".into()))?;
            let start = Instant::now();
            loop {
                if cancelled() {
                    return Err(stop());
                }
                let s = http.get(&format!("{HORDE_BASE}/generate/status/{}", enc(&id)), &[])?;
                if s.status == 200 {
                    let v: serde_json::Value = serde_json::from_slice(&s.body).map_err(|_| PawError::Exec("AI Horde status was unreadable.".into()))?;
                    if v.get("faulted").and_then(|f| f.as_bool()) == Some(true) {
                        return Err(PawError::Exec("AI Horde reported the job failed; nothing was made.".into()));
                    }
                    if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
                        if v.pointer("/generations/0/censored").and_then(|c| c.as_bool()) == Some(true) {
                            return Err(PawError::Exec("AI Horde filtered that picture; nothing was saved.".into()));
                        }
                        let url = v.pointer("/generations/0/img").and_then(|u| u.as_str()).ok_or_else(|| PawError::Exec("AI Horde finished without an image.".into()))?;
                        if !url.starts_with("https://") {
                            return Err(PawError::Exec("AI Horde returned a non-https image link; refused.".into()));
                        }
                        let img = http.get(url, &[])?;
                        if img.status != 200 {
                            return Err(PawError::Exec(format!("Could not download the finished image (HTTP {}).", img.status)));
                        }
                        if cancelled() || !commit() {
                            return Err(stop());
                        }
                        return save(dir, provider, &img.body);
                    }
                } else if s.status == 402 || s.status == 429 {
                    return Err(limit_failure(provider, &s));
                }
                if start.elapsed() >= give_up {
                    let _ = http.get(&format!("{HORDE_BASE}/generate/status/{}", enc(&id)), &[]);
                    return Err(PawError::Exec(format!(
                        "AI Horde had not finished after {} seconds (anonymous requests are lowest priority). Nothing was saved; try again later or use Pollinations.",
                        give_up.as_secs()
                    )));
                }
                std::thread::sleep(poll_every);
            }
        }
    }
}

/// One request per provider at a time, counted until the network thread really
/// ends (an abandoned request still counts). Pollinations Classic also asks for
/// at least 45 s between requests; asking sooner just earns a 402.
pub struct Guard {
    state: std::sync::Mutex<[(bool, Option<Instant>); 2]>,
}
pub struct Ticket<'a> {
    guard: &'a Guard,
    slot: usize,
}
impl std::fmt::Debug for Ticket<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ticket({})", self.slot)
    }
}
impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.guard.state.lock() {
            s[self.slot].0 = false;
        }
    }
}
pub const POLLINATIONS_SPACING: Duration = Duration::from_secs(45);
impl Guard {
    pub const fn new() -> Self {
        Guard { state: std::sync::Mutex::new([(false, None), (false, None)]) }
    }
    pub fn begin(&self, provider: ImageProvider, now: Instant) -> Result<Ticket<'_>, PawError> {
        let slot = match provider { ImageProvider::Pollinations => 0, ImageProvider::Horde => 1 };
        let mut s = self.state.lock().map_err(|_| PawError::Exec("internal lock problem".into()))?;
        if s[slot].0 {
            return Err(PawError::Exec(format!("An earlier {} request is still running (even if you cancelled it). I send one at a time. Try again in a moment; nothing was sent.", provider.label())));
        }
        if provider == ImageProvider::Pollinations {
            if let Some(t) = s[slot].1 {
                let since = now.saturating_duration_since(t);
                if since < POLLINATIONS_SPACING {
                    return Err(PawError::Exec(format!("Pollinations asks for about {} seconds between free requests. Try again in {} seconds; nothing was sent.", POLLINATIONS_SPACING.as_secs(), (POLLINATIONS_SPACING - since).as_secs() + 1)));
                }
            }
        }
        s[slot] = (true, Some(now));
        Ok(Ticket { guard: self, slot })
    }
}
pub static GUARD: Guard = Guard::new();

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        replies: RefCell<Vec<HttpReply>>,
        seen: RefCell<Vec<String>>,
    }
    fn rep(status: u16, ct: &str, body: &[u8]) -> HttpReply {
        HttpReply { status, retry_after: None, content_type: ct.into(), body: body.to_vec() }
    }
    impl Http for Fake {
        fn get(&self, url: &str, _h: &[(&str, &str)]) -> Result<HttpReply, PawError> {
            self.seen.borrow_mut().push(format!("GET {url}"));
            Ok(self.replies.borrow_mut().remove(0))
        }
        fn post(&self, url: &str, h: &[(&str, &str)], b: &str) -> Result<HttpReply, PawError> {
            self.seen.borrow_mut().push(format!("POST {url} key={} body={b}", h.iter().find(|x| x.0 == "apikey").map(|x| x.1).unwrap_or("")));
            Ok(self.replies.borrow_mut().remove(0))
        }
    }
    fn fake(r: Vec<HttpReply>) -> Fake {
        Fake { replies: RefCell::new(r), seen: RefCell::new(vec![]) }
    }
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pl-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];
    const D0: Duration = Duration::from_millis(1);

    #[test]
    fn url_is_encoded_and_has_no_extra_params() {
        let u = pollinations_url("a cat & a hat/ü");
        assert_eq!(u, "https://image.pollinations.ai/prompt/a%20cat%20%26%20a%20hat%2F%C3%BC?width=512&height=512");
    }
    #[test]
    fn prompt_cleaning() {
        assert_eq!(clean_prompt("a\ncat \t here").unwrap(), "a cat here");
        assert!(clean_prompt("   ").is_err());
        assert!(clean_prompt(&"x".repeat(301)).is_err());
    }
    #[test]
    fn pollinations_saves_png() {
        let d = tmp("p1");
        let h = fake(vec![rep(200, "image/png", PNG)]);
        let p = generate(&h, ImageProvider::Pollinations, "a red kite", &d, D0, D0, &|| false, &|| true).unwrap();
        assert!(p.extension().unwrap() == "png" && std::fs::read(&p).unwrap() == PNG);
        assert!(h.seen.borrow()[0].starts_with("GET https://image.pollinations.ai/prompt/a%20red%20kite"));
    }
    #[test]
    fn pollinations_402_is_a_plain_failure_with_no_retry() {
        let d = tmp("p2");
        let mut r = rep(402, "application/json", b"{}");
        r.retry_after = Some(45);
        let h = fake(vec![r]);
        let e = generate(&h, ImageProvider::Pollinations, "x", &d, D0, D0, &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("45 seconds") && e.contains("never pay"), "{e}");
        assert_eq!(h.seen.borrow().len(), 1);
        assert!(!d.exists());
    }
    #[test]
    fn non_image_body_is_refused() {
        let d = tmp("p3");
        let h = fake(vec![rep(200, "image/png", b"<html>nope</html>")]);
        assert!(generate(&h, ImageProvider::Pollinations, "x", &d, D0, D0, &|| false, &|| true).is_err());
        assert!(!d.exists());
    }
    #[test]
    fn horde_flow_uses_anon_key_and_saves() {
        let d = tmp("h1");
        let h = fake(vec![
            rep(202, "application/json", br#"{"id":"abc-123"}"#),
            rep(200, "application/json", br#"{"done":false}"#),
            rep(200, "application/json", br#"{"done":true,"generations":[{"img":"https://r2.example/x.webp"}]}"#),
            rep(200, "image/webp", b"RIFF\x00\x00\x00\x00WEBPxx"),
        ]);
        let p = generate(&h, ImageProvider::Horde, "a lighthouse", &d, D0, Duration::from_secs(5), &|| false, &|| true).unwrap();
        assert!(p.extension().unwrap() == "webp");
        let seen = h.seen.borrow();
        assert!(seen[0].contains("key=0000000000") && seen[0].contains("\"prompt\":\"a lighthouse\"") && seen[0].contains("censor_nsfw"));
        assert!(seen[1].ends_with("/generate/status/abc-123"));
    }
    #[test]
    fn horde_timeout_is_honest() {
        let d = tmp("h2");
        let mut v = vec![rep(202, "application/json", br#"{"id":"q"}"#)];
        for _ in 0..200 { v.push(rep(200, "application/json", br#"{"done":false}"#)); }
        let h = fake(v);
        let e = generate(&h, ImageProvider::Horde, "x", &d, Duration::from_millis(2), Duration::from_millis(20), &|| false, &|| true).unwrap_err().to_string();
        assert!(e.contains("had not finished") && !d.exists(), "{e}");
    }
    #[test]
    fn horde_refuses_http_image_link_and_censored() {
        let d = tmp("h3");
        let h = fake(vec![rep(202, "", br#"{"id":"q"}"#), rep(200, "", br#"{"done":true,"generations":[{"img":"http://x/y.png"}]}"#)]);
        assert!(generate(&h, ImageProvider::Horde, "x", &d, D0, Duration::from_secs(1), &|| false, &|| true).unwrap_err().to_string().contains("non-https"));
        let h = fake(vec![rep(202, "", br#"{"id":"q"}"#), rep(200, "", br#"{"done":true,"generations":[{"img":"https://x/y.png","censored":true}]}"#)]);
        assert!(generate(&h, ImageProvider::Horde, "x", &d, D0, Duration::from_secs(1), &|| false, &|| true).unwrap_err().to_string().contains("filtered"));
    }

    #[test]
    fn cancel_discards_late_result_and_saves_nothing() {
        let d = tmp("c1");
        let h = fake(vec![rep(200, "image/png", PNG)]);
        let e = generate(&h, ImageProvider::Pollinations, "x", &d, D0, D0, &|| true, &|| true).unwrap_err().to_string();
        assert!(e.contains("Cancelled") && !d.exists(), "{e}");
        let h = fake(vec![rep(202, "", br#"{"id":"q"}"#)]);
        let e = generate(&h, ImageProvider::Horde, "x", &d, D0, Duration::from_secs(1), &|| true, &|| true).unwrap_err().to_string();
        assert!(e.contains("Cancelled") && !d.exists());
    }
    #[test]
    fn long_prompt_is_refused_not_truncated() {
        let e = clean_prompt(&"y".repeat(450)).unwrap_err().to_string();
        assert!(e.contains("450") && e.contains("won't cut it"), "{e}");
    }

    #[test]
    fn guard_one_inflight_and_spacing_even_after_abandon() {
        let g = Guard::new();
        let t0 = Instant::now();
        let t = g.begin(ImageProvider::Pollinations, t0).unwrap();
        // abandoned by the UI but the thread still runs: a new request is refused
        assert!(g.begin(ImageProvider::Pollinations, t0).unwrap_err().to_string().contains("still running"));
        // the other provider is independent
        let h = g.begin(ImageProvider::Horde, t0).unwrap();
        drop(t);
        // thread ended, but spacing still applies
        let e = g.begin(ImageProvider::Pollinations, t0 + Duration::from_secs(10)).unwrap_err().to_string();
        assert!(e.contains("36 seconds"), "{e}");
        assert!(g.begin(ImageProvider::Pollinations, t0 + Duration::from_secs(46)).is_ok());
        drop(h);
        assert!(g.begin(ImageProvider::Horde, t0).is_ok());
    }
    #[test]
    fn commit_denied_means_no_file_even_with_a_good_answer() {
        let d = tmp("c2");
        let h = fake(vec![rep(200, "image/png", PNG)]);
        let e = generate(&h, ImageProvider::Pollinations, "x", &d, D0, D0, &|| false, &|| false).unwrap_err().to_string();
        assert!(e.contains("Cancelled") && !d.exists(), "{e}");
    }

    /// Live smoke, run by hand only: cargo test -p pawline-core live_ -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_pollinations() {
        let d = tmp("live-p");
        let h = UreqHttp::new(Duration::from_secs(90));
        let p = generate(&h, ImageProvider::Pollinations, "a small red kite over a green hill", &d, D0, D0, &|| false, &|| true);
        println!("RESULT {:?} {:?}", p, p.as_ref().ok().map(|p| std::fs::metadata(p).map(|m| m.len())));
    }
    #[test]
    #[ignore]
    fn live_horde() {
        let d = tmp("live-h");
        let h = UreqHttp::new(Duration::from_secs(90));
        let p = generate(&h, ImageProvider::Horde, "a small red kite over a green hill", &d, Duration::from_secs(4), Duration::from_secs(100), &|| false, &|| true);
        println!("RESULT {:?} {:?}", p, p.as_ref().ok().map(|p| std::fs::metadata(p).map(|m| m.len())));
    }
}
