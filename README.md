# Pawline v0.3.0-test.1 - test binaries (pre-release)

Test builds of commit 34d5beb (tag v0.3.0-test.1 on main). SANDBOX-TESTED ONLY:
not verified on real hardware, real audio, a real desktop, or Windows. The
audits mentioned in the docs are author-written.

## Files
- `pawline-v0.3.0-test.1-linux-x86_64` - Linux x86-64 executable (47 MB, model
  weights embedded). Needs libxkbcommon (present on any Wayland/X11 desktop)
  and glibc 2.34+. Optional: espeak/espeak-ng for voice, ffmpeg for video shrink.
- `pawline-v0.3.0-test.1-windows-x86_64.exe` - Windows cross-build. NEVER RUN
  on Windows; treat as an untested compile artifact.
- `SHA256SUMS.txt` - checksums; verify with `sha256sum -c SHA256SUMS.txt`.

## Run (Linux)
    chmod +x pawline-v0.3.0-test.1-linux-x86_64
    ./pawline-v0.3.0-test.1-linux-x86_64

## Pre-AVX processors (e.g. Intel i3-370M): expected to work
The embedded AI-model runtime contains AVX/AVX2/BMI instructions, but they are
behind CPU dispatch: the full "AI suggestions (local model)" path was tested
under qemu CPU emulation set to Westmere (the i3-370M's class) and completed a
real inference without an illegal instruction. Emulation is not silicon, so
the confirmation test is on the real machine: use it, and if AI suggestions
ever crash with "illegal instruction", open Settings (Ctrl+comma), set
"AI suggestions (local model)" to OFF (everything else keeps working) and
report it.

## Source
github.com/linuxkiller9000/pawline at tag v0.3.0-test.1 (GPL-3.0).
See docs/CHANGELOG.md and docs/USER-GUIDE-DRAFT.md there.
