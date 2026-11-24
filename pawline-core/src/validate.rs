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
}

impl Validator {
    pub fn new(root: PathBuf) -> Self {
        // The root itself may live behind a symlink; compare in canonical form.
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Validator { root }
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
            LocationRef::Named(name) => ctx.resolve_named(name).or_else(|| {
                // A name we have not touched this session may still exist on disk.
                let cand = self.root.join(name);
                if cand.exists() {
                    Some(cand)
                } else {
                    None
                }
            }).ok_or_else(|| PawError::UnknownReference(name.clone()))?,
            // Bare creates land at the root: predictable beats clever here.
            // Context-driven placement happens through "inside it"/names instead.
            LocationRef::CurrentDir => self.root.clone(),
        };
        let n = self.normalize(&raw)?;
        self.real_confine(&n)
    }

    fn apply_ext_hint(name: &str, kind: &CreateKind) -> String {
        if let CreateKind::File { ext_hint: Some(ext) } = kind {
            if Path::new(name).extension().is_none() {
                return format!("{name}.{ext}");
            }
        }
        name.to_string()
    }

    pub fn build_plan(
        &self,
        directives: &[Directive],
        ctx: &ContextManager,
    ) -> Result<Plan, PawError> {
        let mut ops: Vec<Operation> = Vec::new();
        // Paths this plan itself creates, so later steps may build on them.
        let mut will_exist: HashSet<PathBuf> = HashSet::new();
        // Simulated context: later directives in the same message refer to
        // things this plan will create ("inside it", "inside pics").
        let mut sim = ctx.clone();

        for d in directives {
            match d {
                Directive::Create { kind, names, parent } => {
                    let base = self.resolve_location(parent, &sim)?;
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
                Directive::OpenApp { name } => ops.push(Operation::OpenApp { name: name.clone() }),
                Directive::Answer { text } => ops.push(Operation::Answer { text: text.clone() }),
                Directive::Search { engine, query } => ops.push(Operation::Search {
                    engine: *engine,
                    query: query.clone(),
                }),
            }
        }

        let reasons: Vec<String> = ops
            .iter()
            .filter(|o| o.needs_confirmation())
            .map(|o| o.describe())
            .collect();
        Ok(Plan {
            assist_derived: false,
            needs_confirmation: !reasons.is_empty(),
            confirmation_reasons: reasons,
            ops,
        })
    }
}
