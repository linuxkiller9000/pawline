//! Resolution + validation: turn directives into a checked Plan.
//! Rules: everything stays under the allowed root, parents exist or are
//! created earlier in the same plan, destructive steps are flagged.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::context::ContextManager;
use crate::error::PawError;
use crate::plan::{CreateKind, Directive, LocationRef, Operation, Plan};

pub struct Validator {
    root: PathBuf,
    /// Test hook: (desktop entry dirs, PATH) to use instead of the real environment.
    pub app_env: Option<(Vec<PathBuf>, String)>,
    /// Ranked folders (zoxide or Pawline's own list) the bridge supplies per request. Used ONLY to find
    /// a destination folder by name; every result is still confined to the home area and shown for confirmation.
    pub dir_hints: Vec<PathBuf>,
    hint_notes: std::cell::RefCell<Vec<String>>,
}

impl Validator {
    pub fn new(root: PathBuf) -> Self {
        // The root itself may live behind a symlink; compare in canonical form.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Validator { root, app_env: None, dir_hints: vec![], hint_notes: std::cell::RefCell::new(vec![]) }
    }

    /// Words like "home container models" -> "models": drop a leading "home" and filler words.
    pub fn place_words(words: &str) -> String {
        let mut w: Vec<&str> = words.trim().trim_end_matches('.').split_whitespace().collect();
        if w.len() > 1 && w[0].eq_ignore_ascii_case("home") {
            w.remove(0);
        }
        w.retain(|x| !matches!(x.to_lowercase().as_str(), "container" | "containing" | "contains" | "folder" | "directory" | "the" | "my" | "called" | "named"));
        w.join(" ")
    }

