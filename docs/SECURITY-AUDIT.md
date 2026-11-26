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

### Slice 3: memmap2 RUSTSEC-2026-0186 assessment (7:51)
- Chain: memmap2 0.8.0 <- xkbcommon 0.7.0 <- smithay-client-toolkit 0.19.2 <- pawline-native (Wayland path only). grep-searcher uses memmap2 0.9.12 (patched).
- Affected functions per the advisory: advise_range, unchecked_advise_range, flush_range, flush_async_range. A source grep of smithay-client-toolkit 0.19, xkbcommon 0.7 and Pawline's own crates (sandbox cargo registry copies) found NO call to any of them. So the unsound functions are not called by this dependency chain as built. That is a source grep, not a proof for every transitive path, and the 0.8.0 crate stays in the tree.
- Upgrade attempt: cargo update --precise 0.9.11 is impossible (xkbcommon 0.7 requires ^0.8). Bumping smithay-client-toolkit to 0.21.1 (xkbcommon 0.8, memmap2 0.9.12) FAILS to compile: the delegate_* macros moved, KeyboardHandler gained repeat_key and a changed update_modifiers signature, and the dispatch bounds for keyboard/pointer/data device changed (main.rs ~25, 2216-2250). It is a real API migration of the Wayland code. Not applied: it cannot be runtime-verified here (no Wayland compositor in the sandbox), and unverified input-handling changes are riskier than a non-called function. Status: unfixed, assessed not-called, upgrade needs a Wayland runtime check. rustybuzz and ttf-parser unmaintained warnings also remain.

### Slice 4: toolkit migration on an ISOLATED branch (successor/sctk-migration, not merged anywhere)
- smithay-client-toolkit 0.19.2 -> 0.21.1 (xkbcommon 0.8, memmap2 0.9.12) and calloop 0.13 -> 0.14. Code changes in main.rs: the ten delegate_* macros became one `delegate_dispatch2!(App)`; KeyboardHandler got `repeat_key` (same handling as a press, because 0.19 delivered repeats through press_key) and `update_modifiers` gained the raw-modifiers argument.
- cargo audit after: 162 crates, 0 vulnerabilities, 2 warnings (rustybuzz, ttf-parser unmaintained). RUSTSEC-2026-0186 (memmap2) is gone from the lock file.
- Evidence: workspace tests and Windows build via safe_ship; headless Weston 9 smoke (tools/wayland_smoke.sh, docs/WAYLAND-SMOKE.md) shows startup, surface and redraws; X11 under Xvfb still draws and takes typing. NOT verified: any Wayland key or pointer input (weston 9 cannot inject it), key repeat behaviour, clipboard/data device, layer-shell on a real compositor (weston has none; it fell back to a window), KDE, niri.

## Integration of the sctk 0.21 migration (successor line, 8:17)
Accepted by main on sway-only regression evidence. Successor line = baseline 97ba9a7 + migration. Rollback reference: branch successor/work-20261126-item23e-repeat (97ba9a7, sctk 0.19.2). Remote main untouched, no release. Unverified: KDE, niri, XFCE, orb drag, paste, fractional scale, GPU, multi-monitor. Two unmaintained warnings (rustybuzz, ttf-parser) remain; rustybuzz 0.20.1 RUSTSEC-2026-0206 and ttf-parser 0.25.1 RUSTSEC-2026-0192, 0 vulnerabilities, 162 crates scanned.


## 2026-11-26 item 21 follow-up: HTTP limits, voice commands, parser fuzz (author: successor agent)

### HTTP redirect / size-limit review
- search.rs get_json_within had NO explicit body limit (ureq 3.5.2 default 10 MB) and followed ureq's default of
  10 redirects. Fixed: explicit 2 MB limit (MAX_SEARCH_BYTES; real JSON replies are kilobytes) and max_redirects(3).
- imagegen.rs UreqHttp (weather, pictures, decks, questions) already limited bodies to MAX_IMAGE_BYTES (8 MB);
  added the same max_redirects(3). Redirect targets are not host-revalidated; the residual risk (a compromised
  API redirecting to an unexpected host) is documented here, accepted for now because requests carry no
  credentials or cookies and every online route is consent-carded first.
- Behavioural tests (real localhost sockets, author-written): an_oversized_reply_is_refused_instead_of_filling_memory,
  a_long_redirect_chain_is_refused, one_redirect_is_followed, a_server_that_never_answers_times_out_instead_of_hanging.

