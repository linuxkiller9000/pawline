//! Execution layer. Structured native operations only: no shell, ever.
//!
//! Needle note: when the embedded assist proposes tool calls, those proposals
//! are converted into the same `Operation`s and executed through the same
//! backend after the same validation. The model never touches the disk itself.
//!
//! TOCTOU: DirectFsBackend resolves every path beneath a directory handle held
//! open on the allowed root (cap-std, openat2 RESOLVE_BENEATH on Linux).
//! Validation is UX-level checking; confinement is enforced at syscall time,
//! so a parent swapped for a symlink after validation cannot redirect a write
//! outside the root.

use std::path::{Path, PathBuf};

use cap_std::fs::Dir;

use crate::error::PawError;
use crate::plan::Operation;

#[derive(Debug)]
pub struct OpOutcome {
    pub op: Operation,
    pub detail: String,
}

pub trait ExecutionBackend {
    /// Run an explicit online search. The fs backend has no network; UI
    /// layers override this. Default: refused, never silently skipped.
    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError>;
    fn name(&self) -> &'static str;
    fn search(&self, _engine: crate::plan::SearchEngine, _query: &str) -> Result<String, PawError> {
        Err(PawError::Exec(
            "online search is not available in this backend".into(),
        ))
    }
}

/// Real filesystem backend, confined to a root directory handle.
pub struct DirectFsBackend {
    root_path: PathBuf,
    root: Dir,
    wallpaper: Box<dyn crate::wallpaper::WallpaperSetter>,
}

impl DirectFsBackend {
    pub fn new(root: &Path) -> Result<Self, PawError> {
        let canon = std::fs::canonicalize(root)?;
        let dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority())?;
        Ok(DirectFsBackend {
            root_path: canon,
            root: dir,
            wallpaper: Box::new(crate::wallpaper::SystemWallpaper),
        })
    }

    /// Replace the wallpaper setter (tests, or a UI-specific one).
    pub fn with_wallpaper(mut self, w: Box<dyn crate::wallpaper::WallpaperSetter>) -> Self {
        self.wallpaper = w;
        self
    }

    fn read_journal(&self) -> String {
        use std::io::Read;
        let mut s = String::new();
        if let Ok(mut f) = self.root.open(crate::journal::JOURNAL) {
            let _ = f.read_to_string(&mut s);
        }
        s
    }

    fn append_journal(&self, line: &str) -> Result<(), PawError> {
        use std::io::Write;
        self.root.create_dir_all(crate::journal::STORE)?;
        let mut f = self.root.open_with(
            crate::journal::JOURNAL,
            cap_std::fs::OpenOptions::new().append(true).create(true),
        )?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    /// Record a change that already happened. A journal failure is reported,
    /// not swallowed: the change itself succeeded, but it cannot be undone.
    fn record(&self, kind: &str, a: &Path, b: Option<String>) -> Result<(), PawError> {
        let id = crate::journal::next_id(&self.read_journal());
        let e = crate::journal::Entry {
            id,
            kind: kind.into(),
            a: self.rel(a)?.to_string_lossy().to_string(),
            b,
        };
        self.append_journal(&e.to_line()).map_err(|err| {
            PawError::Exec(format!("{kind} done, but it could not be recorded for undo: {err}"))
        })
    }

    fn undo(&self, entry: u64) -> Result<String, PawError> {
        let text = self.read_journal();
        let e = crate::journal::last_open(&text)
            .ok_or_else(|| PawError::Validation("nothing to undo".into()))?;
        if e.id != entry {
            return Err(PawError::Validation(
                "the history changed since you were asked; ask to undo again".into(),
            ));
        }
        let a = PathBuf::from(&e.a);
        let b = e.b.as_ref().map(PathBuf::from);
        let gone = |p: &Path| !self.root.try_exists(p).unwrap_or(true);
        match e.kind.as_str() {
            "create_dir" => {
                self.root.remove_dir(&a).map_err(|_| {
                    PawError::Validation(format!("~/{} is missing or not empty; leaving it", e.a))
                })?;
            }
            "create_file" => {
                let m = self.root.symlink_metadata(&a)?;
                if !m.is_file() || m.len() != 0 {
                    return Err(PawError::Validation(format!(
                        "~/{} has content now; not removing it",
                        e.a
                    )));
                }
                self.root.remove_file(&a)?;
            }
            "rename" | "move" => {
                let b = b.ok_or_else(|| PawError::Exec("journal entry is damaged".into()))?;
                if !gone(&a) {
                    return Err(PawError::Validation(format!("~/{} exists again; not overwriting", e.a)));
                }
                self.root.rename(&b, &self.root, &a)?;
            }
            "delete" => {
                let b = b.ok_or_else(|| PawError::Exec("journal entry is damaged".into()))?;
                if !gone(&a) {
                    return Err(PawError::Validation(format!("~/{} exists again; not overwriting", e.a)));
                }
                if let Some(parent) = a.parent() {
                    if !parent.as_os_str().is_empty() {
                        self.root.create_dir_all(parent)?;
                    }
                }
                self.root.rename(&b, &self.root, &a)?;
            }
            "wallpaper" => {
                let prev = b.ok_or_else(|| {
                    PawError::Validation(
                        "no earlier Pawline-set wallpaper is known, so I cannot revert this one".into(),
                    )
                })?;
                if !self.root.symlink_metadata(&prev).map(|m| m.is_file()).unwrap_or(false) {
                    return Err(PawError::Validation("the previous wallpaper copy is missing".into()));
                }
                self.wallpaper.set(&self.root_path.join(&prev), None)?;
            }
            k => return Err(PawError::Exec(format!("cannot undo a \"{k}\" entry"))),
        }
        self.append_journal(&crate::journal::undone_line(e.id))?;
        Ok(format!("undone: {}", e.describe_undo()))
    }

    fn rel(&self, p: &Path) -> Result<PathBuf, PawError> {
        p.strip_prefix(&self.root_path)
            .map(|r| r.to_path_buf())
            .map_err(|_| {
                PawError::UnsafePath(format!(
                    "{} is outside the root {}",
                    p.display(),
                    self.root_path.display()
                ))
            })
    }
}

