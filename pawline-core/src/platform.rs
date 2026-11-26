//! Everything that differs by operating system lives here. The Linux/Unix code is the tested implementation.
//! The Windows versions are SCAFFOLDING: they compile, and where a safety property cannot be provided yet they
//! REFUSE (return None / an error) instead of guessing. Nothing here has run on Windows.
use crate::error::PawError;
use cap_std::fs::Dir;
use std::path::Path;

/// (device, file index) identity of an open file or a path. None means "cannot tell": callers must then refuse.
pub type FileId = (u64, u64);

/// True on platforms where this module has a real implementation of every safety primitive.
pub const FULLY_SUPPORTED: bool = cfg!(unix);

#[cfg(unix)]
pub fn id_cap(m: &cap_std::fs::Metadata) -> Option<FileId> {
    use cap_std::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}
#[cfg(unix)]
pub fn id_std(m: &std::fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}
#[cfg(not(unix))]
pub fn id_cap(_m: &cap_std::fs::Metadata) -> Option<FileId> {
    // Windows: cap-std only exposes volume serial/file index behind nightly cfgs (windows_by_handle), so on stable Rust
    // this needs our own GetFileInformationByHandle on an open handle (a different signature than Metadata). Until
    // then: "cannot tell", and callers refuse.
    None
}
#[cfg(not(unix))]
pub fn id_std(_m: &std::fs::Metadata) -> Option<FileId> {
    None
}

/// Is this file marked executable (Unix mode bits). On Windows executability is by extension, handled by the caller.
#[cfg(unix)]
pub fn is_executable(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.is_file() && m.permissions().mode() & 0o111 != 0
}
#[cfg(not(unix))]
pub fn is_executable(m: &std::fs::Metadata) -> bool {
    m.is_file()
}

/// Owner-only permissions on one of Pawline's own data files. Windows: not done yet (relies on the profile folder ACL).
#[cfg(unix)]
pub fn set_private_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
pub fn set_private_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Create the (recursive) private directory 0700 through the confined root.
#[cfg(unix)]
pub fn create_private_dir(root: &Dir, rel: &str) -> std::io::Result<()> {
    use cap_std::fs::{DirBuilder, DirBuilderExt};
    let mut b = DirBuilder::new();
    b.recursive(true).mode(0o700);
    root.create_dir_with(rel, &b)
}
#[cfg(not(unix))]
pub fn create_private_dir(root: &Dir, rel: &str) -> std::io::Result<()> {
    root.create_dir_all(rel)
}

/// Mark open options as owner-only (0600) for a newly created file.
#[cfg(unix)]
pub fn private_mode(o: &mut cap_std::fs::OpenOptions) {
    use cap_std::fs::OpenOptionsExt;
    o.mode(0o600);
}
#[cfg(not(unix))]
pub fn private_mode(_o: &mut cap_std::fs::OpenOptions) {}

/// Do not block when opening a possible pipe/device (finder reads).
#[cfg(unix)]
pub fn nonblocking_read(o: &mut cap_std::fs::OpenOptions) {
    use cap_std::fs::OpenOptionsExt;
    o.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
}
#[cfg(not(unix))]
pub fn nonblocking_read(_o: &mut cap_std::fs::OpenOptions) {}

/// Start a child in its own process group so it outlives and is separate from this process.
#[cfg(unix)]
pub fn detach(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}
#[cfg(windows)]
pub fn detach(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS. Not run on Windows yet.
    cmd.creation_flags(0x0000_0200 | 0x0000_0008);
}
#[cfg(not(any(unix, windows)))]
pub fn detach(_cmd: &mut std::process::Command) {}

/// Symbolic link creation inside the confined root.
#[cfg(unix)]
pub fn symlink(dir: &Dir, target: &Path, to: &Path) -> Result<(), PawError> {
    Ok(dir.symlink(target, to)?)
}
#[cfg(not(unix))]
pub fn symlink(_dir: &Dir, _target: &Path, _to: &Path) -> Result<(), PawError> {
    Err(PawError::Exec("copying a symbolic link is not supported on this system, so I stopped".into()))
}

/// Does this directory entry belong to the current user (a /proc style check). Unix only.
#[cfg(unix)]
pub fn same_owner_as_self(p: &Path) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    let me = std::fs::metadata("/proc/self").ok()?.uid();
    Some(std::fs::metadata(p).map(|m| m.uid() == me).unwrap_or(false))
}
#[cfg(not(unix))]
pub fn same_owner_as_self(_p: &Path) -> Option<bool> {
    None
}

