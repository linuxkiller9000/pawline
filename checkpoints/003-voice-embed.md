# Checkpoint 003: voice runtime embedded in one executable

- Branch (frozen): `checkpoint/voice-embed` at commit 05e416d
- Checked: 2026-11-25 15:34 to 15:42 EET. Sandbox, host x86-64, qemu-user `-cpu Nehalem` (SSE4.2, no AVX).
- Not a release. No asset, tag or binary was published.

## What was checked
- Build: `cargo build --release -p pawline-native --features embed-voice` with PAWLINE_VOICE_EMBED_DIR set to the four
  vendor libraries (sha256 values in licenses/THIRD-PARTY-VOICE.txt). Exe sha256
  b3813f11b624d995bf9fb82c10fce383e9235db56f3ff07e723dabc2853fa4f7, about 80 MB.
- Full-window run: that exact exe under `qemu-x86_64-static -cpu Nehalem`, drawn into headless sway (software buffers),
  voice turned on in the app, F9 held while an espeak-ng clip was fed through a PulseAudio pipe source,
  release, transcript appeared in the input box as "Make a folder called pictures." with the "from voice: check the
  words" notice and the confirmation text. No illegal-instruction or crash lines in the log (0 matches).
  Screenshots inspected: listening state and the transcript state.
- In this run the 15-second wall limit fired because qemu is slow and the key was held longer than that; the app used
  what was recorded, as designed.
- Separate check (same day): the libmoonshine source-only archive configures and compiles to a libmoonshine.so
  (12,346,520 B; vendor file 12,454,064 B) with a different compiler and -Wno-error. The 50 exported moonshine_* C API
  symbols match the vendor file and both contain the 15 kPcm_* clip arrays. Hashes differ, so this is not a bit match.

## Untested limits
- Not the Dell, not niri, not a real microphone or PipeWire, not real key repeat. qemu shows instruction compatibility,
  not speed (transcription takes tens of seconds under qemu).
- Only the English tiny model. Arabic is not wired.
- The exe needs glibc 2.35 with voice on. Voice libraries are not cleared for redistribution until the release
  gates in the audit are closed (source archives published next to the binary, final notices review).
