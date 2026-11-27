use std::path::PathBuf;

/// What kind of thing a create-step makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateKind {
    Dir,
    /// A file; the extension hint comes from words like "markdown" or "text"
    /// and is applied only when the name has no extension of its own.
    File { ext_hint: Option<String> },
}

/// Which online source an explicit search uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEngine {
    /// DuckDuckGo instant answers.
    Web,
    /// Wikipedia article summary.
    Wiki,
}

/// One parsed directive before paths are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    Create { kind: CreateKind, names: Vec<String>, parent: LocationRef, parents: bool },
    Delete { target: LocationRef },
    Rename { target: LocationRef, new_name: String },
    Move { target: LocationRef, dest: LocationRef },
    Copy { target: LocationRef, dest: LocationRef },
    List { target: LocationRef, page: usize, contains: Option<String> },
    OpenApp { name: String },
    /// Undo the most recent journaled change.
    Undo,
    /// Set the desktop wallpaper to an image the user pointed at.
    SetWallpaper { image: LocationRef, via: Option<String>, no_backup_ok: bool },
    /// A side-effect-free computed answer (deterministic math). Never
    /// touches disk, never needs confirmation.
    Answer { text: String },
    /// An explicit online search the user asked for by name. Sends the
    /// query to the named source and shows the result; no local effects.
    Search { engine: SearchEngine, query: String },
    /// Weather for a named place from Open-Meteo. Sends the place name; asks first.
    Weather { place: String },
    /// A free online picture from a provider the user named. Sends only the prompt.
    GenerateImage { provider: crate::imagegen::ImageProvider, prompt: String },
}

/// A place the user pointed at, before resolution against the session context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationRef {
    Home,
    Desktop,
    Documents,
    Downloads,
    /// An explicit path the user typed (absolute, or ~-relative).
    Explicit(String),
    /// "it", "that folder": the thing we just worked with.
    LastCreated,
    /// A bare name, e.g. "pics": resolved against session context.
    Named(String),
    /// No location said: the current context directory.
    CurrentDir,
}

/// A fully-resolved, ordered, validated-ready operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    CreateDir { path: PathBuf },
    CreateFile { path: PathBuf },
    Delete { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
    List { path: PathBuf, page: usize, contains: Option<String> },
    /// Make a smaller copy of one video with ffmpeg at `to`. The original is never touched.
    Shrink { from: PathBuf, to: PathBuf },
    /// Build a .pptx presentation at `to` from the outline file `from`. Local only; the outline is not changed.
    Deck { from: PathBuf, to: PathBuf },
    /// Online: ask the model for an outline from a topic. Shown for review; writes nothing.
    DeckDraft { topic: String },
    /// Save the outline the user reviewed (held in memory) as a new .pptx.
    DeckText { text: String, to: PathBuf },
    /// `argv` is the exact reviewed command (resolved at planning time); never run through a shell.
    OpenApp { name: String, argv: Vec<String> },
    SetWallpaper { image: PathBuf, via: Option<crate::wallpaper::Adapter>, no_backup_ok: bool },
    /// Undo journal entry `entry`; `summary` is what it will do.
    Undo { entry: u64, summary: String },
    Answer { text: String },
    Search { engine: SearchEngine, query: String },
    Weather { place: String },
    GenerateImage { provider: crate::imagegen::ImageProvider, prompt: String },
    /// Send one question to Pollinations text. Shown only, never run. Always confirmed when it comes from here.
    AskOnline { text: String },
    /// One online chat turn: `context` = earlier lines of THIS session (alternating you / assistant, oldest first), `text` = the new message.
    ChatOnline { provider: crate::providers::ProviderSpec, context: Vec<String>, text: String },
    LookAtImage { provider: crate::providers::ProviderSpec, path: String, prompt: String },
    /// One "what's on my screen" turn: capture the screen, then answer from
    /// OCR text or pixels depending on the question. Always confirmed.
    /// `share` is what the user chose to share: one window, a region they
    /// drew, or the whole screen. It is decided before the card is shown.
    ScreenQA { provider: crate::providers::ProviderSpec, question: String, share: crate::screencap::ShareTarget },
}

