//! Provider-agnostic chat backends for online/LLM chat turns.
//!
//! Pawline's chat can talk to any server that speaks the OpenAI
//! chat-completions protocol: a local runner on this machine (Ollama,
//! llama.cpp's server, LM Studio, vLLM, ...) or a hosted endpoint
//! (the built-in default stays Pollinations, which needs no account).
//!
//! Rules baked in here:
//! - The API key (when a server needs one) lives only in the config file,
//!   written owner-only (0600). It is never shown in the UI, never put in
//!   a plan, a card, a log line or an error, and `Debug`/`Display` redact it.
//! - A card always names the exact endpoint and model before anything is
//!   sent, and says plainly whether text leaves the machine.
//! - Pawline never pays, upgrades or bypasses a limit; an unavailable
//!   server just means no answer, reported honestly.

use crate::error::PawError;
use std::path::{Path, PathBuf};

/// One chat backend. `api_key` is secret: it never leaves this struct
/// except into the `Authorization` header of the request itself.
#[derive(Clone)]
pub struct ChatProvider {
    /// Human label for cards and answers, e.g. "Ollama (this computer)".
    pub name: String,
    /// Full chat-completions URL, e.g. "http://127.0.0.1:11434/v1/chat/completions".
    pub url: String,
    /// Model name sent in the JSON body.
    pub model: String,
    /// Optional bearer key. Never displayed anywhere.
    api_key: Option<String>,
    /// True when the endpoint is a loopback address (text stays on this machine).
    pub local: bool,
}

/// The non-secret snapshot that travels inside a plan so the confirm card
/// can name exactly what will be used. Equality with the live config is
/// checked again at execution time; a mismatch refuses to send.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProviderSpec {
    pub name: String,
    pub url: String,
    pub model: String,
    pub local: bool,
}

impl core::fmt::Debug for ChatProvider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ChatProvider")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("local", &self.local)
            .finish()
    }
}

impl ChatProvider {
    /// The built-in default: Pollinations anonymous tier (unchanged behaviour).
    pub fn pollinations() -> Self {
        ChatProvider {
            name: "Pollinations".into(),
            url: "https://text.pollinations.ai/openai".into(),
            model: "openai".into(),
            api_key: None,
            local: false,
        }
    }

    /// Ollama serving on this machine (default port 11434).
    pub fn ollama(model: &str) -> Self {
        ChatProvider {
            name: "Ollama (this computer)".into(),
            url: "http://127.0.0.1:11434/v1/chat/completions".into(),
            model: model.into(),
            api_key: None,
            local: true,
        }
    }

    /// llama.cpp's `llama-server` on this machine (default port 8080).
    pub fn llama_cpp(model: &str) -> Self {
        ChatProvider {
            name: "llama.cpp server (this computer)".into(),
            url: "http://127.0.0.1:8080/v1/chat/completions".into(),
            model: model.into(),
            api_key: None,
            local: true,
        }
    }

    /// LM Studio's local server (default port 1234).
    pub fn lm_studio(model: &str) -> Self {
        ChatProvider {
            name: "LM Studio (this computer)".into(),
            url: "http://127.0.0.1:1234/v1/chat/completions".into(),
            model: model.into(),
            api_key: None,
            local: true,
        }
    }

    /// Any other OpenAI-compatible endpoint, local or hosted. `base_url` is
    /// the server root; `/v1/chat/completions` is appended unless the URL
    /// already ends with it or with `/chat/completions`.
    pub fn openai_compatible(name: &str, base_url: &str, model: &str, api_key: Option<String>) -> Self {
        let base = base_url.trim().trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/v1/chat/completions")
        };
        let local = is_loopback(&url);
        ChatProvider {
            name: if local { format!("{name} (this computer)") } else { name.to_string() },
            url,
            model: model.into(),
            api_key,
            local,
        }
    }

    /// The non-secret snapshot for plans and cards.
    pub fn spec(&self) -> ProviderSpec {
        ProviderSpec {
            name: self.name.clone(),
            url: self.url.clone(),
            model: self.model.clone(),
            local: self.local,
        }
    }

    /// Headers for the request. The key appears only here, in the
    /// `Authorization` header of the request itself.
    pub fn headers(&self) -> Vec<(String, String)> {
        match &self.api_key {
            Some(k) if !k.is_empty() => Vec::from([("Authorization".to_string(), format!("Bearer {k}"))]),
            _ => Vec::new(),
        }
    }

    /// One card line: where the text goes.
    pub fn destination_line(&self) -> String {
        if self.local {
            format!(
                "The request goes to {} on this computer (loopback only); your text does not leave the machine. The runner is a separate program: its own model downloads, updates or telemetry are its settings, not Pawline's.",
                self.url
            )
        } else {
            format!(
                "The request goes over the internet to {} ({}): your message text and IP address are visible to that service.",
                self.url, self.name
            )
        }
    }

    /// Honest "could not reach" text, named per provider.
    pub fn unreachable_error(&self) -> PawError {
        if self.local {
            PawError::Exec(format!(
                "{} could not be reached at {}. The server is probably not running (start it, or pick another provider with \"chat provider\"). Nothing was sent, and Pawline did not retry. Offline help and commands still work.",
                self.name, self.url
            ))
        } else {
            PawError::Exec(format!(
                "{} could not be reached (offline, or the service is down). There is no answer, and Pawline did not retry. Offline help and commands still work; try again later or say \"chat off\".",
                self.name
            ))
        }
    }

    /// Answer label prefix, so a reply always carries where it came from.
    pub fn answer_label(&self) -> String {
        if self.local {
            format!("{} chat, model \"{}\" (on this computer, not checked, may be wrong)", self.name, self.model)
        } else {
            format!("{} chat (online, not checked, may be wrong)", self.name)
        }
    }
}

