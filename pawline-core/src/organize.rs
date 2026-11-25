//! "organize <folder>": group the loose files of one folder into type folders (Images, Documents, ...).
//! It only PLANS: a list of folder creations and moves that the user reviews on a confirmation card.
//! Nothing is deleted, nothing is overwritten (a file whose destination name is taken is left where it is and
//! reported), subfolders and hidden files are left alone, and files of unknown type stay where they are.
use crate::plan::Operation;
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 200;

pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("Images", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "tiff"]),
    ("Documents", &["pdf", "doc", "docx", "odt", "txt", "rtf", "md", "xls", "xlsx", "ods", "ppt", "pptx", "odp", "csv"]),
    ("Videos", &["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv"]),
    ("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus"]),
    ("Archives", &["zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst"]),
];

pub struct Organized {
    pub ops: Vec<Operation>,
    pub notes: Vec<String>,
}

fn category(name: &str) -> Option<&'static str> {
    let ext = Path::new(name).extension()?.to_string_lossy().to_lowercase();
    CATEGORIES.iter().find(|(_, e)| e.contains(&ext.as_str())).map(|(c, _)| *c)
}

/// `folder` must already be a canonical folder inside the home area (the caller resolves and confines it).
pub fn plan(folder: &Path) -> Result<Organized, String> {
    let rd = std::fs::read_dir(folder).map_err(|e| format!("can't read {}: {e}", folder.display()))?;
    let mut names: Vec<String> = vec![];
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_file() && !ft.is_symlink() && !name.starts_with('.') {
            names.push(name);
        }
    }
    names.sort();
    let mut notes = vec![];
    if names.len() > MAX_FILES {
        notes.push(format!("{} loose files here; only the first {MAX_FILES} are planned. Run it again for the rest.", names.len()));
        names.truncate(MAX_FILES);
    }
    let mut dirs: Vec<Operation> = vec![];
    let mut moves: Vec<Operation> = vec![];
    let mut blocked: Vec<&str> = vec![];
    let mut left = 0usize;
    for n in &names {
        let Some(cat) = category(n) else { left += 1; continue };
        let cdir = folder.join(cat);
        if cdir.exists() && !cdir.is_dir() {
            if !blocked.contains(&cat) {
                blocked.push(cat);
                notes.push(format!("{cat} exists but is not a folder, so those files stay where they are."));
            }
            continue;
        }
        let to: PathBuf = cdir.join(n);
        if to.exists() {
            notes.push(format!("{n} stays: {} already has a file with that name.", cat));
            continue;
        }
        if !cdir.exists() && !dirs.iter().any(|d| matches!(d, Operation::CreateDir { path } if *path == cdir)) {
            dirs.push(Operation::CreateDir { path: cdir.clone() });
        }
        moves.push(Operation::Move { from: folder.join(n), to });
    }
    if left > 0 {
        notes.push(format!("{left} file(s) of other types stay where they are."));
    }
    dirs.extend(moves);
    Ok(Organized { ops: dirs, notes })
}

/// "organize <place words>" / "sort out <place words>" (case-insensitive). The words are the place; empty is not a command.
pub fn parse(input: &str) -> Option<String> {
    let t = input.trim().trim_end_matches(['.', '!']);
    let low = t.to_lowercase();
    for p in ["organize my ", "organise my ", "organize the ", "organise the ", "organize ", "organise ", "sort out my ", "sort out the ", "sort out ", "tidy up my ", "tidy up "] {
        if low.starts_with(p) {
            let rest = t[p.len()..].trim();
            let rest = rest.strip_prefix("files in ").or_else(|| rest.strip_prefix("files on ")).or_else(|| rest.strip_prefix("files inside ")).unwrap_or(rest).trim();
            let rest = rest.strip_suffix(" folder").unwrap_or(rest).trim();
            return if rest.is_empty() || rest.eq_ignore_ascii_case("files") { None } else { Some(rest.to_string()) };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-org-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }
    #[test]
    fn plans_moves_only_and_never_overwrites() {
        let d = tmp("a");
        for f in ["a.JPG", "b.pdf", "c.mp4", "d.zip", "e.unknown", ".hidden.png", "noext"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/inner.png"), "x").unwrap();
        std::fs::create_dir_all(d.join("Documents")).unwrap();
        std::fs::write(d.join("Documents/b.pdf"), "other").unwrap();
        let o = plan(&d).unwrap();
        let text: Vec<String> = o.ops.iter().map(|p| p.describe().replace(&d.display().to_string(), "")).collect();
        assert!(text.contains(&"Create folder /Images".to_string()) && text.contains(&"Move /a.JPG to /Images/a.JPG".to_string()), "{text:?}");
        assert!(!text.iter().any(|t| t.contains("b.pdf") || t.contains("inner") || t.contains("hidden") || t.contains("e.unknown") || t.contains("Create folder /Documents")), "{text:?}");
        assert!(o.notes.iter().any(|n| n.contains("b.pdf stays")));
        assert!(o.ops.iter().all(|p| matches!(p, Operation::CreateDir { .. } | Operation::Move { .. })), "no delete, copy or overwrite");
        // folders first, then moves
        let first_move = o.ops.iter().position(|p| matches!(p, Operation::Move { .. })).unwrap();
        assert!(o.ops[..first_move].iter().all(|p| matches!(p, Operation::CreateDir { .. })));
        // planning touches nothing
        assert!(d.join("a.JPG").exists() && !d.join("Images").exists());
    }
    #[test]
    fn parse_forms() {
        assert_eq!(parse("organize my downloads"), Some("downloads".into()));
        assert_eq!(parse("Organize the files in ho co models."), Some("ho co models".into()));
        assert_eq!(parse("sort out documents folder"), Some("documents".into()));
        assert!(parse("organize").is_none() && parse("organize my files").is_none() && parse("organization chart").is_none());
    }
}
