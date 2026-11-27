//! Where the user's own model files live.
//!
//! Pawline never downloads a model. The two things it can use from disk are the
//! vision model behind screen questions (a SmolVLM GGUF plus its mmproj
//! projector) and the speech-to-text model (a whisper.cpp ggml file). Both are
//! set by the user - in Settings, or with "use vision model <path>" and "use
//! speech model <path>" - and stored in one small private file. Environment
//! variables still win for a one-off run.
//!
//! Everything here is pure filesystem inspection: no network, no model load.

use std::path::{Path, PathBuf};

use crate::error::PawError;

/// The three files Pawline can be pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The vision model itself (SmolVLM-256M-Instruct-Q8_0.gguf).
    Vision,
    /// The vision projector that goes with it (mmproj-...gguf).
    Mmproj,
    /// The speech-to-text model (ggml-base.en.bin and friends).
    Speech,
}

impl Kind {
    pub fn env(self) -> &'static str {
        match self {
            Kind::Vision => "PAWLINE_VISION_MODEL",
            Kind::Mmproj => "PAWLINE_VISION_MMPROJ",
            Kind::Speech => "PAWLINE_SPEECH_MODEL",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Vision => "vision model",
            Kind::Mmproj => "vision projector (mmproj)",
            Kind::Speech => "speech model",
        }
    }

    /// The setting line the user types, e.g. "use speech model".
    pub fn command(self) -> &'static str {
        match self {
            Kind::Vision => "use vision model",
            Kind::Mmproj => "use vision mmproj",
            Kind::Speech => "use speech model",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelPaths {
    pub vision: Option<PathBuf>,
    pub mmproj: Option<PathBuf>,
    pub speech: Option<PathBuf>,
}

impl ModelPaths {
    pub fn get(&self, k: Kind) -> Option<&PathBuf> {
        match k {
            Kind::Vision => self.vision.as_ref(),
            Kind::Mmproj => self.mmproj.as_ref(),
            Kind::Speech => self.speech.as_ref(),
        }
    }

    pub fn set(&mut self, k: Kind, p: Option<PathBuf>) {
        match k {
            Kind::Vision => self.vision = p,
            Kind::Mmproj => self.mmproj = p,
            Kind::Speech => self.speech = p,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.vision.is_none() && self.mmproj.is_none() && self.speech.is_none()
    }
}

/// Where the settings file lives: next to the chat provider settings, so one
/// Pawline data folder holds all of it. `<data>/model-paths.json`.
pub fn store_path() -> Option<PathBuf> {
    crate::providers::config_path().map(|p| p.with_file_name("model-paths.json"))
}

/// `~/x` and a bare `~` expand against the user's home. Nothing else is touched:
/// a path is taken as typed, spaces and all.
pub fn expand_tilde(raw: &str, home: Option<&Path>) -> PathBuf {
    let t = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if t == "~" {
        return home.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(t));
    }
    if let Some(rest) = t.strip_prefix("~/") {
        if let Some(h) = home {
            return h.join(rest);
        }
    }
    PathBuf::from(t)
}

/// Human size for the reply ("175 MB").
pub fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else {
        format!("{} KB", (bytes / 1024).max(1))
    }
}

