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
use crate::platform::rename_noreplace;

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
    fn weather(&self, _place: &str) -> Result<String, PawError> {
        Err(PawError::Exec("weather is not available in this backend".into()))
    }

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
        crate::platform::create_private_dir(&self.root, rel)?;
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
        use std::io::Write;
        self.private_dir(crate::journal::STORE)?;
        let mut o = cap_std::fs::OpenOptions::new();
        o.append(true).create(true);
        crate::platform::private_mode(&mut o);
        let mut f = self.root.open_with(crate::journal::JOURNAL, &o)?;
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

    /// Presentation from an outline file: built in memory, written to a unique temp file, published with no-replace.
    pub fn deck(&self, from: &Path, to: &Path) -> Result<String, PawError> {
        use std::io::Read;
        let rf = self.rel(from)?;
        let mut src = self.root.open(&rf)?.into_std();
        let sm = src.metadata()?;
        if !sm.is_file() || sm.len() > crate::deck::MAX_SOURCE {
            return Err(PawError::Validation(format!("{} is not a plain text outline of at most 256 KB", from.display())));
        }
        let mut text = String::new();
        Read::take(&mut src, crate::deck::MAX_SOURCE + 1).read_to_string(&mut text).map_err(|_| PawError::Validation("the outline is not readable UTF-8 text".into()))?;
        let rd = crate::deck::confined_image_reader(&self.root_path, from.parent().unwrap_or(&self.root_path));
        let (slides, rep) = crate::deck::compose_with(&text, &rd).map_err(PawError::Validation)?;
        self.publish_deck(&slides, rep.theme, to, "The outline is untouched.")
    }

    /// The reviewed draft text (held in memory by the UI) as a new deck: same build and no-replace publish as `deck`.
    pub fn deck_text(&self, text: &str, to: &Path) -> Result<String, PawError> {
        let rd = crate::deck::confined_image_reader(&self.root_path, &self.root_path);
        let (slides, rep) = crate::deck::compose_with(text, &rd).map_err(PawError::Validation)?;
        self.publish_deck(&slides, rep.theme, to, "It was written from the draft you reviewed.")
    }

    fn publish_deck(&self, slides: &[crate::deck::Slide], theme: crate::deck::Theme, to: &Path, note: &str) -> Result<String, PawError> {
        use std::io::Write;
        let rt = self.rel(to)?;
        let bytes = crate::deck::build_pptx_themed(slides, theme);
        refuse_clobber(&self.root, &rt)?;
        self.tracked("copy", self.rel_str(to)?, None, |id| {
            let nf = NewFile::create(&self.root, &rt)?;
            let mut out = &nf.file;
            if let Err(e) = out.write_all(&bytes).and_then(|_| out.sync_all()) {
                nf.discard(&self.root);
                return Err(e.into());
            }
            nf.publish(&self.root, &rt).inspect_err(|_| nf.discard(&self.root))?;
            Ok((format!("Saved {} ({} slides). Open it with LibreOffice Impress, PowerPoint or Google Slides. {note} Undo moves the new file to the Pawline trash.", to.display(), slides.len()), self.fingerprint_full(&rt)))
        })
    }

    /// Smaller copy of one video, published only after it was checked. See `shrink.rs`.
    /// ffmpeg and ffprobe work on file handles this function opened through the confined root (the source as stdin,
    /// the temporary file as stdout), never on paths, so nothing can be swapped in between the checks and the work.
    pub fn shrink(&self, from: &Path, to: &Path, cancelled: &dyn Fn() -> bool) -> Result<String, PawError> {
        use crate::shrink as sh;
        let (rf, rt) = (self.rel(from)?, self.rel(to)?);
        let ex = |m: String| PawError::Exec(m);
        let src = self.root.open(&rf)?.into_std();
        let sm = src.metadata()?; // the handle's own metadata, not the path's
        if !sm.is_file() || sm.len() == 0 || !sh::is_video(from) {
            return Err(PawError::Validation(format!("{} is not a video file I can shrink", from.display())));
        }
        let src_size = sm.len();
        if !Path::new("/dev/fd/0").exists() {
            return Err(ex("this system has no /dev/fd, so I can't run ffmpeg on exact open files; nothing was changed".into()));
        }
        let ffmpeg = sh::find_exe("ffmpeg").ok_or_else(|| ex("ffmpeg is not installed, so I can't shrink videos. It is free: install it with your package manager (for example \"sudo apt install ffmpeg\"). Nothing was changed.".into()))?;
        let ffprobe = sh::find_exe("ffprobe").ok_or_else(|| ex("ffprobe (part of ffmpeg) is missing, so I couldn't check the result. Nothing was changed.".into()))?;
        let (src_dur, src_v, src_a) = sh::probe(&ffprobe, src.try_clone()?, cancelled)
            .ok_or_else(|| ex("I could not read that video (ffprobe failed or took too long). Nothing was changed.".into()))?;
        if src_v == 0 {
            return Err(ex("that file has no video stream. Nothing was changed.".into()));
        }
        refuse_clobber(&self.root, &rt)?;
        self.tracked("copy", self.rel_str(to)?, None, |id| {
            let nf = NewFile::create(&self.root, &rt)?;
            let out = nf.file.try_clone()?;
            let fail = |root: &Dir, m: &str| -> PawError {
                nf.discard(root);
                PawError::Exec(m.to_string())
            };
            let limit = std::time::Duration::from_secs(sh::TIMEOUT_SECS);
            match sh::run_bounded(&ffmpeg, &sh::ffmpeg_args().iter().map(|a| a.to_str().unwrap_or("")).collect::<Vec<_>>(), src.try_clone()?, Some(out.try_clone()?), limit, cancelled) {
                Err(e) if e == "cancelled" => return Err(fail(&self.root, "Cancelled. The smaller copy was not kept. Your video was not touched.")),
                Err(e) if e == "timeout" => return Err(fail(&self.root, "Shrinking took too long and was stopped. Nothing was kept. Your video was not touched.")),
                Err(e) => return Err(fail(&self.root, &format!("could not run ffmpeg: {e}. Nothing was kept."))),
                Ok((false, _)) => return Err(fail(&self.root, "ffmpeg could not shrink that video. Nothing was kept. Your video was not touched.")),
                Ok((true, _)) => {}
            }
            // verify on the open temp file: same length, same streams, and it decodes end to end
            let out_size = out.metadata().map(|m| m.len()).unwrap_or(0);
            let probed = sh::probe(&ffprobe, out.try_clone()?, cancelled);
            if probed.is_none() && cancelled() {
                return Err(fail(&self.root, "Cancelled. The smaller copy was not kept. Your video was not touched."));
            }
            let streams_ok = matches!(probed, Some((d, v, a)) if v == 1 && a == src_a && (d - src_dur).abs() <= (src_dur * 0.02).max(1.0));
            if out_size == 0 || !streams_ok {
                return Err(fail(&self.root, "The shrunk copy did not pass the check (wrong length, or it lost a video or audio stream), so I threw it away. Your video was not touched."));
            }
            match sh::decodes_fully(&ffmpeg, out.try_clone()?, limit, cancelled) {
                Ok(true) => {}
                Err(e) if e == "cancelled" => return Err(fail(&self.root, "Cancelled. The smaller copy was not kept. Your video was not touched.")),
                _ => return Err(fail(&self.root, "The shrunk copy did not play through without errors, so I threw it away. Your video was not touched.")),
            }
            if out_size >= src_size {
                return Err(fail(&self.root, &format!(
                    "Shrinking would not make it smaller ({} to {}), so I kept only your original. Nothing new was saved.",
                    sh::mb(src_size),
                    sh::mb(out_size)
                )));
            }
            nf.publish(&self.root, &rt).inspect_err(|_| nf.discard(&self.root))?;
            let pct = 100 - (out_size * 100 / src_size);
            Ok((format!("Saved {} ({} to {}, {pct}% smaller). Checked: it decodes end to end, same length, {src_v}->1 video and {src_a} audio stream(s). Your original is untouched. Undo moves the new copy to the Pawline trash.", to.display(), sh::mb(src_size), sh::mb(out_size)), self.fingerprint_full(&rt)))
        })
    }

    /// mtime + inode + size: changes when the content is edited or the file replaced.
    fn fingerprint_full(&self, rel: &Path) -> Option<String> {
        let m = self.root.symlink_metadata(rel).ok()?;
        if m.is_dir() {
            return tree_fingerprint(&self.root, rel);
        }
        let t = m.modified().ok()?.into_std().duration_since(std::time::UNIX_EPOCH).ok()?;
        let id = crate::platform::id_cap(&m)?; // no file identity on this system: no fingerprint, so undo refuses
        Some(format!("{}:{}:{}", t.as_nanos(), id.1, m.len()))
    }

    fn fingerprint(&self, rel: &Path) -> Option<String> {
        let m = self.root.symlink_metadata(rel).ok()?;
        let t = m.modified().ok()?.into_std().duration_since(std::time::UNIX_EPOCH).ok()?;
        let id = crate::platform::id_cap(&m)?;
        Some(format!("{}:{}", t.as_nanos(), id.1))
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
                match &e.meta {
                    Some(want) if self.fingerprint_full(&b).as_deref() == Some(want.as_str()) => {}
                    Some(_) => {
                        return Err(PawError::Validation(format!(
                            "~/{} was changed (or replaced) after I moved it; leaving it where it is so your edits are kept",
                            e.b.as_deref().unwrap_or("")
                        )));
                    }
                    None => {
                        return Err(PawError::Validation(format!(
                            "I can't verify that ~/{} is unchanged since it was moved (this history entry has no fingerprint), so I left it where it is. Move it back yourself if you want.",
                            e.b.as_deref().unwrap_or("")
                        )));
                    }
                }
                rename_noreplace(&self.root, &b, &a)?;
            }
            "copy" => {
                // Not destroyed: the copy goes to the trash.
                self.private_dir(crate::journal::TRASH)?;
                let name = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.root.symlink_metadata(&a)?;
                match &e.meta {
                    Some(want) if self.fingerprint_full(&a).as_deref() == Some(want.as_str()) => {}
                    Some(_) => {
                        return Err(PawError::Validation(format!(
                            "~/{} was changed (or replaced) after I made it; leaving it where it is so your edits are kept",
                            e.a
                        )));
                    }
                    None => {
                        return Err(PawError::Validation(format!(
                            "I can't verify that ~/{} is unchanged since I made it (this history entry has no fingerprint, or the folder is too large to check), so I left it where it is. Remove it yourself if you want.",
                            e.a
                        )));
                    }
                }
                rename_noreplace(&self.root, &a, Path::new(&format!("{}/{}-undo-{name}", crate::journal::TRASH, e.id)))?;
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
                rename_noreplace(&self.root, &b, &a)?;
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
        let mut o = cap_std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        crate::platform::private_mode(&mut o);
        let mut out = self.root.open_with(&rel, &o)?;
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
                    rename_noreplace(&self.root, &rel, Path::new(&stored))?;
                    Ok(((), None))
                })?;
                Ok(done(op, "moved to the Pawline trash (undo brings it back)"))
            }
            Operation::Rename { from, to } | Operation::Move { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                let kind = if matches!(op, Operation::Move { .. }) { "move" } else { "rename" };
                let (rf, rt) = (self.rel(from)?, self.rel(to)?);
                // Fingerprint of what is being moved, so undo can tell if the moved file was edited since.
                let before = self.fingerprint_full(&rf);
                self.tracked(kind, self.rel_str(from)?, Some(self.rel_str(to)?), |_| {
                    rename_noreplace(&self.root, &rf, &rt)?;
                    Ok(((), before))
                })?;
                Ok(done(op, "done"))
            }
            Operation::Copy { from, to } => {
                refuse_clobber(&self.root, &self.rel(to)?)?;
                let (rf, rt) = (self.rel(from)?, self.rel(to)?);
                self.tracked("copy", self.rel_str(to)?, None, |_| {
                    copy_tree(&self.root, &rf, &rt)?;
                    Ok(((), self.fingerprint_full(&rt)))
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
            Operation::Deck { from, to } => {
                let detail = self.deck(from, to)?;
                Ok(done(op, &detail))
            }
            Operation::DeckText { text, to } => {
                let detail = self.deck_text(text, to)?;
                Ok(done(op, &detail))
            }
            Operation::DeckDraft { .. } => Err(PawError::Exec("Writing a draft needs the online step, which this runner does not have. Nothing was sent.".into())),
            Operation::Shrink { from, to } => {
                let detail = self.shrink(from, to, &|| false)?;
                Ok(done(op, &detail))
            }
            Operation::OpenApp { name, .. } => Err(PawError::Exec(format!(
                "launching apps ({name}) is a UI-layer job; not done by the fs backend"
            ))),
            Operation::GenerateImage { .. } | Operation::AskOnline { .. } | Operation::ChatOnline { .. } => Err(PawError::Exec(
                "making pictures is a UI-layer job; not done by the fs backend".into(),
            )),
            Operation::Answer { text } => Ok(OpOutcome {
                op: op.clone(),
                detail: text.clone(),
            }),
            Operation::Weather { place } => {
                let text = self.weather(place)?;
                Ok(OpOutcome { op: op.clone(), detail: text })
            }
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
/// Copy one regular file with create_new. If the copy fails, the partial file is removed only when the name still
/// points at the file this call created. `after_create` lets tests inject a failure at that point.
/// A new file being written before it gets its final name. It is an UNNAMED file (O_TMPFILE) in the target folder,
/// given its name by linkat, which can never replace an existing name. There is deliberately no named-temporary
/// fallback: if the folder's filesystem cannot make unnamed files, saving fails closed and nothing is created,
/// because a named temporary file could be swapped between our check and the rename.
struct NewFile {
    file: std::fs::File,
    mine: crate::platform::FileId,
}

type AnonFn = fn(&Dir, &Path) -> std::io::Result<std::fs::File>;

impl NewFile {
    fn create(root: &Dir, rt: &Path) -> Result<Self, PawError> {
        Self::create_via(root, rt, crate::platform::anon_file)
    }

    fn create_via(root: &Dir, rt: &Path, anon: AnonFn) -> Result<Self, PawError> {
        let dir = rt.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let f = anon(root, &dir).map_err(|e| {
            PawError::Exec(format!(
                "this folder cannot save a new file safely (no unnamed files: {e}), so I did not start. Nothing was changed."
            ))
        })?;
        let mine = crate::platform::id_std(&f.metadata()?).ok_or_else(|| PawError::Exec("cannot tell files apart on this system, so I did not start".into()))?;
        Ok(NewFile { file: f, mine })
    }

    /// Nothing to clean up: an unnamed file disappears when it is dropped.
    fn discard(&self, _root: &Dir) {}

    fn publish(&self, root: &Dir, rt: &Path) -> Result<(), PawError> {
        let dir = rt.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let name = rt.file_name().ok_or_else(|| PawError::Exec("bad path".into()))?;
        crate::platform::link_anon(root, &dir, &self.file, name)?;
        // Detects a replacement of the name right after publishing. It cannot detect an in-place edit that keeps the
        // same inode; before the name existed nobody else could open the file, so what was checked is what was published.
        let landed = root.symlink_metadata(rt).ok().and_then(|m| crate::platform::id_cap(&m));
        if landed != Some(self.mine) {
            return Err(PawError::Exec("the new file was swapped right after saving; I can't vouch for it, please check it. Nothing else was touched.".into()));
        }
        Ok(())
    }
}

fn copy_file(dir: &Dir, from: &Path, to: &Path, after_create: &dyn Fn() -> std::io::Result<()>) -> Result<(), PawError> {
    let mut src = dir.open(from)?;
    let mut dst = dir.open_with(to, cap_std::fs::OpenOptions::new().write(true).create_new(true))?;
    let mine = dst.metadata().ok().and_then(|m| crate::platform::id_cap(&m));
    let res = after_create().and_then(|_| std::io::copy(&mut src, &mut dst).map(|_| ())).and_then(|_| dst.sync_all());
    if let Err(e) = res {
        let same = dir.symlink_metadata(to).ok().and_then(|m| crate::platform::id_cap(&m));
        if mine.is_some() && same == mine {
            let _ = dir.remove_file(to);
        }
        return Err(e.into());
    }
    if let Ok(m) = src.metadata() {
        let _ = dst.set_permissions(m.permissions());
    }
    Ok(())
}

/// Bounded manifest of a folder tree: every entry's relative path, kind, size, mtime, inode (and link target),
/// hashed (FNV-1a 64). Sees nested edits, additions, removals and replacements by another file. It does not read
/// file contents, so an edit that keeps size, mtime and inode (mtime can be set by hand) is not seen.
/// None when the tree is larger than the bound or anything cannot be read: callers then refuse Undo.
const TREE_LIMIT: usize = 20_000;

fn tree_fingerprint(dir: &Dir, rel: &Path) -> Option<String> {
    fn walk(dir: &Dir, rel: &Path, pre: &Path, out: &mut Vec<String>) -> Option<()> {
        let mut ents: Vec<_> = dir.read_dir(rel).ok()?.collect::<Result<Vec<_>, _>>().ok()?;
        ents.sort_by_key(|e| e.file_name());
        for e in ents {
            if out.len() >= TREE_LIMIT {
                return None;
            }
            let (name, p, q) = (e.file_name(), rel.join(e.file_name()), pre.join(e.file_name()));
            let m = dir.symlink_metadata(&p).ok()?;
            let t = m.modified().ok()?.into_std().duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
            let id = crate::platform::id_cap(&m)?;
            let kind = if m.is_dir() { 'd' } else if m.file_type().is_symlink() { 'l' } else { 'f' };
            let link = if kind == 'l' { dir.read_link(&p).ok()?.to_string_lossy().to_string() } else { String::new() };
            out.push(format!("{}\0{kind}\0{}\0{t}\0{}\0{link}", q.display(), m.len(), id.1));
            let _ = name;
            if kind == 'd' {
                walk(dir, &p, &q, out)?;
            }
        }
        Some(())
    }
    let mut out = Vec::new();
    walk(dir, rel, Path::new(""), &mut out)?;
    let mut h: u64 = 0xcbf29ce484222325;
    for line in &out {
        for b in line.bytes().chain(std::iter::once(b'\n')) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    Some(format!("tree:{}:{h:016x}", out.len()))
}

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
        crate::platform::symlink(dir, &target, to)?;
    } else {
        copy_file(dir, from, to, &|| Ok(()))?;
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

#[cfg(test)]
mod noreplace_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pawline-nr-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn unnamed_file_gets_its_name_only_at_publish_and_never_replaces() {
        use std::io::Write;
        let root = tmp("anon");
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let rt = Path::new("sub/out.bin");
        let nf = match NewFile::create(&dir, rt) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("no O_TMPFILE on this filesystem ({e}); fail-closed test covers it");
                return;
            }
        };
        (&nf.file).write_all(b"mine").unwrap();
        assert!(names(&root.join("sub")).is_empty(), "no name exists before publish");
        nf.publish(&dir, rt).unwrap();
        assert_eq!(std::fs::read(root.join("sub/out.bin")).unwrap(), b"mine");
        let nf2 = NewFile::create(&dir, rt).unwrap();
        (&nf2.file).write_all(b"other").unwrap();
        let err = nf2.publish(&dir, rt).unwrap_err();
        assert!(matches!(err, PawError::ConfirmationRequired(_)), "{err}");
        assert_eq!(std::fs::read(root.join("sub/out.bin")).unwrap(), b"mine");
        drop(nf2);
        assert_eq!(names(&root.join("sub")), vec!["out.bin"]);
        std::fs::write(root.join("sub/taken"), "theirs").unwrap();
        let nf3 = NewFile::create(&dir, Path::new("sub/taken")).unwrap();
        assert!(nf3.publish(&dir, Path::new("sub/taken")).is_err());
        assert_eq!(std::fs::read(root.join("sub/taken")).unwrap(), b"theirs");
    }

    #[test]
    fn when_unnamed_files_are_unsupported_saving_fails_closed_and_creates_nothing() {
        let root = tmp("failclosed");
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        fn no(_: &Dir, _: &Path) -> std::io::Result<std::fs::File> {
            Err(std::io::Error::from_raw_os_error(95))
        }
        let r = NewFile::create_via(&dir, Path::new("sub/x.bin"), no);
        assert!(r.is_err());
        assert!(names(&root.join("sub")).is_empty(), "no named temporary file is ever created");
    }

    #[test]
    fn tree_fingerprint_sees_nested_edits_additions_and_replacements() {
        let root = tmp("tree");
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        std::fs::create_dir_all(root.join("t/a/b")).unwrap();
        std::fs::write(root.join("t/a/b/f.txt"), "one").unwrap();
        std::fs::write(root.join("t/top.txt"), "x").unwrap();
        let fp = || tree_fingerprint(&dir, Path::new("t"));
        let base = fp().unwrap();
        assert_eq!(fp().unwrap(), base, "stable when nothing changed");
        std::fs::write(root.join("t/a/b/f.txt"), "one!").unwrap();
        let edited = fp().unwrap();
        assert_ne!(edited, base, "nested edit");
        std::fs::write(root.join("t/a/b/new.txt"), "n").unwrap();
        let added = fp().unwrap();
        assert_ne!(added, edited, "nested addition");
        std::fs::remove_file(root.join("t/a/b/new.txt")).unwrap();
        std::fs::write(root.join("t/top.txt"), "x").unwrap();
        let tmpf = root.join("t/top2");
        std::fs::write(&tmpf, "x").unwrap();
        std::fs::rename(&tmpf, root.join("t/top.txt")).unwrap();
        assert_ne!(fp().unwrap(), edited, "same-size replacement by another file (new inode)");
    }

    #[test]
    fn a_competitor_destination_survives_byte_for_byte() {
        let d = tmp("one");
        std::fs::write(d.join("src.txt"), "SOURCE").unwrap();
        std::fs::write(d.join("sub/dst.txt"), "COMPETITOR").unwrap();
        let root = Dir::open_ambient_dir(&d, cap_std::ambient_authority()).unwrap();
        // the check-then-rename gap is gone: the kernel refuses at rename time even though no pre-check ran
        let r = rename_noreplace(&root, Path::new("src.txt"), Path::new("sub/dst.txt"));
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(d.join("sub/dst.txt")).unwrap(), "COMPETITOR");
        assert_eq!(std::fs::read_to_string(d.join("src.txt")).unwrap(), "SOURCE");
        // and a free name works
        rename_noreplace(&root, Path::new("src.txt"), Path::new("sub/free.txt")).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("sub/free.txt")).unwrap(), "SOURCE");
        assert!(!d.join("src.txt").exists());
    }

    #[test]
    fn racing_creators_never_lose_either_file() {
        let d = tmp("race");
        let root = Dir::open_ambient_dir(&d, cap_std::ambient_authority()).unwrap();
        let mut moved = 0;
        for i in 0..300 {
            std::fs::write(d.join("s.txt"), "SOURCE").unwrap();
            let dst = d.join("sub").join(format!("d{i}.txt"));
            let dst2 = dst.clone();
            let t = std::thread::spawn(move || {
                let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(&dst2).and_then(|mut f| std::io::Write::write_all(&mut f, b"COMP"));
            });
            let r = rename_noreplace(&root, Path::new("s.txt"), Path::new(&format!("sub/d{i}.txt")));
            t.join().unwrap();
            let got = std::fs::read_to_string(&dst).unwrap_or_default();
            if r.is_ok() {
                moved += 1;
                assert!(got == "SOURCE" || got == "COMP" || got.is_empty(), "{got}");
                let _ = std::fs::remove_file(&dst);
            } else {
                assert_eq!(std::fs::read_to_string(d.join("s.txt")).unwrap(), "SOURCE", "source kept when refused");
                assert_eq!(got, "COMP", "competitor bytes survive");
            }
        }
        assert!(moved > 0);
    }
}

