# Checkpoint 001: voice model download with consent card

- Branch (frozen): `checkpoint/voice-model-download` at commit 7b280b8
- Checked: 2026-11-25, sandbox (headless sway, host CPU). Not the Dell, not niri.
- Not a release. No asset, tag or binary was published.

## What was checked
- 9 unit tests in `pawline-voice/src/download.rs`: per-file size and sha256 pins (EN tiny, 8 files, 45,398,523 B),
  https only, no redirects, `.partial` staging then atomic rename, cancel flag, a deterministic late-cancel case.
- The in-app consent card: Cancel focused by default, a Download button, Esc cancels a running download, progress shown.
  Rendered in headless sway and the screenshots were inspected by eye.
- F8 compares the sha256 of every model file before voice is treated as installed.

## Untested limits
- A real end-to-end download over the network on the Dell.
- Free disk space is not checked before download (the card says so).
- Real microphone, PipeWire, key repeat, Dell, niri.
- Licence/source audit for redistributing voice libraries is still open; this checkpoint is source only.