/// Check one file the user pointed at and return the path to store.
///
/// A wrong path is refused here, before anything is saved, so the setting can
/// never hold a file that is not there. The message says what to do instead.
pub fn check(k: Kind, raw: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let p = expand_tilde(raw, home);
    if p.as_os_str().is_empty() {
        return Err(format!("Give me the full path to the {} file, for example ~/.local/share/pawline/models/SmolVLM-256M-Instruct-Q8_0.gguf. Nothing changed.", k.label()));
    }
    let meta = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("There is no file at {}. Nothing changed.", p.display()));
        }
        Err(e) => return Err(format!("I cannot read {}: {e}. Nothing changed.", p.display())),
    };
    if !meta.is_file() {
        return Err(format!("{} is not a file (it may be a folder). Point me at the model file itself. Nothing changed.", p.display()));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let is_gguf = name.ends_with(".gguf");
    let is_bin = name.ends_with(".bin") || name.ends_with(".onnx");
    match k {
        Kind::Vision => {
            if name.contains("mmproj") {
                return Err(format!(
                    "{} is the projector file, not the model. Say \"{}\" for that one, or point me at the SmolVLM .gguf itself. Nothing changed.",
                    p.display(),
                    Kind::Mmproj.command()
                ));
            }
            if !is_gguf {
                return Err(format!("{} does not look like a llama.cpp model (the name does not end in .gguf). Nothing changed.", p.display()));
            }
        }
        Kind::Mmproj => {
            if !is_gguf {
                return Err(format!("{} does not look like a llama.cpp projector (the name does not end in .gguf). Nothing changed.", p.display()));
            }
        }
        Kind::Speech => {
            if !(is_bin || is_gguf) {
                return Err(format!("{} does not look like a whisper.cpp model (the name should end in .bin, or .gguf). Nothing changed.", p.display()));
            }
        }
    }
    if meta.len() < 1024 * 1024 {
        return Err(format!(
            "{} is only {}, which is too small to be a model (a download that stopped early looks like this). Nothing changed.",
            p.display(),
            size_text(meta.len())
        ));
    }
    Ok(p)
}

/// The configured file for `k`, with the environment override ahead of it.
/// `None` means "not set" - the caller decides what to do about that.
pub fn configured(k: Kind, stored: &ModelPaths, env: &dyn Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(raw) = env(k.env()) {
        let raw = raw.trim().to_string();
        if !raw.is_empty() {
            let p = expand_tilde(&raw, home);
            if p.is_file() {
                return Some(p);
            }
            // A set-but-missing override must not silently fall through to an
            // older stored file: the run should say what it tried.
            return Some(p);
        }
    }
    stored.get(k).cloned()
}

/// First file in `dir` whose name matches, sorted for a stable answer.
fn pick_in(dir: &Path, want: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    v.sort();
    v.into_iter().find(|p| p.file_name().map(|n| want(&n.to_string_lossy().to_lowercase())).unwrap_or(false))
}

/// Folders Pawline looks in when nothing is set: its own data folder first,
/// then the obvious places a person keeps a model they downloaded.
pub fn search_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/share/pawline/models"),
        home.join("models"),
        home.join("Downloads"),
        home.join("Downloads/vlm"),
        home.join("Downloads/models"),
    ]
}

/// Best guess from what is already on disk. Never sets anything: the result is
/// only offered to the user ("found this - use it?").
pub fn discover(home: &Path) -> ModelPaths {
    let mut m = ModelPaths::default();
    let dirs = search_dirs(home);
    let vision = |n: &str| n.ends_with(".gguf") && !n.contains("mmproj") && (n.contains("smolvlm") || n.contains("smollm"));
    let mmproj = |n: &str| n.ends_with(".gguf") && n.contains("mmproj");
    let speech = |n: &str| (n.ends_with(".bin") || n.ends_with(".gguf")) && (n.starts_with("ggml-") || n.contains("whisper"));
    for d in &dirs {
        if m.vision.is_none() {
            m.vision = pick_in(d, &vision);
        }
        if m.mmproj.is_none() {
            m.mmproj = pick_in(d, &mmproj);
        }
        if m.speech.is_none() {
            m.speech = pick_in(d, &speech);
        }
    }
    m
}

pub fn load(path: &Path) -> ModelPaths {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return ModelPaths::default(),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return ModelPaths::default(),
    };
    let get = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from);
    ModelPaths { vision: get("vision"), mmproj: get("mmproj"), speech: get("speech") }
}