### voice.rs custom-command audit (PAWLINE_REC_CMD / PAWLINE_STT_CMD / PAWLINE_TTS_CMD)
- Custom commands are split on whitespace and exec'd directly (Command::new + args); NO shell anywhere: no injection.
- The recorder WAV is created private (0600) with create_new before the recorder opens it; predictable temp names
  are safe under a sticky /tmp and exclusive create.
- Speech text reaches the TTS command on stdin, never through argument interpolation.
- STT capture is bounded (64 KB) and cancelled jobs kill the whole process group.
- Custom routes never claim local handling in labels (enforced by tests status_labels_claim_local_only_for_the_built_in_route
  and the mic-notice tests). Residual, by design: custom commands run with the user's own privileges - they are the
  user's own commands, and the UI says "custom, unchecked".

### Parser fuzzing -> real DoS found and FIXED
- New deterministic fuzz test (intent::fuzz_tests::fuzz_parse_never_panics_or_wedges, 2500 mutated/random inputs
  plus 10KB/100KB/500KB monsters, behavioural) immediately found a wedge: "delete "x14286 (100 KB) did not return
  within 90 s. Profiling (pawline-core/examples/probe.rs) showed clean O(n^2): 2k tokens 2.35s, 4k tokens 9.42s,
  80KB of unknown words 77s.
- Root cause: split_clauses called to_ascii_lowercase() on the WHOLE remaining suffix at every byte position, and
  parse_sentence lowercased the whole remainder per location-prefix iteration. Fix: starts_with_ci compares only
  the separator's bytes. Result: 4k tokens 9.42s -> 8.3ms; 80KB 77s -> 24ms; linear scaling verified by probe.
- The fuzz test now passes in 0.22s and guards the regression. Probe kept at pawline-core/examples/probe.rs.

### 2026-11-26: cactus runtime is a process-global session -> SIGSEGV under concurrent use, FIXED
- Found via `cargo test -p pawline-native -- --test-threads 4` SIGSEGVing (2/2 runs) right after the bridge
  model-touching tests. Evidence chain: strace showed one thread munmapping the 35,336,192-byte weights arena
  while another faulted INSIDE that range (SEGV_MAPERR); a scratch probe proved a second cactus_load in one
  process munmaps the first session's arena and remaps; a concurrent probe showed one thread's complete()
  failing with "cactus_init not called" after another thread's load replaced the global session; a third run
  aborted with "Rust cannot catch foreign exceptions" (a C++ exception escaping the FFI boundary under corruption).
- Root cause: the cactus C API is a single global session (cactus_complete/cactus_reset take no handle; a second
  cactus_load frees the previous arena). Three bridge tests (editing_the_draft, every_online_ask,
  typo_suggestion_never) lazy-load the embedded model concurrently under parallel test threads.
- App-level exposure, not just tests: TimedAssist abandons a hung worker after its deadline (by design the app
  never waits on the stuck call), and a later fresh assist (e.g. after a new conversation) would have called
  cactus_load again, freeing the arena under the abandoned worker's in-flight inference.
- Fix (pawline-needle/src/lib.rs): every FFI call is serialized on a process-wide mutex held via TRY_LOCK
  (busy is a graceful "the local model is busy with another request" error, never a wait, so a hung model still
  cannot wedge the app), the weights load at most once per process (no more arena free), and a reused session is
  reset first so no earlier conversation leaks into a new one. bridge.rs: a transient busy no longer burns the
  one lazy-load attempt; a genuinely failed load still does (no 1.4s retry on every parser gap).
- Regression tests (pawline-needle/tests/concurrent_sessions.rs, real model, behavioural):
  concurrent_sessions_are_serialized_and_neither_sees_an_uninitialized_model (3 threads load+complete, retrying
  transient busy like a later request would; pre-fix this produced "cactus_init not called" failures) and
  a_second_session_reuses_the_weights_and_starts_clean (strace-verified: no munmap of the arena on the second
  session start).
- Verification: `cargo test -p pawline-native -- --test-threads 4` green 4/4 after the fix (was 2/2 SIGSEGV
  before, same invocation); needle concurrent tests green; full suite green at default and single thread counts.
  The crash was timing-dependent, so absence over 4 runs is evidence, not proof; the mutex removes the mechanism.
- Residual: cactus could still throw a foreign exception across the FFI boundary on an internal error (Rust
  cannot catch those; the process would abort). Not observed on any non-corrupted path; corrupt weights are
  refused at load (item 16 subprocess tests) and the embedded weights are sha256-checked at build time.

### Caveats carried
- This review is by the author side, not an independent reviewer; an independent security review is still owed.
- License compatibility wording in docs/LICENSE-AUDIT.md is a developer reading, not legal advice.