/// True for loopback URLs (127.x, ::1, localhost).
fn is_loopback(url: &str) -> bool {
    let u = url.to_lowercase();
    u.starts_with("http://127.")
        || u.starts_with("https://127.")
        || u.starts_with("http://localhost")
        || u.starts_with("https://localhost")
        || u.starts_with("http://[::1]")
        || u.starts_with("https://[::1]")
}

// ─── Config file ────────────────────────────────────────────────────────────
//
// $<data>/chat-provider.json, written owner-only. Shape:
// {
//   "type": "pollinations" | "ollama" | "llama.cpp" | "lmstudio" | "openai-compatible",
//   "model": "...",                     // required except for pollinations
//   "base_url": "http://...",           // openai-compatible only
//   "name": "...",                      // openai-compatible only, display label
//   "api_key": "..."                    // optional, secret, never shown
// }

/// Where the config lives: $PAWLINE_DATA, else $XDG_DATA_HOME/pawline, else
/// ~/.local/share/pawline — the same root as Pawline's other data files.
pub fn config_path() -> Option<PathBuf> {
    let dir = if let Some(d) = std::env::var_os("PAWLINE_DATA") {
        PathBuf::from(d)
    } else if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(d).join("pawline")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".local/share/pawline")
    };
    Some(dir.join("chat-provider.json"))
}

/// The configured provider, or the Pollinations default when there is no
/// file or the file cannot be parsed (a broken file never blocks chat:
/// the default is what earlier Pawline versions always used).
pub fn load(path: &Path) -> ChatProvider {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ChatProvider::pollinations();
    };
    parse_config(&raw).unwrap_or_else(ChatProvider::pollinations)
}

/// Parse one config document. Kept separate from `load` so tests never
/// touch the filesystem for the happy paths.
pub fn parse_config(raw: &str) -> Option<ChatProvider> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let ty = v["type"].as_str()?;
    let model = v["model"].as_str().unwrap_or("").trim();
    let key = v["api_key"].as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    match ty {
        "pollinations" => Some(ChatProvider::pollinations()),
        "ollama" if !model.is_empty() => {
            let mut p = ChatProvider::ollama(model);
            p.api_key = key;
            Some(p)
        }
        "llama.cpp" | "llamacpp" | "llama-server" if !model.is_empty() => {
            let mut p = ChatProvider::llama_cpp(model);
            p.api_key = key;
            Some(p)
        }
        "lmstudio" | "lm-studio" | "lm_studio" if !model.is_empty() => {
            let mut p = ChatProvider::lm_studio(model);
            p.api_key = key;
            Some(p)
        }
        "openai-compatible" | "custom" if !model.is_empty() => {
            let base = v["base_url"].as_str()?;
            if !(base.starts_with("http://") || base.starts_with("https://")) {
                return None;
            }
            let name = v["name"].as_str().unwrap_or("Custom server");
            Some(ChatProvider::openai_compatible(name, base, model, key))
        }
        _ => None,
    }
}

