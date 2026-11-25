# Pawline

An offline-first desktop assistant for Linux, written in Rust with Tauri.

**Status: early development, not a release.**

**AI status (read this):** Pawline currently loads the Needle3 model weights
(`needle3.cact`, compiled into the binary) and runs them through the prebuilt
Cactus inference runtime (`libcactus.a`, statically linked). That means the
inference code is Cactus's, not Pawline's. The goal is to run Needle with
inference code written inside Pawline's own Rust codebase, with no separate
engine. That native port is not done yet. Until it is, this repository does
not meet that goal.

Target machine: a Dell Inspiron N5010 (i3-M 370, 6 GB RAM) running Arch Linux
with niri (Wayland). KDE Plasma support is planned alongside niri.

## How it stays safe

The model can propose, never act.

1. A deterministic parser handles known commands first (file operations,
   math answers, and more to come).
2. When the parser doesn't understand, the embedded model may *propose* a
   plan. That proposal goes through the exact same validation as parser
   output, and every model-derived plan requires explicit user confirmation
   before anything runs.
3. Execution is confined at syscall time with cap-std rooted handles: the
   executor can only touch files under its root, whatever the plan says.
4. No shell commands are ever run. Model-written code is never executed.
5. Destructive operations always need confirmation. Overwrites are refused.

Math is computed by a deterministic evaluator, never guessed by the model.

## Current status

Working slice, tested in a headless sandbox (not yet on the target Dell):

- Compact floating orb that expands into a chat panel
- File commands: make, delete, rename, move, copy, list (create works
  end-to-end with the confined executor; the rest are validated by tests)
- Assist proposals with pre-execution confirmation
- Deterministic math answers ("what is 21 * 2", "20% of 85")
- Fail-closed refusal of unrecognized input

Planned, in stages: explicit web search (DuckDuckGo/Wikipedia, offline
preserved), wallpaper setting with theme matching (swaybg, awww, and KDE's
plasma-apply-wallpaperimage), KDE Plasma integration and KDE Connect,
per-change backup/restore, and named reusable actions through the same
safety checks.

## Layout

- `pawline-core` — parser, planner, validator, confined executor, math
- `pawline-needle` — the embedded model runtime (libcactus + Needle3)
- `pawline-ui` — the Tauri desktop app
- `proofs/` — local test captures (not shipped)

## Third-party components

See NOTICE. Needle3 and the cactus runtime are Apache-2.0; the cactus
*engine source* is under a restricted license, so Pawline links the
prebuilt Apache-2.0 binaries instead.

## License

Pawline is free software under the GNU General Public License v3.0 (see LICENSE).
If you reuse this code, your project must be open source under a compatible license.
Third-party components keep their own licenses - see NOTICE.
