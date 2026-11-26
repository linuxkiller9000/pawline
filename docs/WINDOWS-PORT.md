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

## Item 8 findings (cross-compile only, nothing run on Windows)
- `platform::detach` now sets CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS on Windows (compiles; unrun).
- File identity: cap-std exposes volume serial/file index only behind nightly cfgs, so it stays "cannot tell" (callers refuse) until a handle-based GetFileInformationByHandle function replaces the Metadata-based one.
- No Wine and no Windows machine in the dev sandbox, so no Windows behaviour is verified.

## Item 8 window backend (cross-compiles and links; NEVER RUN on Windows)
- `pawline-native` now builds for x86_64-pc-windows-gnu and links to a real PE32+ exe (debug, ~125 MB, console subsystem). Wayland/X11 code is cfg(unix); smithay and x11rb are unix-only deps; calloop is a direct dep.
- `winb.rs`: layered always-on-top popup, per-pixel alpha via UpdateLayeredWindow, window thread with its own message loop, mouse/wheel/keyboard (WM_KEYDOWN for named keys and Ctrl chords, WM_CHAR for typed Unicode text), clipboard (CF_UNICODETEXT), work area from SPI_GETWORKAREA (primary monitor only, not updated live). Same `Xb`/`XEv` API as x11.rs, so the shared drawing and key handling is the Linux code. `keys.rs` supplies Keysym/KeyEvent stand-ins.
- Visibly unavailable on Windows: panel control socket (--ctl, widgets), embedded Needle assist (Cactus runtime is Linux-only; stubs report unavailable), moving files, saving new files, file identity (undo), swaybg handling. Voice recording code compiles but no Windows recorder/STT command is configured.
- Not verified at all: that the window appears, takes focus, renders correctly, handles IME/DPI/multi-monitor, or that confirm cards behave. Readiness gate: a real Windows run.

## Item 8 adapters (cross-compiled, unrun)
- App launching: Windows finds only an exact `name.exe` / `name.com` on PATH (`;` separated). `.bat`/`.cmd` are refused on purpose (they go through the command interpreter), as are names with path separators or drive prefixes. No Start Menu (.lnk) reading yet, so apps not on PATH cannot be opened by name. Tested on Linux through the pure `which_with` helper only.
- Wallpaper: `SystemParametersInfoW(SPI_SETDESKWALLPAPER)` after `windows_check` (absolute, existing ordinary file, bmp/jpg/png); naming a Linux tool is refused; `SPI_GETDESKWALLPAPER` supplies the previous picture for undo. `windows_check` is tested on Linux; the system calls never ran.
- Unavailable on Windows by design (refuse): move, new-file save, symlink copy, undo fingerprints (file identity), panel socket, embedded assist, swaybg. These refusals are plain cfg stubs; they have no Windows-run tests.

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

## Item 22 source-prep slice 1 (nothing run on Windows)
- Windows has no HOME. Five places read HOME directly (work root, data dir, Pictures folder, voice temp dir, app launch cwd); on Windows the work root would have failed to start. They now use `platform::home_dir()` (HOME, then USERPROFILE on Windows only) and `platform::data_dir_from` (PAWLINE_DATA, XDG_DATA_HOME, then %LOCALAPPDATA%\pawline on Windows, else ~/.local/share/pawline). Pure-function unit tests run on Linux (structural; the real Windows environment is untested). `apps::default_dirs` for Windows still has no Start Menu discovery.
- Remaining Windows source gaps (unimplemented, unblocked for design, blocked for runtime proof): file identity via GetFileInformationByHandle, MoveFileExW no-replace move, reserved names (CON, NUL) and `\` path handling in intent/validate, case-insensitive name collisions, per-user ACLs for private dirs, Start Menu .lnk discovery, a Windows recorder/STT/TTS, DPI/IME/multi-monitor.
