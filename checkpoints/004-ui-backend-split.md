# Checkpoint 004: backend-neutral UI split

- Branch: `checkpoint/ui-backend-split` (frozen), commit 280c60b. Date: 2026-11-25.
- What: all UI state, drawing and input handling moved into `Ui` (no windowing code). The Wayland part is a thin `App`
  that derefs to it. `Out` trait (present pixels, resize) and `Ke` (key) are the seam for another backend.
  `Ui::focus_lost()` is the single focus-loss path.
- Evidence (provenance: sandbox):
  - `cargo test -p pawline-native`: 7 Ui tests pass (Cancel focused by default, typing never approves, Enter on default
    cancels, Tab+Enter or y with Do it focused approves, Esc/n cancel, voice text only previews and is flagged
    voice-origin, long plan needs scrolling to the end, focus loss is safe).
  - Headless sway, old binary (470450f, sha256 15ea772f8c80...) vs new (sha256 0f8529d5c57f...): typed request,
    confirm card (Cancel focused), Enter on default (folder kept), Do it focused, y (folder deleted). All five PNG pairs
    byte-identical.
  - Harness note: the first key of each wtype call is dropped, so every call carries a throwaway Shift_L.
- Untested: AI-origin and voice-origin confirmations in a window, real microphone capture, real focus loss in a
  compositor, the Dell, niri. No X11 backend exists yet.
