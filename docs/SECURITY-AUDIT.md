# Pawline security audit (item 21), slice 1: source review at the head named in the report

This is a tested-scope audit by the author of the code (not independent). It lists what was checked and what was not. It is not a safety claim.

## 1. Processes started (source review: `Command::new` call sites)
- shrink.rs: ffmpeg/ffprobe with an argument list (no shell), stderr discarded, input and output are files under the home.
- wallpaper.rs: a fixed list of wallpaper programs, image path is Pawline's private copy.
- apps.rs: app launch from the installed-app list by exact name, argv list (no shell), cwd HOME, only after a confirm card.
- dirs.rs: `zoxide query --list --score`, read only.
- voice.rs: user-configured STT/REC/TTS commands run by design (uncontrolled path, stated in the disclosure).
- main.rs: re-launch of its own executable for panel-only mode.
- Tests (bridge.rs) run ffmpeg/ffprobe on generated files only.
- No call site goes through `sh -c` in this list (grep of `Command::new`; voice.rs custom commands were not re-audited in this slice).

## 2. Network requests (source review)
Hosts present in source: text.pollinations.ai, image.pollinations.ai, aihorde.net, api.duckduckgo.com, en.wikipedia.org, open-meteo.com plus geocoding-api.open-meteo.com. Each call site checks the software offline gate (`netgate::check`, 19 references) or sits behind a disclosure card. The offline gate is software, not a firewall.
- FINDING (fixed in this slice): search.rs `get_json` used ureq's default agent with no global timeout, so a server that accepts and never answers could block the engine thread. Now a 20 s global timeout. Behavioural test with a real silent local socket (1 s timeout, must return error in under 3 s).
- Not checked: redirect behaviour of the search calls to non-HTTPS hosts, response size limits beyond ureq defaults, certificate handling beyond webpki-roots defaults.

## 3. Dependencies and licenses (cargo metadata, 163 packages, sandbox resolve, no network audit)
- Every package declares a license. Counts: MIT/Apache family dominate; BSD-2/3, ISC, Zlib, Unlicense, 0BSD, Unicode-3.0, one CDLA-Permissive-2.0 (webpki-roots).
- GPL-3.0-only appears only on Pawline's own two crates (pawline-native, pawline-needle-native); pawline-core and pawline-needle are Apache-2.0. Consistent with the repo LICENSE as far as read; a lawyer-level compatibility review was NOT done.
- NOT done: RustSec advisory scan (no cargo-audit available offline), license of the Needle model weights and any bundled assets/fonts (needs verification before release).

## 4. Model output cannot execute code (existing evidence)
- Assist proposals are parsed into the same Directive types as typed text, then validated and shown on a confirm card; empty proposals and panics fail safe (16a/16b tests, behavioural). Typing never approves a card.
- Not done: fuzzing the tool-call parser.

## 5. Accidental logging of private data (source review)
- eprintln/println sites: PERF timing (opt-in env), x11 diagnostics, terminal mode output, test messages. None prints file names or message text, except terminal mode which prints replies to the user's own terminal by design and the `xev` debug line in main.rs. FINDING (fixed): with PAWLINE_DEBUG set that line printed the key symbol of every key press, i.e. a typed-text log (could include a pasted or typed secret). It now prints "key (symbol not logged)". The x11.rs `PAWLINE_DEBUG` lines print sizes only.
- Not done: runtime log capture with a canary string.

## 6. File operations and permission boundaries
- Existing evidence: cap-std confinement, fs_stress.rs (9+ behavioural tests), recovery tests (15a). Open: FIFO copy/rename/read, devices, Windows, root, TOCTOU under threads (item 17 gap list).

## 7. Slice 2: advisory scan and runtime canary (7:36, sandbox)
- Advisory scan: access was NOT blocked. Network works from the sandbox; cargo-audit 0.23.0 was built locally (cargo install --locked) and run against a fresh clone of github.com/RustSec/advisory-db (commit b9b8a3c2, 2026-11-25, 1568 advisories) on Cargo.lock (163 crates). Result: 0 vulnerabilities, 3 warnings: rustybuzz 0.20.1 unmaintained (RUSTSEC-2026-0206), ttf-parser 0.25.1 unmaintained (RUSTSEC-2026-0192), memmap2 0.8.0 unsound pointer offset (RUSTSEC-2026-0186, fix >=0.9.11; pulled in by xkbcommon 0.7.0 <- smithay-client-toolkit 0.19.2, the Wayland path). Not yet evaluated whether the unsound memmap2 code path is reachable, and not upgraded (needs a smithay-client-toolkit bump; unimplemented, not blocked).
- Runtime canary (release build, Xvfb, PAWLINE_DEBUG=1 and PAWLINE_PERF=1 on): typed "make a folder called zqcanaryxj", confirmed it, then typed "hello zqcanaryxj secretpw9". Captured stderr (289 lines): 0 occurrences of either canary. Data dir: the chat text "secretpw9" appears in no file (history off). The folder name is stored in frequent-folders.json and as a step line "Create folder /tmp/.../zqcanaryxj" in task-memory.json, which is the documented visible, deletable steps-only memory. Journald/syslog and the X server were not inspected. Single run, one canary pair, author-written.