/// Rename that can never replace an existing destination: renameat2(RENAME_NOREPLACE) on directory handles opened
/// through the confined root (no path walk outside it). Unsupported kernels/filesystems fail closed.
#[cfg(unix)]
pub fn rename_noreplace(root: &Dir, from: &Path, to: &Path) -> Result<(), PawError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    fn split(p: &Path) -> Result<(PathBuf, std::ffi::CString), PawError> {
        let name = p.file_name().ok_or_else(|| PawError::Exec("bad path".into()))?;
        let c = std::ffi::CString::new(name.as_bytes()).map_err(|_| PawError::Exec("bad name".into()))?;
        Ok((p.parent().map(|x| x.to_path_buf()).unwrap_or_default(), c))
    }
    let open = |d: &Path| -> Result<Dir, PawError> {
        if d.as_os_str().is_empty() { Ok(root.try_clone()?) } else { Ok(root.open_dir(d)?) }
    };
    let (fd_dir, fname) = split(from)?;
    let (td_dir, tname) = split(to)?;
    let (fd, td) = (open(&fd_dir)?, open(&td_dir)?);
    // SAFETY: both fds are live for the call; the names are NUL-terminated and contain no '/'.
    let r = unsafe { libc::syscall(libc::SYS_renameat2, fd.as_raw_fd(), fname.as_ptr(), td.as_raw_fd(), tname.as_ptr(), 1u32) };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EEXIST) | Some(libc::ENOTEMPTY) => Err(PawError::ConfirmationRequired(format!("{} exists; not overwriting", to.display()))),
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => Err(PawError::Exec(
            "this system cannot move without risking an overwrite (no-replace rename unsupported); nothing was changed".into(),
        )),
        _ => Err(e.into()),
    }
}
#[cfg(not(unix))]
pub fn rename_noreplace(_root: &Dir, _from: &Path, _to: &Path) -> Result<(), PawError> {
    // TODO(windows): MoveFileExW without MOVEFILE_REPLACE_EXISTING on handles opened through the root, with a real-machine test.
    Err(PawError::Exec("moving files is not implemented on this system yet; nothing was changed".into()))
}

/// An unnamed new file (O_TMPFILE) in the folder `dir` (relative to the root, "" = the root itself), opened through the
/// confined root. It has no name until `link_anon`, so nothing can be swapped in under a temporary name.
/// Any failure (old kernel, a filesystem without O_TMPFILE) means "use a named temporary file instead".
#[cfg(unix)]
pub fn anon_file(root: &Dir, dir: &Path) -> std::io::Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let d = if dir.as_os_str().is_empty() { root.try_clone()? } else { root.open_dir(dir)? };
    // SAFETY: d is a live directory fd; "." is NUL-terminated; on success the new fd is owned by the File.
    let fd = unsafe { libc::openat(d.as_raw_fd(), b".\0".as_ptr() as *const libc::c_char, libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC, 0o666 as libc::c_uint) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}
#[cfg(not(unix))]
pub fn anon_file(_root: &Dir, _dir: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no unnamed files here"))
}

/// Give an unnamed file its name with linkat. Like the no-replace rename it can never replace an existing name.
#[cfg(unix)]
pub fn link_anon(root: &Dir, dir: &Path, file: &std::fs::File, name: &std::ffi::OsStr) -> Result<(), PawError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let d = if dir.as_os_str().is_empty() { root.try_clone()? } else { root.open_dir(dir)? };
    let cname = std::ffi::CString::new(name.as_bytes()).map_err(|_| PawError::Exec("bad name".into()))?;
    let proc_path = std::ffi::CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|_| PawError::Exec("bad fd".into()))?;
    // SAFETY: proc_path and cname are NUL-terminated; d is a live directory fd.
    let r = unsafe { libc::linkat(libc::AT_FDCWD, proc_path.as_ptr(), d.as_raw_fd(), cname.as_ptr(), libc::AT_SYMLINK_FOLLOW) };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EEXIST) => Err(PawError::ConfirmationRequired(format!("{} exists; not overwriting", Path::new(name).display()))),
        _ => Err(e.into()),
    }
}
#[cfg(not(unix))]
pub fn link_anon(_root: &Dir, _dir: &Path, _file: &std::fs::File, _name: &std::ffi::OsStr) -> Result<(), PawError> {
    Err(PawError::Exec("saving new files is not implemented on this system yet; nothing was changed".into()))
}

