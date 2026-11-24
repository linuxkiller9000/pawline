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
}

impl DirectFsBackend {
    pub fn new(root: &Path) -> Result<Self, PawError> {
        let canon = std::fs::canonicalize(root)?;
        let dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority())?;
        Ok(DirectFsBackend {
            root_path: canon,
            root: dir,
        })
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
                Ok(done(op, "file created"))
            }
            Operation::Delete { path } => {
                let rel = self.rel(path)?;
                let meta = self.root.symlink_metadata(&rel)?;
                if meta.is_dir() {
                    self.root.remove_dir_all(&rel)?;
                } else {
                    self.root.remove_file(&rel)?;
                }
                Ok(done(op, "deleted"))
            }
            Operation::Rename { from, to } | Operation::Move { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                self.root.rename(self.rel(from)?, &self.root, self.rel(to)?)?;
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