/// Write the settings file private (0600), like the provider settings.
pub fn save(path: &Path, m: &ModelPaths) -> Result<(), PawError> {
    let mut obj = serde_json::Map::new();
    for (k, p) in [("vision", &m.vision), ("mmproj", &m.mmproj), ("speech", &m.speech)] {
        if let Some(p) = p {
            obj.insert(k.to_string(), serde_json::json!(p.to_string_lossy()));
        }
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(obj))
        .map_err(|e| PawError::Exec(format!("could not encode the model settings: {e}")))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| PawError::Exec(format!("could not create {}: {e}", dir.display())))?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
        f.write_all(text.as_bytes()).map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
        drop(f);
        let mut perms = std::fs::metadata(path).map_err(|e| PawError::Exec(format!("could not stat {}: {e}", path.display())))?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms).map_err(|e| PawError::Exec(format!("could not make {} private: {e}", path.display())))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text).map_err(|e| PawError::Exec(format!("could not write {}: {e}", path.display())))?;
    }
    Ok(())
}


/// Prefixes a settings line can start with. Kept here so the chat parser and the
/// Settings screen can never disagree about what the words mean.
const PREFIXES: &[(&str, Kind)] = &[
    ("use vision model", Kind::Vision),
    ("use smolvlm model", Kind::Vision),
    ("use smollm model", Kind::Vision),
    ("point vision model at", Kind::Vision),
    ("use vision mmproj", Kind::Mmproj),
    ("use vision projector", Kind::Mmproj),
    ("use speech model", Kind::Speech),
    ("use whisper model", Kind::Speech),
    ("use stt model", Kind::Speech),
    ("point speech model at", Kind::Speech),
];

fn is_clear(arg: &str) -> bool {
    matches!(arg.trim().to_lowercase().as_str(), "" | "off" | "auto" | "reset" | "clear" | "none" | "unset" | "default")
}

/// Parse "use vision model <path>" and friends. The path keeps the user's own
/// casing, so this reads the raw input rather than a lowercased copy.
/// `Some((kind, None))` means "back to automatic".
pub fn parse_setting(input: &str) -> Option<(Kind, Option<String>)> {
    let t = input.trim().trim_end_matches(['.', '!']).trim();
    let lower = t.to_lowercase();
    for (p, k) in PREFIXES {
        let Some(rest) = lower.strip_prefix(p) else { continue };
        if !(rest.is_empty() || rest.starts_with(' ')) {
            continue;
        }
        let arg = t.get(p.len()..).unwrap_or("").trim().to_string();
        if is_clear(&arg) {
            return Some((*k, None));
        }
        return Some((*k, Some(arg)));
    }
    None
}

/// The read-only question: "model files", "which models", "model paths".
pub fn is_status_query(input: &str) -> bool {
    let t = input.trim().trim_end_matches(['.', '?', '!']).trim().to_lowercase();
    matches!(t.as_str(), "model files" | "model paths" | "which models" | "models" | "my models" | "model settings")
}