/// The user's home folder from environment values. `windows` selects the Windows rule: USERPROFILE when HOME is
/// absent (Windows has no HOME by default). The Unix rule never looks at USERPROFILE.
pub fn home_from(get: &dyn Fn(&str) -> Option<std::ffi::OsString>, windows: bool) -> Option<std::path::PathBuf> {
    let pick = |k: &str| get(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    pick("HOME").or_else(|| if windows { pick("USERPROFILE") } else { None })
}

/// Home folder of the running process (HOME; on Windows also USERPROFILE).
pub fn home_dir() -> Option<std::path::PathBuf> {
    home_from(&|k| std::env::var_os(k), cfg!(windows))
}

/// Per-user data folder when PAWLINE_DATA is unset: XDG_DATA_HOME/pawline, else on Windows %LOCALAPPDATA%\pawline,
/// else ~/.local/share/pawline.
pub fn data_dir_from(get: &dyn Fn(&str) -> Option<std::ffi::OsString>, windows: bool) -> Option<std::path::PathBuf> {
    let pick = |k: &str| get(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if let Some(d) = pick("PAWLINE_DATA") {
        return Some(d);
    }
    if let Some(d) = pick("XDG_DATA_HOME") {
        return Some(d.join("pawline"));
    }
    if windows {
        if let Some(d) = pick("LOCALAPPDATA") {
            return Some(d.join("pawline"));
        }
    }
    home_from(get, windows).map(|h| h.join(".local/share/pawline"))
}

#[cfg(test)]
mod home_tests {
    use super::*;
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<std::ffi::OsString> + 'a {
        move |k| pairs.iter().find(|(a, _)| *a == k).map(|(_, v)| std::ffi::OsString::from(*v))
    }
    #[test]
    fn windows_falls_back_to_userprofile_and_unix_does_not() {
        let e = env(&[("USERPROFILE", "C:\\Users\\a")]);
        assert_eq!(home_from(&e, true), Some("C:\\Users\\a".into()));
        assert_eq!(home_from(&e, false), None);
        let e = env(&[("HOME", "/home/a"), ("USERPROFILE", "C:\\Users\\a")]);
        assert_eq!(home_from(&e, true), Some("/home/a".into()));
        assert_eq!(home_from(&env(&[("HOME", "")]), true), None);
    }
    #[test]
    fn data_dir_order() {
        let e = env(&[("PAWLINE_DATA", "/d"), ("XDG_DATA_HOME", "/x"), ("HOME", "/h")]);
        assert_eq!(data_dir_from(&e, false), Some("/d".into()));
        let e = env(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]);
        assert_eq!(data_dir_from(&e, false), Some("/x/pawline".into()));
        let e = env(&[("LOCALAPPDATA", "C:\\L"), ("USERPROFILE", "C:\\U")]);
        assert_eq!(data_dir_from(&e, true), Some(std::path::PathBuf::from("C:\\L").join("pawline")));
        assert_eq!(data_dir_from(&e, false), None);
        assert_eq!(data_dir_from(&env(&[("HOME", "/h")]), false), Some("/h/.local/share/pawline".into()));
    }
}

/// Why a file name cannot be created on Windows, or None. Pure rule: Windows device names (CON, PRN, AUX, NUL,
/// COM1-9, LPT1-9, with or without an extension), the characters < > : " | ? * and control characters, and names
/// ending in a dot or space. Enforced only when running on Windows (Linux allows all of these).
pub fn windows_name_problem(name: &str) -> Option<&'static str> {
    if name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 32) {
        return Some("contains a character Windows does not allow in names");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("ends with a dot or a space, which Windows drops");
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let dev = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0');
    if dev {
        return Some("is a reserved Windows device name");
    }
    None
}

#[cfg(test)]
mod winname_tests {
    use super::windows_name_problem as p;
    #[test]
    fn reserved_and_bad_names() {
        for bad in ["CON", "nul", "Aux.txt", "com1", "LPT9.log", "a:b", "x?", "end.", "sp ", "q\"r", "t\u{1}"] {
            assert!(p(bad).is_some(), "{bad}");
        }
        for ok in ["kitten paws", "console", "COM0", "com10", "notes.txt", "اسم", "a.b.c"] {
            assert!(p(ok).is_none(), "{ok}");
        }
    }
}