impl ExecutionBackend for DirectFsBackend {
    fn name(&self) -> &'static str {
        "direct-fs"
    }

    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError> {
        match op {
            Operation::CreateDir { path } => {
                self.root.create_dir(self.rel(path)?)?;
                self.record("create_dir", path, None)?;
                Ok(done(op, "folder created"))
            }
            Operation::CreateFile { path } => {
                self.root
                    .open_with(
                        self.rel(path)?,
                        cap_std::fs::OpenOptions::new().write(true).create_new(true),
                    )
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            PawError::Validation(format!("{} already exists", path.display()))
                        } else {
                            PawError::Exec(e.to_string())
                        }
                    })?;
                self.record("create_file", path, None)?;
                Ok(done(op, "file created"))
            }
            Operation::Delete { path } => {
                let rel = self.rel(path)?;
                // Nothing is destroyed: the item moves into the Pawline trash
                // and the journal remembers where it came from.
                self.root.symlink_metadata(&rel)?;
                if rel.starts_with(crate::journal::STORE) {
                    return Err(PawError::UnsafePath("Pawline's own store is off limits".into()));
                }
                self.root.create_dir_all(crate::journal::TRASH)?;
                let id = crate::journal::next_id(&self.read_journal());
                let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let stored = format!("{}/{id}-{name}", crate::journal::TRASH);
                self.root.rename(&rel, &self.root, &stored)?;
                self.record("delete", path, Some(stored))?;
                Ok(done(op, "moved to the Pawline trash (undo brings it back)"))
            }
            Operation::Rename { from, to } | Operation::Move { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                self.root.rename(self.rel(from)?, &self.root, self.rel(to)?)?;
                let kind = if matches!(op, Operation::Move { .. }) { "move" } else { "rename" };
                self.record(kind, from, Some(self.rel(to)?.to_string_lossy().to_string()))?;
                Ok(done(op, "done"))
            }
            Operation::Copy { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                copy_tree(&self.root, &self.rel(from)?, &self.rel(to)?)?;
                Ok(done(op, "copied"))
            }
            Operation::List { path } => {
                let mut names = Vec::new();
                for e in self.root.read_dir(self.rel(path)?)? {
                    names.push(e?.file_name().to_string_lossy().to_string());
                }
                names.sort();
                Ok(OpOutcome {
                    op: op.clone(),
                    detail: if names.is_empty() {
                        "(empty)".into()
                    } else {
                        names.join(", ")
                    },
                })
            }
            Operation::Undo { entry, .. } => {
                let detail = self.undo(*entry)?;
                Ok(done(op, &detail))
            }
            Operation::SetWallpaper { image, via } => {
                // Open beneath the root handle, check the OPEN file (not the
                // path), then copy its bytes into Pawline's private store.
                // The external tool only ever sees our copy, so nothing can
                // swap the picture between this check and the tool opening it.
                use std::io::Read;
                let rel = self.rel(image)?;
                if !crate::wallpaper::extension_ok(image) {
                    return Err(PawError::Validation(format!(
                        "{} is not a picture I can use",
                        image.display()
                    )));
                }
                let mut f = self.root.open(&rel)?;
                let meta = f.metadata()?;
                if !meta.is_file() {
                    return Err(PawError::Validation(format!(
                        "{} is no longer a usable picture",
                        image.display()
                    )));
                }
                const MAX: u64 = 64 * 1024 * 1024;
                if meta.len() > MAX {
                    return Err(PawError::Validation("that picture is over 64 MB".into()));
                }
                let mut bytes = Vec::new();
                (&mut f).take(MAX + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX {
                    return Err(PawError::Validation("that picture is over 64 MB".into()));
                }
                let journal = self.read_journal();
                let id = crate::journal::next_id(&journal);
                let previous = crate::journal::last_open_of(&journal, "wallpaper").map(|e| e.a);
                self.root.create_dir_all(crate::journal::WALLPAPERS)?;
                let name = rel
                    .file_name()
                    .map(|n| n.to_string_lossy().replace(['/', ' '], "_"))
                    .unwrap_or_else(|| "wallpaper".into());
                let stored_rel = format!("{}/{id}-{name}", crate::journal::WALLPAPERS);
                {
                    use cap_std::fs::OpenOptionsExt;
                    use std::io::Write;
                    let mut out = self.root.open_with(
                        &stored_rel,
                        cap_std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600),
                    )?;
                    out.write_all(&bytes)?;
                }
                let stored = self.root_path.join(&stored_rel);
                let detail = self.wallpaper.set(&stored, *via)?;
                let e = crate::journal::Entry { id, kind: "wallpaper".into(), a: stored_rel, b: previous };
                self.append_journal(&e.to_line()).map_err(|err| {
                    PawError::Exec(format!("wallpaper set, but it could not be recorded for undo: {err}"))
                })?;
                Ok(done(op, &detail))
            }
            Operation::OpenApp { name } => Err(PawError::Exec(format!(
                "launching apps ({name}) is a UI-layer job; not done by the fs backend"
            ))),
            Operation::Answer { text } => Ok(OpOutcome {
                op: op.clone(),
                detail: text.clone(),
            }),
            Operation::Search { engine, query } => {
                let text = self.search(*engine, query)?;
                Ok(OpOutcome {
                    op: op.clone(),
                    detail: text,
                })
            }
        }
    }
}