/// Persist the choice, owner-only. The key (when set) is written here and
/// nowhere else; the file is created 0600 and chmodded 0600 on every save
/// in case it pre-existed with wider permissions.
pub fn save(path: &Path, p: &ChatProvider, ty: &str) -> Result<(), PawError> {
    let body = serde_json::json!({
        "type": ty,
        "model": p.model,
        "base_url": base_url_of(&p.url),
        "name": p.name.trim_end_matches(" (this computer)"),
        "api_key": p.api_key.as_deref(),
    });
    let text = serde_json::to_string_pretty(&body)
        .map_err(|e| PawError::Exec(format!("could not encode the provider settings: {e}")))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| PawError::Exec(format!("could not create {}: {e}", dir.display())))?;
    }
    // Write-then-rename is not needed for a two-line settings file, but
    // permissions are: create with 0600 from the start so the key is never
    // on disk world-readable, even between create and chmod.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts.open(path)
            .map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
        use std::io::Write;
        f.write_all(text.as_bytes())
            .map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
        drop(f);
        // In case the file pre-existed with wider permissions.
        let mut perms = std::fs::metadata(path)
            .map_err(|e| PawError::Exec(format!("could not stat {}: {e}", path.display())))?
            .permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)
            .map_err(|e| PawError::Exec(format!("could not make {} private: {e}", path.display())))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text)
            .map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Strip the well-known endpoint suffix to recover the server root.
fn base_url_of(url: &str) -> String {
    url.trim_end_matches("/v1/chat/completions")
        .trim_end_matches("/chat/completions")
        .to_string()
}

/// Human-readable summary for "chat provider": what is set, and how to change it.
pub fn status_text(p: &ChatProvider) -> String {
    let kind = if p.local { "on this computer" } else { "online service" };
    format!(
        "Chat answers come from: {} ({kind})\n  endpoint: {}\n  model: {}\nTo change it, say for example: \"use ollama model llama3.2\", \"use llama.cpp model qwen2.5\", \"use lm studio model mistral\" or \"use pollinations\". For another OpenAI-compatible server, edit {} (type \"openai-compatible\", base_url, model; api_key only if the server needs one - keys are never typed into the chat).",
        p.name,
        p.url,
        p.model,
        config_path().map(|p| p.display().to_string()).unwrap_or_else(|| "chat-provider.json".into())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_stay_pollinations() {
        let p = ChatProvider::pollinations();
        assert_eq!(p.url, "https://text.pollinations.ai/openai");
        assert_eq!(p.model, "openai");
        assert!(!p.local);
        assert!(p.headers().is_empty());
    }

    #[test]
    fn local_runners_are_loopback_and_marked_local() {
        for p in [
            ChatProvider::ollama("llama3.2"),
            ChatProvider::llama_cpp("qwen2.5"),
            ChatProvider::lm_studio("mistral"),
            ChatProvider::openai_compatible("vLLM", "http://localhost:8000", "m", None),
            ChatProvider::openai_compatible("tabby", "http://127.0.0.2:5000/v1", "m", None),
        ] {
            assert!(p.local, "{} should be local", p.url);
            assert!(p.url.ends_with("/v1/chat/completions"), "{}", p.url);
        }
    }

    #[test]
    fn remote_custom_endpoint_is_not_local() {
        let p = ChatProvider::openai_compatible("Hosted", "https://llm.example.com", "m", None);
        assert!(!p.local);
        assert_eq!(p.url, "https://llm.example.com/v1/chat/completions");
    }

    #[test]
    fn key_only_ever_appears_in_the_auth_header() {
        let secret = "sk-test-1234567890";
        let p = ChatProvider::openai_compatible("Hosted", "https://llm.example.com", "m", Some(secret.into()));
        let h = p.headers();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0], ("Authorization".to_string(), format!("Bearer {secret}")));
        // Redaction: neither Debug nor the human-facing texts may contain it.
        assert!(!format!("{p:?}").contains(secret), "Debug leaks the key");
        assert!(!p.destination_line().contains(secret));
        assert!(!p.unreachable_error().to_string().contains(secret));
        assert!(!status_text(&p).contains(secret));
        assert!(!format!("{:?}", p.spec()).contains(secret));
    }

    #[test]
    fn config_roundtrip_keeps_everything_but_shows_nothing_secret() {
        let p = ChatProvider::ollama("llama3.2");
        let dir = std::env::temp_dir().join(format!("pawline-prov-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat-provider.json");
        save(&path, &p, "ollama").unwrap();
        let back = load(&path);
        assert_eq!(back.spec(), p.spec());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn broken_or_missing_config_falls_back_to_default() {
        assert_eq!(parse_config("not json").unwrap_or_else(ChatProvider::pollinations).url,
                   ChatProvider::pollinations().url);
        assert_eq!(parse_config(r#"{"type":"ollama"}"#).is_none(), true, "model is required");
        let dir = std::env::temp_dir().join(format!("pawline-prov-missing-{}", std::process::id()));
        let p = load(&dir.join("nope.json"));
        assert_eq!(p.url, ChatProvider::pollinations().url);
    }

    #[test]
    fn destination_line_says_whether_text_leaves() {
        assert!(ChatProvider::ollama("m").destination_line().contains("does not leave the machine"));
        assert!(ChatProvider::pollinations().destination_line().contains("over the internet"));
    }
}