#[cfg(test)]
mod copy_failure_tests {
    use super::*;

    #[test]
    fn failed_copy_removes_only_its_own_partial_file_and_never_a_replacement() {
        let d = std::env::temp_dir().join(format!("pawline-cf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let d = std::fs::canonicalize(&d).unwrap();
        std::fs::write(d.join("src"), "SRC").unwrap();
        let root = Dir::open_ambient_dir(&d, cap_std::ambient_authority()).unwrap();
        // 1: failure, nobody replaced it: the partial file is gone
        let r = copy_file(&root, Path::new("src"), Path::new("dst1"), &|| Err(std::io::Error::other("boom")));
        assert!(r.is_err() && !d.join("dst1").exists());
        // 2: failure after another process replaced the destination: its bytes survive
        let dd = d.clone();
        let r = copy_file(&root, Path::new("src"), Path::new("dst2"), &move || {
            std::fs::write(dd.join("other"), "THEIRS")?;
            std::fs::rename(dd.join("other"), dd.join("dst2"))?;
            Err(std::io::Error::other("boom"))
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read_to_string(d.join("dst2")).unwrap(), "THEIRS");
        // 3: success
        copy_file(&root, Path::new("src"), Path::new("dst3"), &|| Ok(())).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("dst3")).unwrap(), "SRC");
    }
}
