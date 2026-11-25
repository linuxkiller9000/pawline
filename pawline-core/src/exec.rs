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
    recovery_notes: Vec<String>,
}

const MAX_PICTURE: u64 = 64 * 1024 * 1024;

impl DirectFsBackend {
    pub fn new(root: &Path) -> Result<Self, PawError> {
        let canon = std::fs::canonicalize(root)?;
        let dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority())?;
        let mut me = DirectFsBackend {
            root_path: canon,
            root: dir,
            wallpaper: Box::new(crate::wallpaper::SystemWallpaper),
            recovery_notes: Vec::new(),
        };
        me.recover();
        Ok(me)
    }

    /// Replace the wallpaper setter (tests, or a UI-specific one).
    pub fn with_wallpaper(mut self, w: Box<dyn crate::wallpaper::WallpaperSetter>) -> Self {
        self.wallpaper = w;
        self
    }

    /// What startup recovery found and did (empty when the journal was clean).
    pub fn recovery_notes(&self) -> &[String] {
        &self.recovery_notes
    }

    fn private_dir(&self, rel: &str) -> Result<(), PawError> {
        use cap_std::fs::{DirBuilder, DirBuilderExt};
        let mut b = DirBuilder::new();
        b.recursive(true).mode(0o700);
        self.root.create_dir_with(rel, &b)?;
        // Existing store directories from older runs: tighten if needed.
        Ok(())
    }

    fn read_journal(&self) -> String {
        use std::io::Read;
        let mut s = String::new();
        if let Ok(mut f) = self.root.open(crate::journal::JOURNAL) {
            let _ = f.read_to_string(&mut s);
        }
        s
    }

    fn append(&self, line: &str) -> Result<(), PawError> {
        use cap_std::fs::OpenOptionsExt;
        use std::io::Write;
        self.private_dir(crate::journal::STORE)?;
        let mut f = self.root.open_with(
            crate::journal::JOURNAL,
            cap_std::fs::OpenOptions::new().append(true).create(true).mode(0o600),
        )?;
        writeln!(f, "{line}")?;
        f.sync_all()?;
        Ok(())
    }

    fn rel_str(&self, p: &Path) -> Result<String, PawError> {
        Ok(self.rel(p)?.to_string_lossy().to_string())
    }

    /// Write-ahead: the intent is on disk before the change. If the journal
    /// cannot be written, the change is not made.
    fn tracked<T>(
        &self,
        kind: &str,
        a: String,
        b: Option<String>,
        f: impl FnOnce(u64) -> Result<(T, Option<String>), PawError>,
    ) -> Result<T, PawError> {
        let id = crate::journal::next_id(&self.read_journal());
        let e = crate::journal::Entry { id, kind: kind.into(), a, b, meta: None };
        self.append(&e.intent_line())
            .map_err(|err| PawError::Exec(format!("could not write the undo journal, so nothing was changed: {err}")))?;
        match f(id) {
            Ok((v, meta)) => {
                self.append(&crate::journal::commit_line(id, meta.as_deref())).map_err(|err| {
                    PawError::Exec(format!(
                        "{kind} done, but the journal commit failed ({err}); it will be reconciled at next start"
                    ))
                })?;
                Ok(v)
            }
            Err(err) => {
                let _ = self.append(&crate::journal::abort_line(id));
                Err(err)
            }
        }
    }

    fn fingerprint(&self, rel: &Path) -> Option<String> {
        use cap_std::fs::MetadataExt;
        let m = self.root.symlink_metadata(rel).ok()?;
        let t = m.modified().ok()?.into_std().duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(format!("{}:{}", t.as_nanos(), m.ino()))
    }

    /// Settle journal intents left pending by a crash or a failed write.
    fn recover(&mut self) {
        use crate::journal as j;
        let text = self.read_journal();
        for e in j::pending(&text) {
            let a = PathBuf::from(&e.a);
            let b = e.b.as_ref().map(PathBuf::from);
            let ex = |p: &Path| self.root.symlink_metadata(p).is_ok();
            let (commit, note): (bool, Option<String>) = match e.kind.as_str() {
                "create_dir" | "create_file" => (ex(&a), None),
                "rename" | "move" | "delete" => match &b {
                    Some(b) if ex(b) && !ex(&a) => (true, None),
                    Some(b) if ex(&a) && !ex(b) => (false, None),
                    _ => (false, Some(format!("unclear state for {} ~/{}; left as is, not undoable", e.kind, e.a))),
                },
                "copy" => (
                    false,
                    ex(&a).then(|| format!("an unfinished copy may remain at ~/{}; check it", e.a)),
                ),
                _ => (
                    false,
                    ex(&a).then(|| format!("an unused wallpaper copy remains at ~/{}", e.a)),
                ),
            };
            let meta = if commit && e.kind == "create_file" { self.fingerprint(&a) } else { None };
            let line = if commit {
                j::commit_line(e.id, meta.as_deref())
            } else {
                j::abort_line(e.id)
            };
            if self.append(&line).is_ok() {
                self.recovery_notes.push(format!(
                    "recovered {} ~/{}: {}",
                    e.kind,
                    e.a,
                    if commit { "it had completed" } else { "it had not completed" }
                ));
            }
            if let Some(n) = note {
                self.recovery_notes.push(n);
            }
        }
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
        let damaged = || PawError::Exec("journal entry is damaged".into());
        match e.kind.as_str() {
            "create_dir" => {
                self.root.remove_dir(&a).map_err(|_| {
                    PawError::Validation(format!("~/{} is missing or not empty; leaving it", e.a))
                })?;
            }
            "create_file" => {
                let m = self.root.symlink_metadata(&a)?;
                let untouched = e.meta.is_some() && self.fingerprint(&a) == e.meta;
                if !m.is_file() || m.len() != 0 || !untouched {
                    return Err(PawError::Validation(format!(
                        "~/{} was changed after I made it; not removing it",
                        e.a
                    )));
                }
                self.root.remove_file(&a)?;
            }
            "rename" | "move" => {
                let b = b.ok_or_else(damaged)?;
                if !gone(&a) {
                    return Err(PawError::Validation(format!("~/{} exists again; not overwriting", e.a)));
                }
                self.root.rename(&b, &self.root, &a)?;
            }
            "copy" => {
                // Not destroyed: the copy goes to the trash.
                self.private_dir(crate::journal::TRASH)?;
                let name = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.root.symlink_metadata(&a)?;
                self.root.rename(&a, &self.root, format!("{}/{}-undo-{name}", crate::journal::TRASH, e.id))?;
            }
            "delete" => {
                let b = b.ok_or_else(damaged)?;
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
                    PawError::Validation("the previous wallpaper is unknown, so I cannot revert this one".into())
                })?;
                if !self.root.symlink_metadata(&prev).map(|m| m.is_file()).unwrap_or(false) {
                    return Err(PawError::Validation("the previous wallpaper copy is missing".into()));
                }
                self.wallpaper.set(&self.root_path.join(&prev), None)?;
            }
            k => return Err(PawError::Exec(format!("cannot undo a \"{k}\" entry"))),
        }
        self.append(&crate::journal::undone_line(e.id)).map_err(|err| {
            PawError::Exec(format!("undone, but the history could not be updated: {err}"))
        })?;
        Ok(format!("undone: {}", e.describe_undo()))
    }

    /// Copy picture bytes into the private store (0600) and return its path
    /// relative to the root.
    fn store_picture(&self, id: u64, name: &str, bytes: &[u8]) -> Result<String, PawError> {
        use cap_std::fs::OpenOptionsExt;
        use std::io::Write;
        self.private_dir(crate::journal::WALLPAPERS)?;
        let name = name.replace(['/', ' '], "_");
        let rel = format!("{}/{id}-{name}", crate::journal::WALLPAPERS);
        let mut out = self.root.open_with(
            &rel,
            cap_std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600),
        )?;
        out.write_all(bytes)?;
        out.sync_all()?;
        Ok(rel)
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
                let rel = self.rel(path)?;
                self.tracked("create_dir", self.rel_str(path)?, None, |_| {
                    self.root.create_dir(&rel)?;
                    Ok(((), None))
                })?;
                Ok(done(op, "folder created"))
            }
            Operation::CreateFile { path } => {
                let rel = self.rel(path)?;
                self.tracked("create_file", self.rel_str(path)?, None, |_| {
                    self.root
                        .open_with(
                            &rel,
                            cap_std::fs::OpenOptions::new().write(true).create_new(true),
                        )
                        .map_err(|e| {
                            if e.kind() == std::io::ErrorKind::AlreadyExists {
                                PawError::Validation(format!("{} already exists", path.display()))
                            } else {
                                PawError::Exec(e.to_string())
                            }
                        })?;
                    Ok(((), self.fingerprint(&rel)))
                })?;
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
                let id_hint = crate::journal::next_id(&self.read_journal());
                let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let stored = format!("{}/{id_hint}-{name}", crate::journal::TRASH);
                self.private_dir(crate::journal::TRASH)?;
                self.tracked("delete", self.rel_str(path)?, Some(stored.clone()), |id| {
                    if id != id_hint {
                        return Err(PawError::Exec("journal changed underneath; try again".into()));
                    }
                    self.root.rename(&rel, &self.root, &stored)?;
                    Ok(((), None))
                })?;
                Ok(done(op, "moved to the Pawline trash (undo brings it back)"))
            }
            Operation::Rename { from, to } | Operation::Move { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                let kind = if matches!(op, Operation::Move { .. }) { "move" } else { "rename" };
                let (rf, rt) = (self.rel(from)?, self.rel(to)?);
                self.tracked(kind, self.rel_str(from)?, Some(self.rel_str(to)?), |_| {
                    self.root.rename(&rf, &self.root, &rt)?;
                    Ok(((), None))
                })?;
                Ok(done(op, "done"))
            }
            Operation::Copy { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                let (rf, rt) = (self.rel(from)?, self.rel(to)?);
                self.tracked("copy", self.rel_str(to)?, None, |_| {
                    copy_tree(&self.root, &rf, &rt)?;
                    Ok(((), None))
                })?;
                Ok(done(op, "copied (undo moves the copy to the trash)"))
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
            Operation::SetWallpaper { image, via, no_backup_ok } => {
                // Open beneath the root handle, check the OPEN file (not the
                // path), then copy its bytes into Pawline's private store.
                // The external tool only ever sees that copy.
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
                if meta.len() > MAX_PICTURE {
                    return Err(PawError::Validation("that picture is over 64 MB".into()));
                }
                let mut bytes = Vec::new();
                (&mut f).take(MAX_PICTURE + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_PICTURE {
                    return Err(PawError::Validation("that picture is over 64 MB".into()));
                }
                let name = rel
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "wallpaper".into());

                // Ground the previous wallpaper before changing anything: the
                // last one Pawline set, else a baseline captured from the
                // running manager (swaybg only), else unknown.
                let journal = self.read_journal();
                let mut previous = crate::journal::last_open_of(&journal, "wallpaper")
                    .or_else(|| crate::journal::last_open_of(&journal, "wallpaper_base"))
                    .map(|e| e.a);
                if previous.is_none() {
                    if let Some(cur) = self.wallpaper.current() {
                        if let Ok(m) = std::fs::metadata(&cur) {
                            if m.is_file() && m.len() <= MAX_PICTURE {
                                if let Ok(cur_bytes) = std::fs::read(&cur) {
                                    let cname = cur.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                    let id = crate::journal::next_id(&journal);
                                    let rel_store = format!("{}/{id}-{cname}", crate::journal::WALLPAPERS);
                                    let base = self.tracked("wallpaper_base", rel_store.clone(), None, |id2| {
                                        let r = self.store_picture(id2, &cname, &cur_bytes)?;
                                        Ok((r, None))
                                    })?;
                                    previous = Some(base);
                                }
                            }
                        }
                    }
                }

                if previous.is_none() && !*no_backup_ok {
                    return Err(PawError::Validation(
                        "your current wallpaper can no longer be backed up, so I changed nothing; ask again ending with \"anyway\" to proceed without a backup".into(),
                    ));
                }
                let journal = self.read_journal();
                let id_hint = crate::journal::next_id(&journal);
                let planned = format!(
                    "{}/{id_hint}-{}",
                    crate::journal::WALLPAPERS,
                    name.replace(['/', ' '], "_")
                );
                let detail = self.tracked("wallpaper", planned, previous, |id| {
                    let stored_rel = self.store_picture(id, &name, &bytes)?;
                    let stored = self.root_path.join(&stored_rel);
                    let d = self.wallpaper.set(&stored, *via)?;
                    Ok((d, None))
                })?;
                Ok(done(op, &detail))
            }
            Operation::OpenApp { name, .. } => Err(PawError::Exec(format!(
                "launching apps ({name}) is a UI-layer job; not done by the fs backend"
            ))),
            Operation::GenerateImage { .. } => Err(PawError::Exec(
                "making pictures is a UI-layer job; not done by the fs backend".into(),
            )),
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
