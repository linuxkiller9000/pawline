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
    Create { kind: CreateKind, names: Vec<String>, parent: LocationRef },
    Delete { target: LocationRef },
    Rename { target: LocationRef, new_name: String },
    Move { target: LocationRef, dest: LocationRef },
    Copy { target: LocationRef, dest: LocationRef },
    List { target: LocationRef },
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
    List { path: PathBuf },
    /// `argv` is the exact reviewed command (resolved at planning time); never run through a shell.
    OpenApp { name: String, argv: Vec<String> },
    SetWallpaper { image: PathBuf, via: Option<crate::wallpaper::Adapter>, no_backup_ok: bool },
    /// Undo journal entry `entry`; `summary` is what it will do.
    Undo { entry: u64, summary: String },
    Answer { text: String },
    Search { engine: SearchEngine, query: String },
    GenerateImage { provider: crate::imagegen::ImageProvider, prompt: String },
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
            Operation::List { path } => format!("List {}", path.display()),
            Operation::OpenApp { name, argv } => format!("Open {name}: run {}", argv.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")),
            Operation::Undo { summary, .. } => summary.clone(),
            Operation::SetWallpaper { image, via, .. } => match via {
                Some(a) => format!("Set wallpaper to {} (using {})", image.display(), a.program()),
                None => format!("Set wallpaper to {}", image.display()),
            },
            Operation::Answer { text } => text.clone(),
            Operation::GenerateImage { provider, prompt } => format!(
                "Make a picture of \"{prompt}\" with {}: this {}. Nothing else is sent. It is saved in ~/Pictures/Pawline.",
                provider.label(),
                provider.disclosure()
            ),
            Operation::Search { engine, query } => match engine {
                SearchEngine::Web => format!("Search the web for \"{query}\""),
                SearchEngine::Wiki => format!("Search Wikipedia for \"{query}\""),
            },
        }
    }

    /// Destructive or hard-to-undo operations need explicit user confirmation.
    pub fn needs_confirmation(&self) -> bool {
        matches!(
            self,
            Operation::Delete { .. } | Operation::Move { .. } | Operation::SetWallpaper { .. }
                | Operation::Undo { .. } | Operation::OpenApp { .. } | Operation::GenerateImage { .. }
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