    /// Existing folders inside the home area called `place`: a direct child of home, the session's, or a hinted one.
    fn place_candidates(&self, place: &str, ctx: &ContextManager) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = vec![];
        let mut add = |p: PathBuf, out: &mut Vec<PathBuf>| {
            if let Ok(c) = std::fs::canonicalize(&p) {
                if c.is_dir() && c.starts_with(&self.root) && !c.starts_with(self.root.join(crate::journal::STORE)) && !out.contains(&c) {
                    out.push(c);
                }
            }
        };
        if place.is_empty() || place.contains('/') {
            return out;
        }
        add(self.root.join(place), &mut out);
        if let Some(p) = ctx.resolve_named(place) {
            add(p, &mut out);
        }
        let exact: Vec<&PathBuf> = self.dir_hints.iter().filter(|h| h.file_name().is_some_and(|f| f.to_string_lossy() == place)).collect();
        let loose: Vec<&PathBuf> = self.dir_hints.iter().filter(|h| h.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase() == place.to_lowercase())).collect();
        for h in if exact.is_empty() { loose } else { exact } {
            add(h.clone(), &mut out);
        }
        out
    }

    /// A destination named in words. Ok(None): nothing found. Several: ask. One: use it, and the plan will ask for a yes.
    fn find_place(&self, words: &str, ctx: &ContextManager) -> Result<Option<PathBuf>, PawError> {
        let place = Self::place_words(words);
        let c = self.place_candidates(&place, ctx);
        match c.len() {
            0 => Ok(None),
            1 => {
                self.hint_notes.borrow_mut().push(format!("I took \"{words}\" to mean the folder {}.", c[0].display()));
                Ok(Some(c[0].clone()))
            }
            _ => Err(PawError::UnknownReference(format!(
                "{place}\" (several folders have that name: {}. Say the full path, for example \"make a folder called x in <path>\"",
                c.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")
            ))),
        }
    }

    fn split_in_destination(&self, names: &[String], parent: &LocationRef, ctx: &ContextManager) -> Result<(Vec<String>, Option<PathBuf>), PawError> {
        if !matches!(parent, LocationRef::CurrentDir) || names.len() != 1 {
            return Ok((names.to_vec(), None));
        }
        let n = &names[0];
        if let Some((head, tail)) = n.rsplit_once(" in ") {
            if !head.trim().is_empty() && !tail.trim().is_empty() && tail.split_whitespace().count() <= 4 {
                if let Some(dest) = self.find_place(tail, ctx)? {
                    return Ok((vec![head.trim().to_string()], Some(dest)));
                }
            }
        }
        Ok((names.to_vec(), None))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Normalize a path syntactically (no disk access): collapse . and ..,
    /// rejecting anything that climbs above the allowed root.
    fn normalize(&self, p: &Path) -> Result<PathBuf, PawError> {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::RootDir => out.push("/"),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return Err(PawError::UnsafePath(format!(
                            "{} climbs above the filesystem root",
                            p.display()
                        )));
                    }
                }
                Component::Normal(s) => out.push(s),
                Component::Prefix(_) => {
                    return Err(PawError::UnsafePath("drive prefixes are not Linux paths".into()))
                }
            }
        }
        Ok(out)
    }

    /// Symlink-aware confinement: canonicalize the deepest existing ancestor
    /// and re-append the non-existing tail, so a symlinked parent cannot
    /// smuggle a write outside the root even for not-yet-existing children.
    fn real_confine(&self, p: &Path) -> Result<PathBuf, PawError> {
        let mut existing = p.to_path_buf();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            match (existing.parent().map(|x| x.to_path_buf()), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name.to_os_string());
                    existing = parent;
                }
                _ => {
                    return Err(PawError::UnsafePath(format!(
                        "no existing anchor for {}",
                        p.display()
                    )))
                }
            }
        }
        let canon =
            std::fs::canonicalize(&existing).map_err(|e| PawError::Validation(e.to_string()))?;
        let mut real = canon;
        for name in tail.iter().rev() {
            real.push(name);
        }
        self.check_under_root(&real)?;
        Ok(real)
    }

    /// No silent overwrites: until a deliberate overwrite feature exists,
    /// colliding destinations are refused outright.
    fn refuse_overwrite(&self, from: &Path, to: &Path) -> Result<(), PawError> {
        if to.exists() {
            Err(PawError::Validation(format!(
                "{} would overwrite existing {}; I won't overwrite things - rename or delete the existing one first",
                from.display(),
                to.display()
            )))
        } else {
            Ok(())
        }
    }

    fn check_under_root(&self, p: &Path) -> Result<(), PawError> {
        if p == self.root || p.starts_with(&self.root) {
            Ok(())
        } else {
            Err(PawError::UnsafePath(format!(
                "{} is outside your home area ({})",
                p.display(),
                self.root.display()
            )))
        }
    }

    fn resolve_location(
        &self,
        loc: &LocationRef,
        ctx: &ContextManager,
    ) -> Result<PathBuf, PawError> {
        let raw = match loc {
            LocationRef::Home => self.root.clone(),
            LocationRef::Desktop => self.root.join("Desktop"),
            LocationRef::Documents => self.root.join("Documents"),
            LocationRef::Downloads => self.root.join("Downloads"),
            LocationRef::Explicit(s) => {
                let expanded = if let Some(rest) = s.strip_prefix("~/") {
                    self.root.join(rest)
                } else if s.starts_with('/') {
                    PathBuf::from(s)
                } else {
                    self.root.join(s)
                };
                self.normalize(&expanded)?
            }
            LocationRef::LastCreated => ctx
                .last_created()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| PawError::UnknownReference("it — nothing created yet".into()))?,
            LocationRef::Named(name) => {
                let direct = ctx.resolve_named(name).or_else(|| {
                    // A name we have not touched this session may still exist on disk.
                    let cand = self.root.join(name);
                    if cand.exists() {
                        Some(cand)
                    } else {
                        None
                    }
                });
                let found = match direct {
                    Some(p) => Some(p),
                    None => self.find_place(name, ctx)?,
                };
                found.ok_or_else(|| {
                // Names are case-sensitive and never matched loosely. If a
                // differently-cased entry exists, say so instead of using it.
                let other = std::fs::read_dir(&self.root).ok().and_then(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .find(|n| n != name && n.to_lowercase() == name.to_lowercase())
                });
                match other {
                    Some(o) => PawError::UnknownReference(format!(
                        "{name}\" (names are case-sensitive; there is a different item called \"{o}\", say its exact name if you mean that one"
                    )),
                    None => PawError::UnknownReference(name.clone()),
                }
                })?
            }
            // Bare creates land at the root: predictable beats clever here.
            // Context-driven placement happens through "inside it"/names instead.
            LocationRef::CurrentDir => self.root.clone(),
        };
        let n = self.normalize(&raw)?;
        let n = self.real_confine(&n)?;
        if n.starts_with(self.root.join(crate::journal::STORE)) {
            return Err(PawError::UnsafePath("Pawline's own store is off limits".into()));
        }
        Ok(n)
    }

    fn apply_ext_hint(name: &str, kind: &CreateKind) -> String {
        if let CreateKind::File { ext_hint: Some(ext) } = kind {
            if Path::new(name).extension().is_none() {
                return format!("{name}.{ext}");
            }
        }
        name.to_string()
    }

    /// True when the wallpaper in place now can be restored later: Pawline set
    /// it before, a baseline was captured, or the running manager reveals it.
    fn wallpaper_backup_known(&self) -> bool {
        let text = std::fs::read_to_string(self.root.join(crate::journal::JOURNAL)).unwrap_or_default();
        crate::journal::last_open_of(&text, "wallpaper").is_some()
            || crate::journal::last_open_of(&text, "wallpaper_base").is_some()
            || crate::wallpaper::current_swaybg_image().is_some()
    }

    /// A rename, move or copy needs something to act on: it must exist on
    /// disk now (a dangling symlink does not count) or be made earlier in the
    /// same plan. Checked at planning time so a stale plan is refused before
    /// the confirmation card, not after.
    fn require_source(&self, from: &Path, will_exist: &HashSet<PathBuf>) -> Result<(), PawError> {
        let on_disk = from.exists();
        if on_disk || will_exist.contains(from) {
            Ok(())
        } else {
            Err(PawError::Validation(format!("{} doesn't exist", from.display())))
        }
    }

    pub fn build_plan(
        &self,
        directives: &[Directive],
        ctx: &ContextManager,
    ) -> Result<Plan, PawError> {
        self.hint_notes.borrow_mut().clear();
        let mut ops: Vec<Operation> = Vec::new();
        // Paths this plan itself creates, so later steps may build on them.
        let mut will_exist: HashSet<PathBuf> = HashSet::new();
        // Simulated context: later directives in the same message refer to
        // things this plan will create ("inside it", "inside pics").
        let mut sim = ctx.clone();

        for d in directives {
            match d {
                Directive::Create { kind, names, parent } => {
                    // "make a folder called a in models": if "models" is a real folder, that is where it goes
                    // (the plan asks for a yes). Nothing found: the whole text stays the name, as before.
                    let (names_split, base_split) = self.split_in_destination(names, parent, &sim)?;
                    let names = &names_split;
                    let base = match base_split {
                        Some(b) => b,
                        None => self.resolve_location(parent, &sim)?,
                    };
                    for n in names {
                        if n.contains('/') || n.contains('\\') {
                            return Err(PawError::UnsafePath(format!(
                                "\"{n}\" looks like a path, not a name"
                            )));
                        }
                        if n == "." || n == ".." {
                            return Err(PawError::UnsafePath(format!("bad name \"{n}\"")));
                        }
                        let name = Self::apply_ext_hint(n, kind);
                        let path = self.normalize(&base.join(&name))?;
                        let path = self.real_confine(&path)?;
                        // parent must exist on disk or earlier in this plan
                        let mut anc = path.parent().map(|p| p.to_path_buf());
                        while let Some(a) = anc {
                            if a.exists() || will_exist.contains(&a) || a == self.root {
                                break;
                            }
                            let next = a.parent().map(|p| p.to_path_buf());
                            if next.as_deref() == Some(a.as_path()) || next.is_none() {
                                return Err(PawError::Validation(format!(
                                    "no such place: {}",
                                    a.display()
                                )));
                            }
                            anc = next;
                        }
                        if path.exists() && !will_exist.contains(&path) {
                            return Err(PawError::Validation(format!(
                                "{} already exists",
                                path.display()
                            )));
                        }
                        match kind {
                            CreateKind::Dir => ops.push(Operation::CreateDir { path: path.clone() }),
                            CreateKind::File { .. } => {
                                ops.push(Operation::CreateFile { path: path.clone() })
                            }
                        }
                        sim.remember(&path);
                        will_exist.insert(path);
                    }
                }
                Directive::Delete { target } => {
                    let path = self.resolve_location(target, &sim)?;
                    if path == self.root {
                        return Err(PawError::UnsafePath(
                            "I will not delete your whole home directory".into(),
                        ));
                    }
                    ops.push(Operation::Delete { path });
                }
                Directive::Rename { target, new_name } => {
                    let from = self.resolve_location(target, &sim)?;
                    self.require_source(&from, &will_exist)?;
                    if new_name.contains('/') || new_name.is_empty() {
                        return Err(PawError::UnsafePath("a new name is a name, not a path".into()));
                    }
                    let to = from
                        .parent()
                        .ok_or_else(|| PawError::Validation("nothing to rename into".into()))?
                        .join(new_name);
                    let to = self.real_confine(&to)?;
                    self.refuse_overwrite(&from, &to)?;
                    sim.remember(&to.clone());
                    ops.push(Operation::Rename { from, to });
                }
                Directive::Move { target, dest } | Directive::Copy { target, dest } => {
                    let from = self.resolve_location(target, &sim)?;
                    let dest_dir = self.resolve_location(dest, &sim)?;
                    self.require_source(&from, &will_exist)?;
                    let name = from
                        .file_name()
                        .ok_or_else(|| PawError::Validation("can't move the root".into()))?;
                    let to = self.real_confine(&dest_dir.join(name))?;
                    self.refuse_overwrite(&from, &to)?;
                    if matches!(d, Directive::Move { .. }) {
                        ops.push(Operation::Move { from, to });
                    } else {
                        ops.push(Operation::Copy { from, to });
                    }
                }
                Directive::List { target } => {
                    let path = self.resolve_location(target, &sim)?;
                    ops.push(Operation::List { path });
                }
                Directive::Undo => {
                    let text = std::fs::read_to_string(self.root.join(crate::journal::JOURNAL))
                        .unwrap_or_default();
                    let e = crate::journal::last_open(&text).ok_or_else(|| {
                        PawError::Validation("there is nothing to undo yet".into())
                    })?;
                    if e.kind == "wallpaper" && e.b.is_none() {
                        return Err(PawError::Validation(
                            "I can't undo the last change: it was a wallpaper change and I couldn't see the previous wallpaper. Nothing was changed.".into(),
                        ));
                    }
                    ops.push(Operation::Undo { entry: e.id, summary: e.describe_undo() });
                }
                Directive::SetWallpaper { image, via, no_backup_ok } => {
                    let path = self.resolve_location(image, &sim)?;
                    let meta = std::fs::metadata(&path).map_err(|_| {
                        PawError::Validation(format!("no such image: {}", path.display()))
                    })?;
                    if !meta.is_file() {
                        return Err(PawError::Validation(format!(
                            "{} is not a picture file",
                            path.display()
                        )));
                    }
                    if !crate::wallpaper::extension_ok(&path) {
                        return Err(PawError::Validation(format!(
                            "{} is not a picture I can use (png, jpg, webp, bmp or gif)",
                            path.display()
                        )));
                    }
                    let via = match via {
                        Some(n) => Some(crate::wallpaper::Adapter::from_name(n).ok_or_else(|| {
                            PawError::Validation(format!(
                                "I don't know a wallpaper tool called \"{n}\" (try awww, swaybg or plasma)"
                            ))
                        })?),
                        None => None,
                    };
                    let known = self.wallpaper_backup_known();
                    if !known && !*no_backup_ok {
                        return Err(PawError::Validation(
                            "I can't see your current wallpaper, so I can't back it up and Undo couldn't restore it. Nothing was changed. To go ahead anyway, say the same thing again ending with \"anyway\"."
                                .into(),
                        ));
                    }
                    ops.push(Operation::SetWallpaper { image: path, via, no_backup_ok: *no_backup_ok && !known });
                }
                Directive::OpenApp { name } => {
                    let c = match &self.app_env {
                        Some((dirs, path)) => crate::apps::resolve_in(name, dirs, path)?,
                        None => crate::apps::resolve(name)?,
                    };
                    ops.push(Operation::OpenApp { name: c.name.clone(), argv: c.argv });
                }
                Directive::GenerateImage { provider, prompt } => ops.push(Operation::GenerateImage {
                    provider: *provider,
                    prompt: crate::imagegen::clean_prompt(prompt)?,
                }),
                Directive::Answer { text } => ops.push(Operation::Answer { text: text.clone() }),
                Directive::Search { engine, query } => ops.push(Operation::Search {
                    engine: *engine,
                    query: query.clone(),
                }),
            }
        }

        let mut reasons: Vec<String> = ops
            .iter()
            .filter(|o| o.needs_confirmation())
            .map(|o| o.describe())
            .collect();
        for n in self.hint_notes.borrow_mut().drain(..) {
            reasons.push(format!("{n} Check the path before saying yes."));
        }
        let mut notes = Vec::new();
        if ops.iter().any(|o| matches!(o, Operation::SetWallpaper { no_backup_ok: true, .. })) {
            notes.push(
                "You chose to go ahead without a backup: I can't see your current wallpaper, so Undo won't be able to put it back. It can only return to wallpapers I set from now on."
                    .to_string(),
            );
        }
        Ok(Plan {
            notes,
            assist_derived: false,
            needs_confirmation: !reasons.is_empty(),
            confirmation_reasons: reasons,
            ops,
        })
    }
}
