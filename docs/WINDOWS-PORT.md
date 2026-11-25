# Windows port: status and plan (not a working Windows build)

Status at this commit (cross-compile checks only, nothing has run on Windows):
- `pawline-core`, `pawline-needle` and `pawline-needle-native` COMPILE for `x86_64-pc-windows-gnu` (library code).
  Unix-specific code now sits behind `pawline-core/src/platform.rs`. The Unix implementation is the tested one (342 Linux
  tests pass). The non-Unix versions are scaffolding that REFUSE: no file identity (so undo and copy cleanup cannot
  verify files and refuse), no move/rename (`rename_noreplace` returns an error), no symlink copy, no shrink (needs /dev/fd),
  no app launching or wallpaper adapters that work. This is compile scope, not Windows capability.
- `pawline-native` (the window and the whole UI) does NOT compile for Windows: it is built on Wayland (smithay-client-toolkit)
  and X11 (x11rb) inside a 1,900-line main.rs that mixes drawing and windowing. A Windows window backend plus separating
  the tiny-skia renderer from the window code is the next big piece. Core integration tests (tests/wallpaper.rs) still use Unix APIs.
- Cross toolchain in the dev sandbox: rust-std for the target plus mingw-w64 gcc 10 (Ubuntu pool debs). A cross-compiled exe would still not be a tested product.

Check commands: `cargo check -p pawline-core --target x86_64-pc-windows-gnu`; `cargo check -p pawline-native --target x86_64-pc-windows-gnu` (fails at wayland-sys today).

## What the Unix-only code was (all now behind platform.rs, refusing on Windows)
| File | What | Windows plan |
|---|---|---|
| exec.rs | dev/ino file identity (undo fingerprints, temp-file identity, copy cleanup) | one `platform::file_id(&File)` (Windows: volume serial + file index via GetFileInformationByHandle) |
| exec.rs | `renameat2(RENAME_NOREPLACE)` | `MoveFileExW` without MOVEFILE_REPLACE_EXISTING fails if the target exists. Needs a test on real Windows |
| exec.rs | `/dev/fd/0,1` handles for ffmpeg/ffprobe | no /dev/fd on Windows. Needs another way to bind ffmpeg to the checked files (inherited handles or a verified path), then re-verify the race story |
| exec.rs, dirs, planmem, searchmem | file modes (0600/0700), `DirBuilderExt::mode` | private dirs via ACL defaults under %LOCALAPPDATA%, no mode bits |
| exec.rs | `Dir::symlink` | symlink creation needs privilege on Windows. Copy of symlinks should be refused or skipped there |
| apps.rs | .desktop launcher discovery, `process_group` | Start Menu shortcuts (.lnk) and App Paths, CREATE_NEW_PROCESS_GROUP |
| wallpaper.rs | Linux adapters (gsettings, feh, ...) | SystemParametersInfoW(SPI_SETDESKWALLPAPER) adapter |
| finder.rs | libc O_NONBLOCK reads, descriptor-relative walk | cap-std walk works, non-blocking read guard needs a Windows version |
| shrink.rs | exec bit check in find_exe | `.exe` lookup in PATH |
| pawline-native | X11/Wayland window code | needs its own Windows window backend (separate work) |
| home area | `~` root | USERPROFILE, case-insensitive names, reserved names (CON, NUL), `\` paths, drive letters |

## Order of work
1. Introduce `platform` module with the primitives above (unix implementation = current code, moved, tests unchanged).
2. Make core compile for the Windows target with Windows stubs that FAIL CLOSED (refuse the operation) until each primitive has a real implementation and a real-machine test.
3. Window backend. 4. Real Windows test machine (not available yet). No release before step 4.