/// Recursive copy beneath the root handle; symlinks are copied as links,
/// never followed out.
fn copy_tree(dir: &Dir, from: &Path, to: &Path) -> Result<(), PawError> {
    let meta = dir.symlink_metadata(from)?;
    if meta.is_dir() {
        dir.create_dir(to)?;
        for e in dir.read_dir(from)? {
            let e = e?;
            copy_tree(dir, &from.join(e.file_name()), &to.join(e.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        let target = dir.read_link(from)?;
        dir.symlink(&target, to)?;
    } else {
        dir.copy(from, dir, to)?;
    }
    Ok(())
}

/// Last-line guard: never silently overwrite, even if a caller skips
/// plan-level checks.
fn refuse_clobber(dir: &Dir, rel: &Path) -> Result<(), PawError> {
    if dir.try_exists(rel).unwrap_or(false) {
        Err(PawError::ConfirmationRequired(format!(
            "{} exists; not overwriting without confirmation",
            rel.display()
        )))
    } else {
        Ok(())
    }
}

fn done(op: &Operation, detail: &str) -> OpOutcome {
    OpOutcome {
        op: op.clone(),
        detail: detail.into(),
    }
}

/// Records operations without touching anything: previews and tests.
#[derive(Default)]
pub struct DryRunBackend {
    pub log: std::sync::Mutex<Vec<String>>,
}

impl ExecutionBackend for DryRunBackend {
    fn name(&self) -> &'static str {
        "dry-run"
    }
    fn execute(&self, op: &Operation) -> Result<OpOutcome, PawError> {
        self.log.lock().unwrap().push(op.describe());
        Ok(done(op, "would run"))
    }
}
