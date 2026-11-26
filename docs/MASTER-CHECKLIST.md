# Pawline master checklist

Source: the user's Telegram messages of 26 Nov 2026 (03:03 to 03:08, the checklist sections and the "final order" table). The numbers in the
priority table are the execution numbers. The section numbers the user wrote (17 to 28) are topic labels only; they are mapped below.
Every bullet is kept as written. A status line says what exists today. Built partial code is never counted as completing a bullet.
Publishing a release needs the user's separate approval (priority 29). No release, tag or CI is authorized.

Legend: DONE = done for the scope written, not hardware-verified. PARTIAL = some of it exists. OPEN = not started.
"Runtime gate" = needs hardware or a machine that is not available in the build sandbox, so it stays open until actually run there.

## Priority order (execution numbers)

| Priority | Work | Topic label in the user's sections | Status |
| --- | --- | --- | --- |
| 1 | GitHub source sync | (earlier list, old 1) | DONE: work is on feature branches, remote main untouched, no release |
| 2 | Core action safety | (old 2) | DONE for tested Linux file operations |
| 3 | Arabic/RTL display | (old 3) | DONE in the checked X11 build |
| 4 | Composer, folder picker, Markdown, basic themes | (old 4) | DONE |
| 5 | Voice implementation | (old 5) | DONE as code, virtual-audio tested only |
| 6 | Weather and Q&A | (old 6) | DONE in English |
| 7 | Basic presentations | (old 9) | DONE as a checkpoint |
| 8 | Typo tolerance | new | PARTIAL: command, weather, search and question words (docs/TYPOS.md) |
| 9 | Conversation, hints and jokes | new | DONE for session-only scope (accepted by main 3:35): offline chat, hints, jokes, opt-in online chat with exact-text card. Separate: persistent history (15/19), Arabic (11) |
| 10 | Presentation extras | new | PARTIAL: themes done (theme: line, 6 themes, contrast-tested, LibreOffice render checked); tables done (| a | b | lines, native table, limits 8x6, ragged/oversize refused, RTL cells, LibreOffice render checked); charts done (chart: bar|line|pie over a table, native chart parts with cached values, embedded .xlsx with the same numbers so Edit data has a sheet (workbook verified to open in LibreOffice only; PowerPoint Edit data NOT tested), value labels, bad numbers refused); local images done (image: <path> under a ## slide; PNG or JPEG, 5 MB cap, read through a home-confined handle, no .. or symlink out; text preview with path, type, pixels, size, NO thumbnail; one picture per slide, max 3 text lines, not on the title slide; AI-written drafts can never carry an image line; alt text = file name, not a description); source-backed fact-check support remains OPEN (source notes below are user-written labels, NOT fact-checking, NOT source-backed verification); source notes done (`source: <your text or link>` under a slide: shown on the slide as "Source noted by the author (not checked)" and in the review; Pawline never opens, fetches or verifies it; model-written drafts containing a source line are discarded; cannot be added by draft edit); NO automatic fact-checking exists; OOXML structural check: tools/ooxml_check.py (zip, XML well-formed, content types, rel targets, undeclared r:id, duplicate shape ids, embedded xlsx), all 12 sample decks pass and a deliberately broken deck fails; VALIDATION ONLY, NOT PowerPoint or Google Slides (external viewer check stays OPEN); draft editing done (edit slide N title to X | change slide N line M to X | delete slide N, on the in-memory reviewed draft only, typed text kept verbatim, outline-syntax text refused, old save card dropped, review shown again; GUI proof: live draft (Pollinations) then "change slide 3 title" then save card shows the edited title, nothing saved, Cancel focused; Contents slide edit refused; works only while a draft exists), fact-check support, non-LibreOffice check OPEN |
| 11 | Arabic functionality | new | PARTIAL (11a): fixed Arabic command shapes translated to visible English commands, spelling-variant folding for matching only, questions/negations never run, typo suggest-first on the command word, Arabic weather place lookup; limits and evidence quality in docs/ARABIC.md. Open: dialects, Arabic replies/cards, Arabic chat, voice, presentation commands |
| 12 | Voice/audio completion | new | OPEN (runtime gates). Audit in docs/VOICE.md: real sherpa+Moonshine STT run on 3 sample WAVs only; no mic, no TTS, no Arabic voice |
| 13 | Desktop polish and accessibility | section 25 + extras | OPEN |
| 14 | Privacy and permissions | section 17 | OPEN (parts exist, see below) |
| 15 | Failure recovery | section 18 | OPEN (parts exist) |
| 16 | AI reliability | section 19 | OPEN (parts exist) |
| 17 | Filesystem safety stress testing | section 20 | OPEN (parts exist) |
| 18 | Performance and resource management | section 21 | OPEN |
| 19 | Conversation management | section 22 | OPEN |
| 20 | Settings and customization | section 23 | OPEN |
| 21 | Security audit | section 26 | OPEN |
| 22 | Windows compatibility | old 8 | PARTIAL: builds, never run (runtime gate) |
| 23 | Linux desktop verification | old 7 | PARTIAL: XFCE and KDE X11 headless; niri and KDE Wayland open (runtime gate) |
| 24 | Update and maintenance | section 24 | OPEN |
| 25 | Documentation | section 27 | OPEN (drafted alongside) |
| 26 | Real-world testing | section 28 | OPEN (runtime gate: actual N5010) |
| 27 | Final integration | old 10 | OPEN |
| 28 | Packaging | old 10 | OPEN (no installer or signing is authorized) |
| 29 | Approval before release | | The user approves; nothing is published before that |

Carried from earlier scope (kept, not dropped): panel and multi-monitor limits (X11 work area read at place time; Windows primary monitor only),
Moonshine on the older CPU, real espeak intelligibility, Arabic voice and Arabic weather, the weather chooser using typed numbers instead of chips,
the voice-bar overlap in the chat window, presentation limits (no in-app editing, no images, charts, tables or themes yet, AI text not fact-checked,
not opened in PowerPoint), main-branch divergence reconciliation (belongs to priority 27).

## 13. Desktop polish and accessibility (user section 25: Accessibility, plus earlier extras)
Earlier extras: multiple monitors, reacting to panel changes, theme performance, reduced motion.
- Keyboard-only navigation.
- Screen-reader-friendly elements.
- Text scaling.
- Contrast options.
- Reduced-motion option.
- Focus indicators.
Status: OPEN. Existing: keyboard flow for cards (Cancel focused by default), focus ring on the card buttons. 13a (2026-11-26): computed WCAG contrast audit of the fixed text colours; hint/placeholder text was 2.6 to 4.3:1 on some theme surfaces and the disabled Do it label was 2.9:1, both raised to at least 4.5:1 and locked by a unit test (theme.rs contrast_tests: hint, note, body text on every theme gradient stop, input surface and card; body text on every user bubble). Computed from colour values, NOT a pixel measurement, NOT a user test; text over the animated gradient between stops and anti-aliasing not measured. 13b: text size setting (Settings row, 100/125/150 percent, saved as text= in ui-settings.txt, bad values fall back to 100; scales chat bubbles, markdown replies, input box, card text and scroll step). Pixel checks (Xvfb, debug): 125 percent help reply, 150 percent deck card top and scrolled end (picture strip, note and card text wrap, nothing clipped, Do it/Cancel still work); 100 percent help reply is pixel-identical (0 differing pixels) to the build before the change. Limits: Do it/Cancel labels, settings screen, hint lines and the window itself stay fixed size (buttons NOT scaled); not tested with Arabic/RTL text, long names, the folder chip or every screen at 150 percent; no user test. NOT done: high-contrast option, reduced-motion following a system setting (only the in-app Colours drift toggle, default on), screen-reader support (none, custom CPU-drawn window, no accessibility tree), multi-monitor, panel-change reaction, full keyboard-only pass.

## 14. Privacy and permissions (user section 17)
- Clear permission controls for microphone, files, internet, and AI.
- Show exactly what text will be sent online before sending.
- Ability to revoke permissions.
- No sending personal files or private information without explicit approval.
- Offline mode that genuinely disables network requests.
Status: OPEN. Existing: every online feature (questions, weather, search, presentation writing) shows a disclosure card with the text first and
sends nothing before Do it; secret-looking text is blocked with no approval path; files and chat history are never attached. 14a (2026-11-26): offline mode. pawline-core/src/netgate.rs is one gate called before every connection by the only two real senders (imagegen::UreqHttp get/post, which carries weather, pictures, decks and questions, and the search lookups). Settings has "Offline mode (no network at all)", default off, saved as offline= in ui-settings.txt, applied at start. While on, online requests never reach a card (message: nothing was sent), and the gate refuses any sender anyway. Tests: core netgate test (UreqHttp get/post and Web/Wiki search refused with the attempt counter unchanged, then one request passes after turning it off) and a bridge test (weather, web, wiki, question, picture and deck-topic requests get the offline message, no pending plan, no attempts; a local delete still shows its card). GUI pixels: off_on/off_weather/off_local. Limits: the gate only covers code that calls netgate::check, so new network code must call it (no OS-level block, not a firewall; subprocesses such as an opened app or a user-set STT/TTS command are not controlled); the old Tauri UI (pawline-ui) has no switch (gate default off); persisted-on-restart is covered by the settings round-trip only, not a restart pixel run. 14b (same day): label now says "Offline mode (Pawline's own online features)" with a visible note that it does not stop apps you open, and an extra visible note naming any configured PAWLINE_STT_CMD / PAWLINE_TTS_CMD / PAWLINE_REC_CMD that Pawline cannot control; help text says the same. Settings rows were made 28 px so all notes fit (pixels set_plain/set_custom). Test: a disclosure card pending when offline is switched on sends nothing at Do it (attempt counter unchanged, card consumed); a weather place list fetched while online followed by offline before the pick makes no forecast request and closes the chooser. An already-sent request cannot be undone by the switch. Still missing: a permission screen for microphone/files/internet/AI individually, per-permission revocation, the full privacy audit.

## 15. Failure recovery (user section 18)
- If an operation fails halfway through, recover gracefully.
- Don't leave partially created folders/files without explaining.
- Undo should survive application restarts where possible.
- Recover from crashes without losing conversation or pending tasks.
Status: OPEN. Existing: an on-disk undo journal with recovery notes at start-up; no-replace atomic publish; cancel cleanup of our own temp files.
Scope conflict to settle with the user: "recover conversation and pending tasks after a crash" needs conversation text on disk, while the current
designs promise memory only (chat lines, presentation drafts). Plan: off by default, an explicit setting to keep the conversation locally, accurate
disclosure of what is stored where, a delete control, and the draft/private text is never saved silently.

## 16. AI reliability (user section 19)
- Don't pretend to know something it doesn't.
- Distinguish verified facts from AI-generated answers.
- Ask for clarification when a command has multiple interpretations.
- Never let AI bypass the safety validator.
- Handle model failures, timeouts, and corrupted model data gracefully.
Status: OPEN. Existing: model/online text is labelled and shown as data; generated text cannot choose a save path or run a tool (tested); assist-derived
plans go through the same validator and confirmation; "Did you mean" and folder choosers for ambiguity; online failures give plain messages.

## 17. Filesystem safety stress testing (user section 20)
- Test paths containing Arabic, spaces, emojis, and unusual characters.
- Test symbolic links, permissions, read-only folders, and inaccessible locations.
- Test duplicate filenames and overwrite conflicts.
- Ensure it cannot accidentally escape its allowed directories.
- Test undo after files have been externally modified.
Status: OPEN. Existing: home confinement through handle-relative operations, no-replace publish, undo fingerprint checks; part of this is covered by tests, no full matrix yet. 2026-11-26: special-file handling for copy closed - copying a FIFO is refused at plan time (validator: only files and folders) and again at exec time before opening (a blocking open would hang; a swap between check and open is still possible, tracked as the TOCTOU gap); a FIFO inside a copied folder tree stops the copy with an error; moving a FIFO stays allowed (rename never opens content). Tests: validate::fifo_copy_tests, exec::copy_failure_tests::copying_a_fifo_is_refused_without_hanging (both unix-only; Windows special-file behaviour untested). Search/list/delete already skipped or handled FIFOs (finder and home tests).

## 18. Performance and resource management (user section 21)
- Measure RAM, CPU, startup time, idle usage, and response time.
- Avoid freezing the UI during AI inference or file operations.
- Stop unnecessary background processes.
- Handle low-memory situations gracefully.
- Test extended sessions to detect memory leaks.
Status: OPEN. No measured numbers exist yet; none will be claimed before they are measured (sandbox numbers are not the N5010's).

## 19. Conversation management (user section 22)
- New conversation / clear conversation.
- Conversation history persistence.
- Ability to forget or delete conversations.
- Context limits and handling of long conversations.
- Prevent old context from causing unintended actions in new conversations.
Status: OPEN, partly started in priority 9: a few recent lines are kept in memory only, "forget our chat" clears them, and conversation context never
creates an action (a polite request becomes a pick that still needs sending and a confirm card). Persistence needs the explicit controls above.

## 20. Settings and customization (user section 23)
- Proper settings screen.
- Adjustable voice, language, themes, animations, and AI behavior.
- Model management: download, remove, update, verify integrity.
- Restore default settings.
- Settings survive updates.
Status: OPEN. Existing: a small settings file (ui-settings.txt) and a theme choice; no model management or integrity verification yet.

## 21. Security audit (user section 26)
- Review every file operation and permission boundary.
- Review network requests.
- Check dependencies and their licenses.
- Ensure model output cannot execute arbitrary code.
- Check for accidental logging of private data.
Status: OPEN. The audit will report tested scope and residual limits, not a broad safety claim.

## 22. Windows compatibility (old 8)
Status: PARTIAL. A Windows executable builds and links (cross-compiled, debug); it has never run on Windows. See docs/WINDOWS-PORT.md. Runtime gate.

## 23. Linux desktop verification (old 7)
Status: PARTIAL. XFCE and KDE X11 (headless) checked. niri and KDE Wayland, the Wayland layer-shell path and a physical desktop are open. Runtime gate.
2026-11-26: orb drag verified on X11 (Xvfb, debug build): dragging the orb 200px/144px moved the window (1286,12 -> 1086,0, clamped at the top edge) and a click without movement still opened the card (68x68 orb -> 440x470 card); pixel proof od_after_c/oc_clickc. Wayland orb drag remains unverified. Paste into Pawline verified on X11 same day: xclip-seeded text landed in the input via Ctrl+V unsent, newline/tab became spaces (pixel proof ps_pastec/ps_nlc), and Ctrl+A Ctrl+C copied input text back to the clipboard (xclip read-back). Wayland paste path (clip_paste_wl) unverified.

## 24. Update and maintenance (user section 24)
- Version checking.
- Safe update mechanism.
- Preserve user settings and data during updates.
- Rollback if an update breaks the app.
- Clear changelog and version information.
Status: OPEN (design in docs/UPDATE-DESIGN.md, nothing built). No update server exists; any version check is a network request and must follow the permission rules in 14. No signing or release commitment.

## 25. Documentation (user section 27)
- Installation guide.
- Build instructions.
- Offline functionality explained.
- Supported platforms and known limitations.
- Troubleshooting guide.
- How to uninstall cleanly without leaving unwanted files.
Status: OPEN (PARTIAL draft in docs/USER-GUIDE-DRAFT.md), drafted alongside the features. The uninstall guide is written from a verified inventory of the files, folders, models and settings Pawline
really creates; user-created documents are kept by default.

## 26. Real-world testing (user section 28)
- Test normal usage, not just successful demo cases.
- Test incorrect commands and ambiguous requests.
- Test long sessions.
- Test with internet disconnected.
- Test after reboot.
- Test on your actual N5010, not just virtual machines.
Status: OPEN. Sandbox and virtual-machine runs are labelled as such and do not count as the N5010 test. Runtime gate. 2026-11-26: scenarios A1 (cold start), A2 (mixed session with confirms, undo, cancel path) and B (bad/ambiguous input) ran in the sandbox and passed; results and evidence in docs/REALWORLD-RESULTS.md. B found one performance note, FIXED same day: a 10k-char input burned ~30s in the local AI assist before the parser answered. Inputs over 1000 chars now skip the assist with a plain note (bridge.rs ASSIST_INPUT_MAX + assist_len_ok, boundary test + fast-reply test, live: 2.9s vs 33.7s before). Also found and FIXED same day (item 14): terminal mode ignored the persisted offline=on setting and let online requests reach a disclosure card; run_terminal now applies it like the GUI (live: weather gets the offline refusal, no card; local math unaffected; the disclosure card still gated everything even before the fix, so nothing was ever sent unapproved). D (offline/network loss) ran too: offline mode blocks with no card, a no-network namespace fails clean and fast after approval, control run with network works; mid-transfer drops not covered. C (long sessions: 120 exchanges with stable fds, 50 journaled creates with exact undo behaviour, 28 settings toggles round-trip clean) and E (reboot, sandbox-adapted: settings+history+offline persist across relaunch, seeded journal recovery settles both completed and not-completed intents) ran too. A real reboot and the whole N5010 section remain open (runtime gate).

## 27. Final integration, 28. Packaging, 29. Approval before release
OPEN. Integration also reconciles the diverged main branch (not merged without checking the repo authority for that operation). Packaging makes no installer,
signing or release commitment without approval. Publishing needs the user's explicit approval.


## Tracked gaps (file safety, item 17)
- Rename with quoted names: fixed 2026-11-26 (second fix). Remaining failing shapes had been: filler words before a quoted target (rename the folder "a b" to c: quotes stayed in the name, safe refusal) and a quoted target containing the separator (rename "x to y" to z: split inside the quotes, safe refusal). Both now parse correctly; parser tests plus an end-to-end terminal run renamed real folders on disk. Filler is stripped only when a quote follows, so a real folder named "folder x" is untouched. Unquoted and single/curly-quoted shapes already worked; delete/remove with quoted names works since 2026-11-26 (first fix).

## Tracked cross-item notes (from main, 3:35)
- 14 privacy: claims for the earlier GET routes (Q&A, deck draft, search) and the new POST chat route must be stated per route, not generalized from one. The privacy facts on the chat card come from pollinations.ai/privacy (2026-11-17) only.
- 18 performance: debug-build GUI typing latency is slow (long input redraw); measure and fix, no performance claim yet.
- 21/25: provider terms and privacy pages must be read in full before release; the 2026-11-26 read was a skim (no full legal review claimed).

### 14c microphone control (PARTIAL)
- Settings row "Microphone" (mic= key, default allowed). Blocked: the mic button records nothing and says so. One-time session notice before the first recording: private temp file, local transcription, deletion, text only goes into the box, custom STT/REC commands not controlled.
- File access: Pawline's own file actions are confined to the home folder and need a confirm card.
- Still open: per-permission screen for files/internet/AI, Tauri UI has no switches, full privacy audit, provider terms full read, real mic hardware.
- 14c follow-up: blocking the mic now also cancels an in-flight transcription (not only a recording). `voice::accept_transcript` drops stale, cancelled or mic-blocked transcripts (unit-tested; process-level cancel/kill tests already existed). Xvfb check with fake recorder/STT commands (labelled fixtures, not a real mic): blocked mid-transcription, draft stayed empty after the engine would have answered, no temp WAV left. The chat list now reserves room above the voice bar so the first-recording notice is not clipped.
- 14c label fix: listening/transcribing status labels claim "on this computer, not uploaded" only on the built-in route; a custom PAWLINE_REC_CMD / PAWLINE_STT_CMD shows "custom ..., unchecked" (voice::listening_label / working_label, unit-tested; Xvfb pixels checked, not clipped).

### 15 failure recovery, slice 1 (PARTIAL)
- New integration tests (real files in a temp home): undo of delete and of create-dir works after a simulated restart (new Engine + backend over the same home); a crash after the journal intent but before the change is reported at next start ("had not completed"), leaves no partial folder, and is settled once; a failed move into a read-only folder leaves the source, leaves nothing in the target, and leaves no dangling journal intent.
- Simulated restarts, not a real process kill. Rename/move/copy undo across restart not separately tested. Conversation/pending-task recovery after a crash is still NOT built (needs the opt-in local history decision above).
- 14c notice: the first-recording notice is now route-specific (voice::mic_notice). Local/deleted wording appears only for the built-in route; a custom REC/STT route says Pawline cannot check where that command sends or keeps audio or text. Unit-tested, Xvfb pixels checked (the sandbox has no real recorder, so the built-in-recorder notice has no screenshot).
- Process: tools/safe_ship.sh validates (tests exit and per-suite failures, Windows build) before any commit/push/bundle; refusal verified with failing harmless commands.

### 15 slice 2: opt-in conversation history (PARTIAL; off by default, no approval inferred)
- Settings row "Keep conversation on this computer" (history= key, default off). Nothing is written unless the user switches it on. Saved: typed lines and Pawline's plain replies only (history.txt, 0600, atomic temp+rename, max 100 lines, symlink not followed, oversize/corrupt files read as empty). Never saved: the unsent draft, confirm cards, pending tasks, errors, thumbnails, audio. Turning it off deletes the file; if the delete fails it stays ON and says so.
- On start with it ON, earlier lines are restored with a visible "Restored N earlier messages" note; pending tasks and confirm cards are not restored.
- 5 unit tests in history.rs (round trip incl. Arabic/newlines, privacy mode + no temp, cap/corrupt/oversize, symlink + idempotent delete, kind filter). Xvfb pixels: on, restart-restore, off-deletes (file gone) verified.
- Settings screen now has ONE theme row (Enter cycles) so the notes stay unclipped at the fixed 470 px window; checked at 100% and 150% with custom commands set.
- Windows: the file mode is only set on Unix; on Windows it relies on the profile folder permissions (build only, never run).
- Not done: this is chat text only, so "recover pending tasks after a crash" is still unbuilt. The data-dir file is not encrypted. The user has not chosen this feature on or off for themselves.
- Custom-route mic notice now says Pawline submits nothing to its chat until Enter but the command may already have sent audio/text elsewhere.
- 15 slice 2 polish: Settings text separates the settings file (switches only) from the optional conversation file. Disclosed: submitted messages (typed or from voice) and plain replies are kept, NOT encrypted, readable by anyone who can read the account's files, permissions restricted on Linux only and unverified on Windows, off deletes, never added to online chat context. Online chat context is built from bridge.rs per-session state; a structural test (not a behavioural proof) keeps history out of bridge.rs. Worst case (150% text, custom commands set) is unclipped in Xvfb pixels but only a few px from the panel edge: any new note needs a layout change.

### 16 AI reliability, slice 1 (PARTIAL)
- Found by a new test: an EMPTY assist proposal produced an empty confirm card. Fixed: an empty proposal is a failed proposal and the user gets the parser's plain explanation.
- A panicking assist is now caught in `Engine::plan_with` (std::panic::catch_unwind): same outcome as an error, nothing runs. Needs panic=unwind (the workspace does not set panic=abort).
- Test `model_error_empty_and_panic_fail_safe_and_the_engine_survives` (mock assists, real engine and backend on a temp home): error, empty, panic each give the parser explanation with no model detail leaked, create nothing, never execute, and the engine still works afterwards.
- Not covered: a model that HANGS (propose is synchronous inside the bridge thread; no timeout or cancel for it yet); corrupted weights (the weights are compiled into the binary, so there is no runtime file to corrupt; a load failure falls back to parser-only by code reading, untested); verified-vs-generated labelling audit; wrong-confident model output beyond the existing tests; real Needle model not run in these tests (mocks).
- 16 slice 2: `TimedAssist` (pawline-core/src/assist.rs) wraps the real assist in the bridge with a 30 s wait (untuned guess, not measured against real inference times). This RETURNS CONTROL; it does NOT STOP inference. A hung or slow native call keeps running (CPU and the model stay busy) until it finishes or the process ends. Bounded behaviour: after the first timeout or crash the assist is switched off for the session, so at most one abandoned call exists per session (test: 21 requests after a timeout, 1 model call). The late result goes to a dropped channel and is discarded; test shows no confirm card and nothing created, and the engine works normally afterwards. Mock assists only; the real Needle model was not made to hang. After a timeout the assist stays off until restart (no automatic retry, no user-visible "assist is off" notice yet).
- Behavioural history isolation test (bridge): with a history file present and loaded the way the UI loads it, the online chat confirm card and online_ctx contain none of its text. The structural bridge.rs check stays as a second guard.
- 16 slice 3 (label audit): reply sources and labels. Chat online: "AI-written, not checked, may be wrong". Ask online: "not checked, may be wrong". Wikipedia: "(online)". Web instant answer: "online, may be out of date". Presentation draft: "Draft written by an online AI model (unchecked, may contain mistakes)", nothing saved. Assist-derived plans: confirm card says "proposed by the AI assist" and always waits for a yes. FIXED a gap: picture results said only "Picture ready (Pollinations)"; they now say "AI-generated by <provider>, unchecked". Table test covers chat/ask/wiki/web labels (picture and deck paths are not covered by a unit test). Weather is raw provider data with its source link on the card. Local replies (math, help, small talk, file results) are Pawline's own and unlabelled by design. Not done: a user-facing "verified" marker (nothing is source-verified; Wikipedia/web are quoted, not checked), or labelling of restored history lines beyond the label text that was saved with them.
- Assist timeout is now visible: the next reply says the local AI assist is off until restart, that the answer shown is the parser's, and that the stuck work cannot be stopped from here and may still use CPU (shown once per session; test).
- The structural history/bridge test now splits at an explicit "END OF PRODUCTION CODE" marker comment in bridge.rs instead of the first #[cfg(test)] (and fails if the marker is removed).

### 17 filesystem stress, slice 1 (PARTIAL)
- BUG FIXED: `rename a to "new name.txt"` created a file whose name literally contained the quote characters. Quotes wrapped around the whole new name are now syntax (straight, single, backtick, curly); quotes inside a name are kept. Unit-tested in intent.rs.
- New matrix `pawline-core/tests/fs_stress.rs` on real temp homes (Linux, running as a normal user): rename + undo with spaces, Arabic, emoji, an apostrophe and double spaces; no overwrite on rename/copy/move onto an existing name; symlinks (directory and file) pointing outside the home cannot be deleted, renamed, moved, copied through, or used as a target, and absolute/dotted escapes are refused (refusal messages checked in a debug run, so the tests are not vacuous); read-only target folders fail with Permission denied and leave no temp or partial files; undo of a create does not delete a file that gained content, and undo of a rename does not overwrite a re-created old name.
- Not covered: Windows paths (reserved names, drive letters, long paths, case-insensitive clashes, junctions), Unicode normalisation clashes (NFC vs NFD), very long names / 255-byte limits, hard links, FIFOs/devices, race conditions (TOCTOU) between plan and confirm, huge folders, a full filesystem, non-UTF-8 names, permission tests as root.
- 17 slice 2 (all passed on first run, so no new bug found; strength notes): swap-to-symlink between plan and confirm (the file behind the link survives; a delete of a link deletes the link, which is inside the home), over-limit 300-char name refused and 255-byte name works, NFC vs NFD names do not clobber each other, hard link to an outside file survives deleting the home-side name, a non-UTF-8 name does not break `list`. Weak spots: the "a/b", "..", "." checks only assert nothing appeared outside the home, not the exact refusal; the TOCTOU test covers one swap shape, not a race with threads. Still not covered: Windows names and paths, FIFOs/devices, a full disk, huge folders, running as root.

### 18 performance, slice 1: first measurements (PARTIAL; sandbox numbers, NOT the user's old CPU)
Environment: sandbox VM, 1 vCPU, ~2 GB RAM, Xvfb 1366x768, X11 backend, panel 480x470. Not a real desktop, not Wayland, not the N5010, not the user's CPU. Build A: debug (`target/debug`). Build B: release opt-level 2, thin LTO OFF, codegen-units 16 (`--config` overrides; the shipped profile uses thin LTO and was not built here). Measured with `PAWLINE_PERF=1` (new: prints render+present wall time per redraw to stderr) and /proc CPU ticks.
Workload 1, idle with the card open, Dusk theme, 20 s: drift OFF = 0 ticks in both builds. Drift ON (default, one redraw per second): debug 239 ticks = ~11% of one core; release 11 ticks = ~0.5%.
Workload 2, typing 58 characters with xdotool at 30 ms per key into the empty input (no model, no history): debug 13 redraws for 58 characters, per-redraw 141 to 149 ms (median 142), so typing visibly lags and events coalesce; release 60 redraws, per-redraw 5.0 to 6.9 ms (median 5.2).
Memory (RSS, idle, before any model load): debug ~31 MB, release ~20 MB.
Reading: the debug-build typing latency gap is real and large; release on this VM is fine. The user's old CPU is unmeasured, so no claim about it. Not measured: startup time to first window, model load time and RSS after the Needle model loads, large-folder listing, long chat (10k lines) render/scroll time, Arabic shaping cost, Wayland, battery.

### Item 17 gap list amendment (slice 3)
- Rejection assertions for "a/b", "..", "." only assert nothing appeared outside the home; they do not assert the exact refusal. Stronger assertions still owed.
- FIFO/device blocking: a FIFO test now exists (list/delete must return within 10 s, sandbox, author-written). Copy/rename/read of a FIFO, and char/block devices, are NOT tested. Blocking risk remains open.
- Windows names/paths, full disk, huge folders, root, threaded TOCTOU: still open.

### Item 18 slice 2 (sandbox VM 1 vCPU, Xvfb 1366x768, release opt2 no LTO, NOT the user's CPU)
- Launch to first redraw: ~56 ms (poll granularity 50 ms, empty data dir with settings file).
- 20,000 empty files in one folder, "list docs": engine took 0.1 s ("Took 0.1s" in the reply), but the 20k-name reply made EVERY later redraw ~232 ms (10 samples 230-236) versus ~5 ms normally. Fix: listings are cut at 300 names with an "and N more" note (MAX_LIST_SHOWN). Re-measure pending.
- Repro: create /tmp/big/docs with 20000 files; PAWLINE_PERF=1 PAWLINE_ROOT=/tmp/big pawline --open; type "list docs", then type letters and read PERF draw_ms lines.
- Still unmeasured: model load/RSS, 10k-line chat scroll, Arabic shaping, Wayland, battery.
- Item 18 slice 2 re-measure (same sandbox, release opt2 no LTO, rebuilt at 80757b2): after the 300-name cap, typing with the 20k listing on screen = 10 redraws at 5.8-6.1 ms (was 230-236 ms). Build env for release: LIBRARY_PATH must include /tmp/stdir (libc++ static) besides the xkbcommon dir. Screenshot /tmp/big2c.png shows "... and 19700 more (showing the first 300 of 20000 names)" unclipped.
- Listing limit (current, documented): "list X" shows 300 names per page. "list X page N" and "list X containing WORD" reach every name (behavioural test on a real 700-file temp folder). Not covered: paging buttons in the GUI, sort options.
- Item 18 slice 3 (sandbox VM 1 vCPU, Xvfb, release opt2 no LTO at 3f11f70, NOT the user's CPU): workload = opt-in history file of 100 messages x ~3990 chars (50 English, 50 Arabic, 558 KB, the app's own 100-message cap), history=on. Startup with it: first redraws 31-34 ms, RSS 22.6 MB. Typing 10 chars then 5 PageUp + 5 PageDown: 20 redraws at 30.4-32.9 ms (median ~31) vs ~5.9 ms with an empty chat. RSS after 22.8 MB. Screenshot /tmp/hist10kc.png shows Arabic lines rendered, RTL, not clipped.
  Repro: write /tmp/ad2/history.txt (lines "U|A<TAB>text") and ui-settings.txt with history=on; PAWLINE_PERF=1 PAWLINE_DATA=/tmp/ad2 pawline --open; xdotool type and PageUp/PageDown.
  Not measured: the 10k-line session-only chat beyond the 100-message cap, model load time/RSS (Needle model is compiled in; no separate load step measured), Wayland, battery, the user's CPU. A ~31 ms redraw on this VM may be several times slower on an old CPU; unmeasured there.

### Item 19 status review (nothing new built; evidence per bullet, PARTIAL)
- New/clear conversation: "new chat"/"forget our chat" (session lines). Covered by bridge tests.
- Persistence: opt-in only, OFF by default, one Settings row, 100 messages x 4000 chars cap, unencrypted file disclosed (15b/15c, pixel proof hs150c, hist_restoredc).
- Forget/delete: turning the row OFF deletes the file; "forget our chat" clears session lines. Per-conversation deletion does not exist (single conversation only).
- Long-conversation limits: caps above; perf measured in item 18 slice 3 (~31 ms redraws at the cap, sandbox).
- Old context causing actions: behavioural isolation test (16b); context never causes an action. Still open: multiple named conversations, per-conversation delete, export.

### Item 19 slice: history search (2026-11-26)
- "search history for X" (also my/saved variants) and "find X in history": read-only case-insensitive search of the saved conversation AND every archived copy. The query is taken byte for byte (bridge::memory_tests::history_search_query_is_verbatim_and_only_history_phrases_match: case, inner spacing and mid-phrase punctuation preserved; ordinary file search untouched). The reply lists up to 10 hits (100 chars each) with who said it and an "(archived copy)" label, counts the rest, says nothing was written or sent, and says unsaved on-screen lines are not searched. Empty query asks instead of searching; history off with nothing saved says to turn it on in Settings first.
- Tests: history::search_tests (behavioural, real temp files: across current + archives, case-insensitive, hit cap with total count, long-line clipping, missing/empty/corrupt dirs safe). Pixel proof, debug build, Xvfb sandbox: hs4_hitsc (2 hits incl. "You (archived copy): help", reply fully readable).
- Note: the user's own search line is itself saved when history is ON, so it can appear in its own results; that is the opt-in keeping rule working as disclosed, not a leak.

### Item 19 slice: export saved history (2026-11-26)
- "export history" (also my/saved-conversation variants): writes the saved conversation AND every archived copy to ONE new plain readable text file, "You:"/"Pawline:" labels, "--- archived copy: <name> ---" section headers. Confirm card shows the exact target path (~/Documents/pawline-history-<epoch>.txt, else home), the line count and how many archived files are included, and states the no-overwrite rule before anything is written. The write is atomic no-replace (temp sibling + hard link; a name that already exists is never overwritten, -1..-9 suffixes tried, then an honest error). Empty history asks instead of writing; cancel writes nothing and says so. The originals are untouched.
- Tests: history::export_tests (behavioural, real temp files: render includes archive sections, write_no_replace never clobbers, temp cleaned up), bridge::memory_tests::export_history_phrases_are_app_level_and_file_export_is_untouched (ordinary "export X to file" requests untouched). Pixel proof, debug build, Xvfb sandbox: ex_cardc (card with exact path, 4 saved lines, 1 archive) and ex_donec ("Exported 4 saved lines to ... The originals are untouched."); exported file verified on disk with the archive section.

### Item 19 slice: list conversations + per-conversation delete (2026-11-26)
- "list conversations" (also my/saved variants): read-only numbered listing of the current saved conversation first, then every archived copy, each with real saved-line counts, plus the follow-up phrases. Empty says so (and to turn keeping on in Settings); over 20 entries collapses with a count.
- "delete conversation N" (also archived/saved/my variants): resolves N against a FRESH listing and the confirm card names the exact file and its line count before anything happens; a wrong or stale number deletes nothing and says how many exist. Deleting the current conversation's file keeps the on-screen conversation and keeping ON, and resets the save watermark so the deleted lines are never rewritten (lines from then on save fresh). Already-gone files are reported as already gone, not as deleted. Bare/unknown targets ("delete conversation", "delete conversation abc", 0, negatives) get a hint, never a deletion. Whole-history phrases ("delete saved history" etc., now also the singular "delete saved conversation") keep the shipped delete-everything card.
- Tests: history::list_tests (behavioural, real temp files: order, counts, unrelated files skipped, missing dir, empty file listed with 0 lines); bridge::memory_tests list_conversations_phrases_are_app_level / delete_conversation_number_is_app_level_and_only_numbers_delete / whole_history_delete_phrases_stay_whole_history (parse, no engine plan, nothing created or deleted). Pixel proof, debug build, Xvfb sandbox: dc_listc (numbered listing), dc_cardc (card naming the exact archive file, Cancel focused), dc_donec ("Deleted conversation 2 ... Nothing else was touched."); data dir verified: archive gone, history.txt kept.
- Still open for item 19: nothing in the conversation-management bullet list; named conversations landed 2026-11-26 below. Remaining item 19 work is review, not new commands.

### Item 19 slice: multiple named conversations (2026-11-26)
- "name this conversation X" (also call/rename variants): reversible, no card; the reply names the future file (history-named-<slug>.txt). "new conversation called X": same no-card flow as "new conversation" - the current conversation is stashed with its disclosure note, the fresh one gets the name. "switch to conversation X" (also open/continue/go back variants): confirm card names exactly what loads (with saved-line count) and where the current one goes; the decide arm stashes first and ROLLS BACK the stash if the swap fails, so the view and the file never disagree. Switching to the current conversation, duplicate names and names with no usable letters all get honest messages, never a file change.
- Model: the live conversation stays in history.txt (restore path unchanged); named ones are history-named-<slug>.txt; a tiny current-conversation state file holds the live name so "switch back" works after a restart. slugs: lowercase a-z/0-9/dash, max 24 chars, no path separators (crate::history::slugify, tested). Names travel byte for byte (quoted names unwrapped).
- Retained-data coverage: list, search, export and per-conversation delete all include named files (one shared collector in history.rs); "delete saved history" and turning keeping OFF delete them AND the name state file, and the card says so. A plain "new conversation" from a named conversation stashes to its named home (not an anonymous archive).
- Tests: bridge::memory_tests named_conversation_phrases_are_app_level_and_names_are_verbatim + slugify_makes_safe_file_names (behavioural parse + slug rules). Pixel proof, debug build, Xvfb sandbox: nc_listc (listing: current "work" 2 lines, conversation "trip" 5 lines), nc_cardc (switch card: exact counts, "kept first as its own named file", Cancel focused), nc_switchedc (trip loaded). Data dir verified: history-named-work.txt holds the work lines, history.txt holds trip, current-conversation says trip.
- Known edge (documented, not hidden): the "new conversation called X" line itself belongs to neither saved file, same as the shipped "new conversation" behaviour.

### Item 20 slice 1 (PARTIAL)
- "reset settings" (also "restore defaults"): text size, theme, drift, spoken replies go back to defaults. Offline mode, mic block and saved-conversation switch are NOT changed and nothing is deleted (stated in the reply). Unit test (structural, author-written) plus GUI proof, release build at 8513133, Xvfb sandbox: Forest theme at 150% text with offline=on, mic=blocked became Dusk at 100%, file kept offline=on, mic=blocked (/tmp/rs0c.png before, /tmp/rs1c.png after).

### Item 20 slice 2 (PARTIAL): settings survive updates
- Format-level proof, theme::update_survival_tests (5 behavioural tests, real temp files through the actual load/save code): old-version partial file keeps its values and defaults later keys, and an old file re-saved by the current build keeps its choices; future-version unknown keys are ignored while known keys still load; one corrupt value falls back without losing the other keys; duplicate keys resolve last-line-wins (documents current behaviour); CRLF endings parse, non-UTF-8 falls back to defaults without a panic.
- Caveat: a re-save writes only the keys this build knows, so unknown future keys are dropped on the next save (documented in the test). No updater exists, so survival is proven at the file-format level, not against a real upgrade.
### Item 20 slice 3 (PARTIAL): settings panel scrolls
- The settings panel (7 rows + privacy notes) was taller than the fixed 440x470 window and its last note line clipped at the window edge. The panel now scrolls: rows and notes move inside a clipped viewport (header stays fixed), a scrollbar appears only when content overflows, Up/Down keep the selected row visible, selecting the last row also reveals the notes below it (keyboard-only users can read the whole panel), the mouse wheel scrolls, row clicks account for the scroll offset, and clicks outside the viewport hit nothing. Opening the panel resets to the top.
- Tests: settings_scroll_tests (scroll_clamp, scroll_to_visible) - structural, pure scroll math, author-written. Behavioural proof, debug build, Xvfb sandbox: ss_topc (panel open at top, scrollbar visible), ss_botc (6xDown: last row selected, theme row clipped at the viewport edge, full notes readable to the last line, scrollbar thumb moved), ss_wrapc (7th Down wraps to the top row), ss_clickc (click on the scrolled Microphone row toggles it to BLOCKED and selects it - scroll-aware hit test).
### Item 20 slice 5 (PARTIAL): adjustable voice (speech rate), 2026-11-26
- New settings row "Speech rate: slow/normal/fast" (120/175/260 wpm; 175 is espeak's own default). Passed to espeak-ng/espeak as -s wpm; flite gets duration_stretch relative to 175; a custom PAWLINE_TTS_CMD keeps exactly its own flags (the settings note about custom commands already covers control). Old settings files default to normal, unknown values fall back to normal, "reset settings" resets the rate (look/feel switch, not privacy). Row survives via the same ui-settings.txt format proven in slice 2.
- Tests: voice::tests::tts_rate_flags_per_engine_and_custom_untouched (behavioural argv per engine incl. zero-rate guard), theme::tests::speech_rate_survives_roundtrip_and_bad_values_fall_back (behavioural, real files). Pixel proof, debug build, Xvfb sandbox: sr_rowc (9th row selected, "normal"), sr_fastc (after Enter, "fast"); settings file verified speech_rate=fast. Caveat: the sandbox has no audio device, so engine invocation is proven at the argv level only; actual speech is unproven here.

### Item 20 design resolution: model management and language (2026-11-26, no code)
- Model management (download/remove/update/verify integrity): the assist model is EMBEDDED in the binary (pawline-needle, needle3.cact, Apache-2.0, see NOTICE). There is no downloadable model, so there is nothing to download, remove or update in-app; model updates would arrive with app updates. Integrity: the runtime validates the weights at load (cactus_load rejects bad bytes; proven by the corruption probe in pawline-needle/tests/concurrent_sessions.rs) and a failed load degrades to "assist unavailable", never a crash. Binary tampering is out of scope until signing exists (not authorized).
- Adjustable language: not started. A language row with only English would be dishonest; it depends on item 11 (Arabic is partial).
- Adjustable animations: the existing "Colours drift slowly" row is the animation switch.

### Item 20 slice 4 (PARTIAL): AI behaviour switch
- New settings row "AI suggestions (local model): on/off" (last row; the panel now scrolls, so it fits). On by default, which is the behaviour before the switch existed; old settings files without the key keep suggestions ON (update_survival_tests). Off means a parser gap never wakes the embedded 35 MB model: no load, no 30s timeout burn, the plain parse error is the whole reply. The switch never affects the network (the model is local); it is deliberately NOT reset by "reset settings" (a deliberate behaviour switch, like the privacy switches). Caveat: if the model was already loaded this session, toggling off stops it being consulted but does not free its memory until restart.
- Tests: bridge::memory_tests::ai_suggestions_off_never_wakes_the_model (behavioural: parser-gap input, gate off via the thread-local test override, no load, no wake, error reply intact); theme round-trip/bad-value coverage extended for the assist key (behavioural, file-level).
- Behavioural GUI proof, debug build, Xvfb sandbox: ai_selc (row selected as the new last row, notes still fully readable), ai_offc (Enter toggles to off); ui-settings.txt persisted assist=off.
- Still open: adjustable voice/language, animation control beyond drift, model download/remove/update/integrity (model is compiled in), upgrade-survival proof against a real updater once one exists.

2026-11-26 item 16 additions: corrupted model data handled gracefully, proven two ways. (1) Build-time:
pawline-needle/build.rs now verifies the embedded needle3.cact sha256 against the audited upstream hash
(docs/LICENSE-AUDIT.md) and fails the build on any swapped or corrupted weights. (2) Runtime: subprocess-isolated
test pawline-needle/tests/corrupt_weights.rs feeds empty / truncated / garbage / bit-flipped weights to cactus_load;
all four are refused with "invalid .cact model", no crash, no silent accept. Picture labels: the "Picture ready
(AI-generated by X, unchecked)" line now lives in one shared formatter used by both completion arms, unit-tested
(bridge::memory_tests::generated_pictures_are_labelled_ai_and_unchecked); the deck consent-card label test already
existed. Item 16 stays OPEN (real-hardware model behaviour and low-memory paths remain).

2026-11-26 item 19 slice: "new conversation"/"new chat" is now a real command. BUG FIXED: it used to fall through
to the parser, and since "new" is a create verb it started a file-creation flow for a file called "conversation"
(live GUI proof: the old build answered "What should the file be called?"). Now: clears the offline chat memory,
the online-chat lines and any pending online card, returns a "clearchat" reply; the window clears the visible
bubbles. With history ON the saved file is NOT erased: it is renamed to history-archived-<epoch>.txt in the data
folder (the user is told the exact name; pixel proof nc2_afterc/nc3_afterc) and a fresh history begins. "forget our
chat" now also returns immediately with an honest line instead of falling through. Behavioural test:
bridge::memory_tests::new_conversation_never_creates_a_file. Still open for item 19: an explicit "delete saved
history" control with confirm, context-limit handling for very long conversations.

2026-11-26 item 19 privacy follow-up: archived conversations no longer linger. "delete saved history" (also
"delete my saved history/conversation(s)") is an app-level confirm card listing exactly what goes: history.txt
with its line count plus every history-archived-*.txt; approving deletes them and turns history keeping OFF
(persisted); cancelling deletes nothing. Turning history OFF in Settings now deletes the archives too and says
how many. The Settings disclosure names archives explicitly. Live proofs (Xvfb, debug): dh2_cardc (card),
dh2_donec (deleted, 3 files), tog_offc (toggle removed 2 archives), st3_settingsc (disclosure text).
Behavioural test: bridge::memory_tests::delete_saved_history_asks_and_deletes_nothing_by_itself.
Still open for item 19: context limits for very long conversations.

2026-11-26 item 19 context limits: every conversation context layer is now bounded and documented. Offline chat
memory: 8 turns (chat.rs MAX_TURNS). Online chat context: CHAT_CONTEXT_LINES, per-line cut. Saved history file:
newest 100 lines, 4000 chars/line, 1 MB file cap (history.rs). Restored-on-launch: same 100-line cap. The one
unbounded layer was the GUI's visible bubble list: new App::MAX_VIEW (200) cap drops the oldest VIEW bubbles,
keeps the welcome line, inserts an in-view marker saying what happened, and adjusts the history watermark so the
saved file's newest-100 policy is unaffected (the marker itself is never saved). Unit tests:
view_cap_tests (3). Verified live: the file cap (a session with 100 seeded + 43 typed exchanges kept exactly the
newest 100 lines). NOT done: a live 200-bubble GUI soak - the Xvfb debug driver needs ~2s/exchange and twice ran
past the 110s harness window around bubble ~190; the slowing itself is what the cap bounds. Unit tests cover the
cap logic; marker rendering reuses the proven "restored" bubble style.
