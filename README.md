# Pawline v0.3.0-test.2 - test binaries (pre-release)

Test builds of commit d7c1d81 (successor line, on top of tag v0.3.0-test.1).
SANDBOX-TESTED ONLY: not verified on real hardware, real audio, a real
desktop, or Windows. The audits mentioned in the docs are author-written.

## What changed since v0.3.0-test.1 (9 fixes from real-hardware feedback)
1. Paste and drag-and-drop of files/text into the input box fixed. Verified
   live under X11 (Xvfb); the Wayland paste/DnD path is UNIT-TESTED ONLY -
   no Wayland compositor exists in the build sandbox, so paste/DnD under
   niri still needs a real-machine check.
2. "copy the conversation" / "copy the last 5 messages" copy the chat to the
   clipboard (plain "You: ..." / "Pawline: ..." lines). Verified live under
   X11 with clipboard read-back.
3. Jokes work with conversational lead-ins ("thats funny, tell me a joke",
   "haha, tell me another") instead of the "I don't know how to ..." error.
4. "time now" / "what time is it" / "time in cairo" / "what is the time now
   in new york" answer offline from the system clock and zone files.
5. Weather: "cairo egypt" and "cairo in egypt" split into place + region
   hint; a typed place name now picks from the multi-match list.
6. A missing parent folder is a friendly message naming the folder, never a
   raw OS error; "... with parent folders" creates the levels as confirmed,
   individually undoable steps.
7. Wallpaper refusal explains in plain words what was checked and what was
   found (swaybg/awww installed, daemon running); awww/swww daemon query is
   now a real backup source, so "anyway" is needed less often. The "anyway"
   path itself is unchanged.
8. The no-speech-to-text message is shorter and gives exact Arch steps
   (AUR sherpa-onnx + the Moonshine model download); the user guide matches.
9. (FYI confirmed working on hardware: math, open firefox, weather flow,
   AI Horde pictures, Pollinations; DuckDuckGo instant answers are thin by
   that API's design.)

## Files
- `pawline-v0.3.0-test.2-linux-x86_64` - Linux x86-64 executable (47 MB, model
  weights embedded). Needs libxkbcommon (present on any Wayland/X11 desktop)
  and glibc 2.34+. Optional: espeak/espeak-ng for voice, ffmpeg for video shrink.
- `pawline-v0.3.0-test.2-windows-x86_64.exe` - Windows cross-build. NEVER RUN
  on Windows; treat as an untested compile artifact.
- `SHA256SUMS.txt` - checksums; verify with `sha256sum -c SHA256SUMS.txt`.
- `pawline-v0.3.0-test.1-*` - the previous test build, kept for comparison.

## Run (Linux)
    chmod +x pawline-v0.3.0-test.2-linux-x86_64
    ./pawline-v0.3.0-test.2-linux-x86_64

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
github.com/linuxkiller9000/pawline, commit d7c1d81 on branch
successor/work-20261126-item2-copy-conversation (GPL-3.0). The corresponding
tag on main is v0.3.0-test.1; these nine fixes are not tagged yet.