impl Operation {
    pub fn describe(&self) -> String {
        match self {
            Operation::CreateDir { path } => format!("Create folder {}", path.display()),
            Operation::CreateFile { path } => format!("Create file {}", path.display()),
            Operation::Delete { path } => format!("Delete {}", path.display()),
            Operation::Rename { from, to } => {
                format!("Rename {} to {}", from.display(), to.display())
            }
            Operation::Move { from, to } => format!("Move {} to {}", from.display(), to.display()),
            Operation::Copy { from, to } => format!("Copy {} to {}", from.display(), to.display()),
            Operation::List { path, .. } => format!("List {}", path.display()),
            Operation::DeckDraft { topic } => format!(
                "Write a presentation outline about \"{topic}\" with the free online model (Pollinations \"Classic\": no account, best effort, about one request per 45 seconds per IP address). Pawline sends exactly this request text in the web address it asks text.pollinations.ai for, plus the usual web details (your IP address, a \"Pawline\" user-agent): \"{}\" Nothing from your files, chat history or location is sent. Pollinations' privacy policy (updated 2026-11-17) says answers can be cached, typically up to 30 days, so the same address can return a saved answer; what it keeps is its own policy. Do not put passwords, secrets or private information in the topic. The reply is only shown to you as a draft, nothing is saved, and it is AI-written text that may contain mistakes: check every fact before you use it.",
                crate::deck::author_prompt(topic)
            ),
            Operation::DeckText { to, .. } => format!("Save the reviewed presentation draft as {}. Nothing is sent anywhere. The slides are exactly the draft you were shown.", to.display()),
            Operation::Deck { from, to } => format!("Make a presentation {} from the outline {}. Nothing is sent anywhere. The outline is not changed.", to.display(), from.display()),
            Operation::Shrink { from, to } => format!("Shrink video {} into a NEW smaller copy {} (ffmpeg, H.264 quality 28, AAC 128k). This is a lossy re-encode: picture and sound quality can drop. {} The original is not changed.", from.display(), to.display(), crate::shrink::KEEPS),
            Operation::OpenApp { name, argv } => format!("Open {name}: run {}", argv.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")),
            Operation::Undo { summary, .. } => summary.clone(),
            Operation::SetWallpaper { image, via, .. } => match via {
                Some(a) => format!("Set wallpaper to {} (using {})", image.display(), a.program()),
                None => format!("Set wallpaper to {}", image.display()),
            },
            Operation::Answer { text } => text.clone(),
            Operation::GenerateImage { provider, prompt } => format!(
                "Make a picture of \"{prompt}\" with {}: this {}. Pawline sends only that prompt, plus the usual web details (IP address, a \"Pawline\" user-agent). It is saved in ~/Pictures/Pawline.",
                provider.label(),
                provider.disclosure()
            ),
            Operation::AskOnline { text } => format!(
                "Ask Pollinations (online) \"{text}\": this is the free \"Classic\" service: no account, best effort, about one request per 45 seconds per IP address. Pawline puts your question text in the web address it requests from text.pollinations.ai, plus the usual web details (your IP address, a \"Pawline\" user-agent). Its privacy policy (updated 2026-11-17) says answers can be cached, typically up to 30 days, so the same address can return a saved answer. Pawline does not send your files, chat history or location. What Pollinations keeps is its own policy. Do not put passwords, secrets or private information in this question. The answer is only shown, never run, and may be wrong."
            ),
            Operation::ScreenQA { provider, question, share } => {
                let destination = if provider.local {
                    format!("{} on this computer (loopback only): the capture never leaves the machine", provider.url)
                } else {
                    format!("{} ({}) over the internet: the whole screen is visible to that service", provider.url, provider.name)
                };
                format!(
                    "Answer \"{}\" about YOUR SCREEN with {} (model \"{}\"). {} Text questions are answered from OCR text read off the capture; visual questions send the capture itself (shrunk to 1280px wide). The request goes to {}. The capture file is deleted right after the answer. The answer is only shown, never run, and may be wrong.",
                    question, provider.name, provider.model, share.describe(), destination
                )
            }
            Operation::LookAtImage { provider, path, prompt } => {
                let destination = if provider.local {
                    format!(
                        "The request goes to {} on this computer (loopback only): the image and question do not leave the machine. The runner is a separate program; its own settings are not Pawline's.",
                        provider.url
                    )
                } else {
                    format!(
                        "The request goes over the internet to {} ({}): the image content and question text are visible to that service, along with your IP address.",
                        provider.url, provider.name
                    )
                };
                format!(
                    "Look at an image with {} (model \"{}\"). Pawline reads the file {} from disk, checks it really is a PNG, JPEG, GIF or WebP image ({} MB limit), and POSTs one JSON body to {}: the image (base64 inside the body), your question \"{}\", and the system line Pawline always shows. No chat history goes along. {} Never sent: other files, location or saved history. The answer is only shown, never run, and is AI-written text that may be wrong - small local vision models can describe an image incorrectly with confidence.",
                    provider.name,
                    provider.model,
                    path,
                    crate::textfallback::MAX_IMAGE_BYTES / (1024 * 1024),
                    provider.url,
                    prompt,
                    destination
                )
            }
            Operation::ChatOnline { provider, context, text } => {
                let mut lines = vec![format!("  system: {}", crate::textfallback::CHAT_SYSTEM)];
                for (i, c) in context.iter().enumerate() {
                    lines.push(format!("  {}: {c}", if i % 2 == 0 { "user" } else { "assistant (an earlier answer)" }));
                }
                lines.push(format!("  user: {text}"));
                let destination = if provider.local {
                    format!(
                        "The request goes to {} on this computer (loopback only): your text does not leave the machine. The runner is a separate program; its own downloads, updates or telemetry are its settings, not Pawline's.",
                        provider.url
                    )
                } else {
                    format!(
                        "The request also carries the usual web details (your IP address, a \"Pawline\" user-agent, no cookies, no account). Free \"Classic\" tier: best effort, one request per 45 seconds per IP address. What Pollinations says in its privacy policy (updated 2026-11-17): prompts are processed transiently and not used to train models without opt-in; replies may be cached for up to about 30 days; free usage logs keep a keyed hash and network prefix of the IP address, the country and timestamps; some data may reach its service providers, and externally hosted community models follow their own providers' policies."
                    )
                };
                format!(
                    "Chat turn with {}. Pawline will POST one JSON body to {} with two fields: model = \"{}\", and messages = the lines below (their text is sent word for word; JSON adds only quotes and escapes). Nothing else is in the body. Earlier lines of this session sent along: {} (memory only, cut to {} characters each).\n{}\n{}\nNever sent: your files, location or saved history. Do not put passwords or private information in the chat. The answer is only shown, never run, and is AI-written text that may be wrong. Say \"chat off\" or \"forget our chat\" to end it.",
                    provider.name,
                    provider.url,
                    provider.model,
                    context.len(),
                    crate::textfallback::CHAT_LINE_CHARS,
                    lines.join("\n"),
                    destination
                )
            }
            Operation::Search { engine, query } => match engine {
                SearchEngine::Web => format!(
                    "Search the web for \"{query}\" (online, free, no account): Pawline sends your search words to api.duckduckgo.com (its Instant Answer service, not full web search), plus the usual web details: your IP address and a \"Pawline\" user-agent. Pawline does not send your files, chat history or location. What DuckDuckGo keeps is its own policy. The result is only shown, never run, and may be empty or out of date."
                ),
                SearchEngine::Wiki => format!(
                    "Search Wikipedia for \"{query}\" (online, free, no account): Pawline sends your search words to en.wikipedia.org, plus the usual web details: your IP address and a \"Pawline\" user-agent. Pawline does not send your files, chat history or location. What Wikipedia keeps is its own policy. The result is only shown, never run."
                ),
            },
            Operation::Weather { place } => crate::weather::disclosure(place),
        }
    }

    /// Destructive or hard-to-undo operations need explicit user confirmation.
    pub fn needs_confirmation(&self) -> bool {
        matches!(
            self,
            Operation::Delete { .. } | Operation::Move { .. } | Operation::Shrink { .. } | Operation::Deck { .. } | Operation::DeckDraft { .. } | Operation::DeckText { .. } | Operation::SetWallpaper { .. }
                | Operation::Undo { .. } | Operation::OpenApp { .. } | Operation::GenerateImage { .. } | Operation::AskOnline { .. } | Operation::ChatOnline { .. } | Operation::Search { .. } | Operation::Weather { .. }
        )
    }
}

/// The checked plan handed to the UI and the execution layer.
#[derive(Debug)]
pub struct Plan {
    pub ops: Vec<Operation>,
    pub needs_confirmation: bool,
    pub confirmation_reasons: Vec<String>,
    /// True when ANY directive in this plan came from the model assist
    /// rather than the deterministic parser. Model-proposed plans always
    /// require explicit user confirmation before execution — model
    /// confidence is not evidence the user asked for the operation.
    pub assist_derived: bool,
    /// Plain-language limits the user should see before confirming.
    pub notes: Vec<String>,
}

impl Plan {
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for (i, op) in self.ops.iter().enumerate() {
            s.push_str(&format!("{}. {}\n", i + 1, op.describe()));
        }
        for n in &self.notes {
            s.push_str(&format!("Note: {n}\n"));
        }
        s.trim_end().to_string()
    }
}
