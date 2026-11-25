# Checkpoint 007: image generation (explicit Pollinations / AI Horde) plus save

Branch `feat/image-providers`, frozen as `checkpoint/image-providers`. Not a release. Scope: generation and saving to ~/Pictures/Pawline only; no inline display, no thumbnails, no replies UI.
- Provider must be named; card states what is sent (prompt, IP); Cancel focused; nothing sent before approval. No payment path (402/429 reported as plain failure, never retried).
- Esc while working ends the wait at once and discards any late result (no file written). No silent retry. Over-long prompt (300 chars) is refused with its length, never truncated.
- Evidence (sandbox, headless sway): 12 imagegen unit tests (mocked HTTP), 265 workspace tests pass; live smoke both providers with a synthetic prompt; UI sheets img-sheet.png, img-cancel-sheet.png.
- Not proven: Dell/niri; Horde under queue pressure; Pollinations rate-limit live; picture quality comparison (kite prompt ambiguous).
