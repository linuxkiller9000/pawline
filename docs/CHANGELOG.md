# Pawline changelog (DRAFT)

Status: hand-maintained DRAFT for checklist item 24 ("clear changelog and version
information"). No updater, version check or release pipeline consumes this file
(see docs/UPDATE-DESIGN.md; nothing there is implemented beyond the version shown
in Settings). Version numbers are the `pawline-native` Cargo package version.
Dates are the git tag dates. Entries describe intent at a theme level; the git
history is the exact record.

## Unreleased - successor line (no tag, no published builds)

Everything below is merged on the successor development branches since
v0.2.1-beta (2026-11-25). It is NOT a release and has no version number yet.

- Input and chat: full edit buffer (caret, Home/End/Delete, word jump, selection,
  select-all), wrapped multi-line prompt box, clipboard (Ctrl+C/X/V, Shift+Insert,
  Ctrl+C on an empty box copies the last reply), mouse caret placement and drag
  selection, click a bubble to copy it, local reply with quote preview
  (right-click / Ctrl+R), result timing on replies.
- Files and folders: exact-name file creation with disambiguation questions,
  organize-by-type with preview and undo, fail-closed no-replace renames and
  copies, video shrink through ffmpeg with confined handles and atomic publish,
  read-only finder (name and text search, capability-based traversal), folder
  destinations named in words (zoxide or Pawline's own list), clickable
  ambiguity choices.
- Online features, permission-first: image generation via explicit Pollinations
  or AI Horde with a per-provider confirm card and cancel arbitration; text
  fallback answers behind an allowlist with a secret-value block; web/wiki
  answers labelled with source links and never presented as current; optional
  local search memory with refresh and forget.
- Privacy and safety gates: offline (network) switch, file-changes switch
  (read-only mode), microphone switch, opt-in history that is OFF by default
  with a from-this-moment watermark, "delete our saved conversation" covering
  archives and named files, steps-only task memory (no request wording on
  disk), structural privacy audit test (every network call flows through the
  two gated senders), provider terms and privacy reading in
  docs/PROVIDER-TERMS.md.
- Settings and accessibility: scrollable settings panel with 12 rows; five
  themes including a WCAG AAA high-contrast "Contrast" theme; reduced-motion
  switch that pauses the colour drift; text size 100/125/150%; speech rate and
  voice rows (espeak voices only); offline, microphone, history, AI-suggestions
  and file-changes switches; Ctrl+, opens and closes Settings; F2 starts and
  stops voice input; the Settings header shows the app version.
- Conversations: new/named conversations, list, switch, per-conversation delete,
  export to one text file, search across saved history and archives, visible
  200-bubble cap with an in-chat marker.
- Voice: spoken replies through espeak/espeak-ng/flite or a custom command,
  microphone voice input behind the mic switch with a privacy notice and a
  listening bar (F2 or the mic button).
- Reliability and security: kill -9 crash harness with an honest partial-copy
  warning, journal-based recovery, parser O(n^2) denial-of-service fix found by
  fuzzing, HTTP redirect and size caps, serialized embedded-model session
  (SIGSEGV under concurrent use fixed), build-time sha256 check of model
  weights, license audit with the full licence texts shipped.
- Platform: X11 backend alongside Wayland, panel-widget socket
  (--panel-only; Waybar/Polybar/genmon examples), movable orb, Windows
  cross-build of core and native kept compiling (build-only; never run),
  sandbox performance measurements for chat growth and model memory.
- Documentation: user guide draft, security audit, licence audit, provider
  terms, update design, real-world test results, this changelog.

## v0.2.1-beta - 2026-11-25

- Parser hardening: case-sensitive names and paths, a non-ASCII parser crash
  fixed, plan-time source check; saved actions removed.

## v0.2.0-beta - 2026-11-25

- pawline-native: the CPU-rendered Wayland window becomes the main interface
  (Tauri UI kept as legacy), with a --no-ui terminal mode and automatic
  terminal fallback without Wayland.
- Write-ahead journal with recovery, trash-based delete and confirmed undo.
- Confirmed wallpaper setting (awww, swaybg, Plasma adapters) with undo and
  refuse-unknown-previous behaviour.
- Assist grounding: proposals whose names are not in the user's words are
  refused; operation, source/target and extension grounding with regression
  tests.

## v0.1.1-beta - 2026-11-25

- Plain window by default; layer-shell becomes opt-in (PAWLINE_LAYERSHELL=1).

## v0.1.0-beta - 2026-11-25

- First beta: deterministic math evaluator and solve-for-x, explicit online
  search (wiki and web) with source links, GPL-3.0 licence, native Needle
  inference path with prefix KV cache (vendored needle-rs, MIT, unmodified).

Note on tags: v0.2.0-beta-rc.1/rc.2 and v0.2.1-beta-rc.1 were release
candidates for the versions above. v0.2.2-test.1 is a CI workflow test tag,
not a release.