/// One line per file, for the chat answer and the Settings notes.
pub fn status_text(stored: &ModelPaths, home: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    for k in [Kind::Vision, Kind::Mmproj, Kind::Speech] {
        let line = match configured(k, stored, env, home) {
            Some(p) => match std::fs::metadata(&p) {
                Ok(m) if m.is_file() => format!("{}: {} ({})", k.label(), p.display(), size_text(m.len())),
                _ => format!("{}: {} - MISSING (the file is not there any more)", k.label(), p.display()),
            },
            None => {
                let found = home.map(discover).and_then(|d| d.get(k).cloned());
                match found {
                    Some(p) => format!("{}: not set - found {} (say \"{}\" to use it)", k.label(), p.display(), k.command()),
                    None => format!("{}: not set - none found on this computer", k.label()),
                }
            }
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-models-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn big(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, vec![0u8; 1024 * 1024 + 1]).unwrap();
        p
    }

    #[test]
    fn a_real_file_is_accepted_and_a_missing_one_is_not() {
        let h = home();
        let d = h.join("m");
        std::fs::create_dir_all(&d).unwrap();
        let gguf = big(&d, "SmolVLM-256M-Instruct-Q8_0.gguf");
        assert_eq!(check(Kind::Vision, gguf.to_str().unwrap(), Some(&h)).unwrap(), gguf);
        let err = check(Kind::Vision, d.join("nope.gguf").to_str().unwrap(), Some(&h)).unwrap_err();
        assert!(err.contains("no file at") && err.contains("Nothing changed"), "{err}");
        let dir_err = check(Kind::Vision, d.to_str().unwrap(), Some(&h)).unwrap_err();
        assert!(dir_err.contains("not a file"), "{dir_err}");
    }

    #[test]
    fn the_projector_and_the_model_are_not_interchangeable() {
        let h = home();
        let d = h.join("m");
        std::fs::create_dir_all(&d).unwrap();
        let mm = big(&d, "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf");
        let err = check(Kind::Vision, mm.to_str().unwrap(), Some(&h)).unwrap_err();
        assert!(err.contains("projector file, not the model") && err.contains("use vision mmproj"), "{err}");
        assert_eq!(check(Kind::Mmproj, mm.to_str().unwrap(), Some(&h)).unwrap(), mm);
        let gguf = big(&d, "SmolVLM-256M-Instruct-Q8_0.gguf");
        assert!(check(Kind::Mmproj, gguf.to_str().unwrap(), Some(&h)).is_ok(), "a .gguf is a legal projector path; the name is a hint, not a gate");
    }

    #[test]
    fn a_truncated_download_and_a_wrong_extension_are_refused_with_the_reason() {
        let h = home();
        let d = h.join("m");
        std::fs::create_dir_all(&d).unwrap();
        let tiny = d.join("ggml-base.en.bin");
        std::fs::write(&tiny, b"partial").unwrap();
        let e = check(Kind::Speech, tiny.to_str().unwrap(), Some(&h)).unwrap_err();
        assert!(e.contains("too small") && e.contains("Nothing changed"), "{e}");
        let txt = big(&d, "notes.txt");
        let e2 = check(Kind::Speech, txt.to_str().unwrap(), Some(&h)).unwrap_err();
        assert!(e2.contains(".bin"), "{e2}");
        let wav = big(&d, "ggml-tiny.en.bin");
        assert!(check(Kind::Speech, wav.to_str().unwrap(), Some(&h)).is_ok());
    }

    #[test]
    fn tilde_expands_against_home_and_quotes_are_dropped() {
        let h = home();
        let d = h.join("models");
        std::fs::create_dir_all(&d).unwrap();
        let p = big(&d, "ggml-small.en.bin");
        assert_eq!(check(Kind::Speech, "~/models/ggml-small.en.bin", Some(&h)).unwrap(), p);
        assert_eq!(check(Kind::Speech, "\"~/models/ggml-small.en.bin\"", Some(&h)).unwrap(), p);
        assert_eq!(expand_tilde("~", Some(&h)), h);
        assert_eq!(expand_tilde("~/models/x.bin", None), PathBuf::from("~/models/x.bin"));
    }

    #[test]
    fn settings_round_trip_stay_private_and_a_broken_file_falls_back() {
        let h = home();
        let f = h.join("model-paths.json");
        assert_eq!(load(&f), ModelPaths::default());
        let m = ModelPaths { vision: Some(PathBuf::from("/a/v.gguf")), mmproj: None, speech: Some(PathBuf::from("/b/ggml-base.en.bin")) };
        save(&f, &m).unwrap();
        assert_eq!(load(&f), m);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600, "the settings file is private");
        }
        std::fs::write(&f, "not json").unwrap();
        assert_eq!(load(&f), ModelPaths::default(), "a broken file never blocks the app");
        std::fs::write(&f, r#"{"vision":"  ","speech":"/x.bin"}"#).unwrap();
        assert_eq!(load(&f).speech, Some(PathBuf::from("/x.bin")));
        assert_eq!(load(&f).vision, None, "a blank value is not a path");
    }

    #[test]
    fn the_environment_wins_and_a_set_but_missing_override_does_not_fall_through() {
        let h = home();
        let d = h.join("m");
        std::fs::create_dir_all(&d).unwrap();
        let stored = big(&d, "ggml-base.en.bin");
        let envp = big(&d, "ggml-small.en.bin");
        let stored_m = ModelPaths { speech: Some(stored.clone()), ..Default::default() };
        let env = |k: &str| if k == Kind::Speech.env() { Some(envp.to_string_lossy().to_string()) } else { None };
        assert_eq!(configured(Kind::Speech, &stored_m, &env, Some(&h)), Some(envp));
        let missing = |_: &str| Some("/nowhere/ggml.bin".to_string());
        assert_eq!(configured(Kind::Speech, &stored_m, &missing, Some(&h)), Some(PathBuf::from("/nowhere/ggml.bin")), "the override is reported, not silently swapped");
        let none = |_: &str| None;
        assert_eq!(configured(Kind::Speech, &stored_m, &none, Some(&h)), Some(stored));
        assert_eq!(configured(Kind::Vision, &stored_m, &none, Some(&h)), None);
    }

    #[test]
    fn discovery_finds_the_files_a_user_already_has_and_offers_nothing_else() {
        let h = home();
        let d = h.join("Downloads");
        std::fs::create_dir_all(&d).unwrap();
        assert!(discover(&h).is_empty(), "an empty machine discovers nothing");
        let v = big(&d, "SmolVLM-256M-Instruct-Q8_0.gguf");
        let m = big(&d, "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf");
        let s = big(&d, "ggml-base.en.bin");
        big(&d, "holiday-photo.jpg");
        let found = discover(&h);
        assert_eq!(found.vision, Some(v));
        assert_eq!(found.mmproj, Some(m));
        assert_eq!(found.speech, Some(s));
    }

    #[test]
    fn the_setting_line_reads_the_way_people_type_it() {
        assert_eq!(parse_setting("use vision model ~/m/SmolVLM-256M-Instruct-Q8_0.gguf"), Some((Kind::Vision, Some("~/m/SmolVLM-256M-Instruct-Q8_0.gguf".into()))));
        // casing in the path survives even though the words are matched loosely
        assert_eq!(parse_setting("USE Vision Model /data/SmolVLM-Q8_0.GGUF"), Some((Kind::Vision, Some("/data/SmolVLM-Q8_0.GGUF".into()))));
        assert_eq!(parse_setting("use whisper model /m/ggml-base.en.bin."), Some((Kind::Speech, Some("/m/ggml-base.en.bin".into()))));
        assert_eq!(parse_setting("use speech model off"), Some((Kind::Speech, None)));
        assert_eq!(parse_setting("use vision mmproj /m/mmproj.gguf"), Some((Kind::Mmproj, Some("/m/mmproj.gguf".into()))));
        assert_eq!(parse_setting("use vision model"), Some((Kind::Vision, None)), "no path means back to automatic");
        assert_eq!(parse_setting("use vision modeling stuff"), None, "a prefix must end at a word boundary");
        assert_eq!(parse_setting("make a folder called models"), None);
    }

    #[test]
    fn the_status_answer_says_what_is_set_missing_or_found() {
        let h = home();
        let d = h.join("models");
        std::fs::create_dir_all(&d).unwrap();
        let s = big(&d, "ggml-base.en.bin");
        let stored = ModelPaths { speech: Some(s.clone()), vision: Some(PathBuf::from("/gone/v.gguf")), ..Default::default() };
        let none = |_: &str| None;
        let t = status_text(&stored, Some(&h), &none);
        assert!(t.contains("speech model: ") && t.contains("ggml-base.en.bin"), "{t}");
        assert!(t.contains("MISSING"), "{t}");
        assert!(t.contains("not set"), "{t}");
        assert!(is_status_query("model files?") && is_status_query("Which Models") && !is_status_query("model file please"));
    }

    #[test]
    fn sizes_read_like_a_person_wrote_them() {
        assert_eq!(size_text(175 * 1024 * 1024), "175 MB");
        assert_eq!(size_text(900), "1 KB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
