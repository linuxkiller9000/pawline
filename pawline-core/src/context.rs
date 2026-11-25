use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Conversation memory for the deterministic engine. Tracks what was created
/// or touched recently so follow-ups like "inside it" or "inside pics" resolve.
#[derive(Default, Clone)]
pub struct ContextManager {
    /// Bare name, exactly as spelled -> absolute path, for everything touched this session.
    known: HashMap<String, PathBuf>,
    last_created: Option<PathBuf>,
    last_dir: Option<PathBuf>,
}

impl ContextManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn remember(&mut self, path: &Path) {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            self.known.insert(name.to_string(), path.to_path_buf());
        }
        if path.is_dir() || path.extension().is_none() {
            self.last_dir = Some(path.to_path_buf());
        }
        self.last_created = Some(path.to_path_buf());
    }

    pub fn note_dir(&mut self, path: &Path) {
        self.last_dir = Some(path.to_path_buf());
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            self.known.insert(name.to_string(), path.to_path_buf());
        }
    }

    pub fn last_created(&self) -> Option<&Path> {
        self.last_created.as_deref()
    }

    pub fn current_dir(&self) -> Option<&Path> {
        self.last_dir.as_deref()
    }

    /// Resolve a bare name like "pics": first as a child of the current
    /// context directory, then anywhere in the session's known set.
    pub fn resolve_named(&self, name: &str) -> Option<PathBuf> {
        let key = name.to_string();
        if let Some(dir) = &self.last_dir {
            let cand = dir.join(name);
            if self.known.get(&key).map(|p| p == &cand).unwrap_or(false) || cand.exists() {
                return Some(cand);
            }
        }
        self.known.get(&key).cloned()
    }
}
