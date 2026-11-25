# Checkpoint 010: picture preview in chat

Branch `feat/image-providers`. Frozen as `checkpoint/inline-images`.

Built: after a picture is saved, the "Done" bubble shows a small preview (max 220x150, never enlarged). PNG and JPEG only. WebP (AI Horde often returns it) is saved but not previewed.
Only a file directly inside a `Pictures/Pawline` folder named by a "Saved <path>" line is opened. Size is read from the header first (max 4096 per side, 16 MP, 8 MB file) before any pixels are decoded.
New dependencies: `png` 0.17 (already in the lock through tiny-skia) and `jpeg-decoder` 0.3.2 without default features (no threads). Both MIT/Apache-2.0, listed in NOTICE.
User-sent images are not handled yet (no upload feature exists, and none was authorised).

Proof: sandbox, headless sway: a real Pollinations picture was generated after the confirm card and the preview shows inside the bubble. Unit tests for PNG, JPEG, junk, oversize, truncation and the path rule (workspace 285 pass, 0 fail). The 16 native unit tests also pass under qemu-user Westmere (pre-AVX), debug build.
Not proven: the preview on a real release build under qemu, Dell/niri, WebP, clicking the thumbnail, reply quotes, orb, bar widget.
