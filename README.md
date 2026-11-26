# Pawline v0.3.0-test.3 - test binaries (pre-release)

Test builds of commit 49042aa (successor line, on top of v0.3.0-test.2).
SANDBOX-TESTED ONLY: not verified on real hardware, real audio, a real
desktop, or Windows. The audits mentioned in the docs are author-written.

## What changed since v0.3.0-test.2 (4 fixes from real-hardware feedback)
1. Math: equations that simplify to a linear one now solve offline, with
   full expressions on both sides ("what is 88+50 - 90 = x * (8)^2" and
   "solve 88+50 - 90 = x * (8)^2" both give x = 0.75 in 0.1s; parentheses,
   powers, implicit multiplication like 2x and 2(x+1) supported). Math input
   can no longer fall through to the Pollinations online offer. Quadratics
   and x-in-denominator get a plain "linear only" reply, offline.
2. A lone unknown word ("say", "a") now fails in 0.1s instead of ~6s and
   offers the closest commands plus "hint" / "search <word>" ways forward.
3. Pollinations overuse: questions only you or Pawline can answer no longer
   offer an online card - personal facts ("what is my name", "how old am i",
   "what is my ip") get a plain offline "I don't keep that" reply, and
   immediate-weather questions ("will it rain tomorrow", "do i need an
   umbrella") route to the weather feature (name a place, Open-Meteo asks
   first). Genuine open questions ("what is a black hole") still offer the
   card, by design.
4. Wayland paste AND drag-and-drop: real root cause found and fixed. The
   data request to the compositor was never flushed while Pawline waited
   for the data, so every paste/drop timed out after half a second. This
   affected all Wayland compositors, not just niri.
   HONEST CAVEAT: this fix is verified from the event-loop source and the
   test suite only - it has NOT run on a live Wayland compositor (the build
   sandbox has none), so please do the 30-second check below.

## 30-second Wayland check (please run after updating)
- Copy any text in another app, click into Pawline, press Ctrl+V: the text
  should appear instantly.
- Drag a text file onto the Pawline window: its path should insert.
- If either still fails: run `wl-paste -l` in a terminal right after copying
  (proves the clipboard works outside Pawline), and
  `WAYLAND_DEBUG=1 pawline 2>&1 | grep -i data_offer` while pasting.
  No "offer" lines = niri never sent us the clipboard (a different bug);
  offer lines but no paste = send me that log.

## Files
- `pawline-v0.3.0-test.3-linux-x86_64` - Linux x86-64 executable (47 MB, model
  weights embedded). Needs libxkbcommon (present on any Wayland/X11 desktop)
  and glibc 2.34+. Optional: espeak/espeak-ng for voice, ffmpeg for video shrink.
- `pawline-v0.3.0-test.3-windows-x86_64.exe` - Windows cross-build. NEVER RUN
  on Windows; treat as an untested compile artifact.
- `SHA256SUMS.txt` - checksums; verify with `sha256sum -c SHA256SUMS.txt`.

## Earlier changes
### v0.3.0-test.2

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
