# Pawline v0.3.0-test.6 - test binaries (pre-release)

Test builds of commit 320f095 (successor line, on top of v0.3.0-test.5).
SANDBOX-TESTED ONLY: not verified on real hardware, real audio, a real
desktop, or Windows. The audits mentioned in the docs are author-written.

Linux x86_64 and Windows x86_64. THE WINDOWS EXE WAS CROSS-COMPILED AND
NEVER RUN - build verified only. Release profile: opt-level 2, thin LTO, stripped; Pawline's own crates
are built with codegen-units 1 (dependencies stay at the default 16 -
single-unit tract-core needs more RAM than a 2 GB build box has).

## What changed since v0.3.0-test.5 (10 commits)

1. No more silent multi-minute hangs on slow machines: loading and routing
   the Laya model are time-bound now. If a wait runs long you get an honest
   message and Laya stays off for the session; Esc cancels mid-wait; "laya
   on" retries after a timeout.

2. Undo survives restarts: restart-surviving behaviour tests for
   rename/move/copy undo (the behaviour was already correct; this closed
   the test gap).

3. Confirm cards survive a crash: a pending confirmation is restored after
   a crash or kill, fingerprint-checked, and still needs its real card
   click - typing never approves it.

4. "check the draft against <file>": a word-level check of a reviewed
   presentation draft against a UTF-8 text/markdown file you choose
   (home-confined, 256 KB cap). Lines whose words are not in the file are
   named, with the missing words. Honest label on every result: this is a
   word-level match, not understanding. The draft is never changed.

5. "check the draft against <https address>": the same check against a web
   page, behind a disclosure card that names exactly what is sent (the
   address; the site sees your IP and a "Pawline" user-agent; the page
   text is used once, in memory, never saved or sent anywhere else). Only
   the card's "Do it" sends the one request. Offline mode refuses before
   any card; non-https addresses are refused.

6. Saved actions are back (removed in v0.2.1-beta, now on the current
   parser and card flow): save/run/list/show/delete for create-folder,
   create-file and copy commands, with a deterministic re-check before a
   saved action runs and a delete card for removal.

7. The save-presentation card now shows the fact-check count at the top.

## Verification state (honest)

- 636-test suite: green on the exact source commit. Windows cross-build:
  green (build only).
- GUI flows proven by screenshots in a sandboxed Xvfb session (draft
  disclosure card, draft review, URL disclosure card, fact-check review).
- The Linux binary was smoke-tested in an emulated Arch guest (kernel
  7.3.4-arch1-2, QEMU TCG, Westmere CPU, 951 MB RAM): startup OK (no
  SIGILL), an offline query OK, and one Laya-routed query OK. TCG
  emulation is 100-250x slower than native and proves ISA/behaviour
  only - no speed numbers carry over.
- One GUI pixel gap: the re-rendered review header with the neutral
  "word-level match, not understanding" wording is asserted by
  behavioural tests, and the card and review rendering are
  screenshot-proven on the same binary, but the exact new string was not
  re-captured in pixels (the sandbox harness kept racing Pollinations
  rate limits).
- NOT tested: real hardware, real desktops, Windows at runtime, screen
  readers, non-English locales end to end.


# Pawline v0.3.0-test.5 - test binaries (pre-release)

Test builds of commit 85824ae (successor line, on top of v0.3.0-test.4).
SANDBOX-TESTED ONLY: not verified on real hardware, real audio, a real
desktop, or Windows. The audits mentioned in the docs are author-written.

Linux x86_64 only this time. The Windows build is unchanged since test.3
(the new feature is Linux-only for now), so Windows testers should keep
pawline-v0.3.0-test.3-windows-x86_64.exe.

## What changed since v0.3.0-test.4: Laya setup is fully in-app now

No python, no script, no terminal: say "laya on" and Pawline shows a
consent card with the exact costs. Only a click on "Do it" starts anything
(typing never approves it; Esc cancels while it runs; offline mode refuses
the download instead of showing the card). It then downloads the model
(288 MB) and tokenizer (2 MB) from huggingface.co (free, no account;
huggingface.co sees your IP address), converts the model on your machine,
and loads it. The downloads are size- and checksum-pinned, so a changed or
corrupted file is refused, and nothing is installed until everything
finishes.

Correction to the test.4 notes below: the one-time download is about
290 MB (288 MB model + 2 MB tokenizer), not "~160 MB".

Honest costs: about 950 MB of free disk during setup, about 640 MB kept
afterwards (the download itself is removed), about 700 MB RAM while the
model is loaded, and a few seconds per suggestion on an old CPU (the first
suggestion of a session also loads the model). While it thinks you see a
status line, you can keep typing, and Esc stops the wait. Normal commands
never touch the model.

The manual script still works as a fallback (needs python3 and about 2 GB
free during conversion): sh laya-setup.sh

Also in this build: a display fix - when a suggestion's pick row was shown,
it could cover the last lines of the message above it; the chat view now
makes room for it.

Then say "laya on" inside Pawline (session only; "laya off" unloads it).

# Pawline v0.3.0-test.4 - test binaries (pre-release)

Test builds of commit f25a913 (successor line, on top of v0.3.0-test.3).
SANDBOX-TESTED ONLY: not verified on real hardware, real audio, a real
desktop, or Windows. The audits mentioned in the docs are author-written.

Linux x86_64 only this time. The Windows build is unchanged since test.3
(the new feature below is Linux-only for now), so Windows testers should
keep pawline-v0.3.0-test.3-windows-x86_64.exe.

## What changed since v0.3.0-test.3: the Laya experiment (opt-in, OFF by default)

When the normal parser does not understand a message, Pawline can now ask a
small LOCAL model (Laya intent router, 150M, Apache-2.0, by vrajnotviraj)
what you probably meant and SUGGEST one rephrased command. Nothing runs by
itself: the suggestion only refills the input, and pressing Enter sends it
through the normal parser and the usual confirmation cards. No network,
English only.

It is OFF by default and session-only: say "laya on" to try it, "laya off"
to stop. Without the model files, "laya on" just explains the setup.

Setup (one time, needs python3 and about 2 GB free during conversion):

    sh laya-setup.sh        # included next to this README

It downloads the author's int8 model (~160 MB) and converts it locally to
the fp32 files Pawline loads (656 MB final). Then say "laya on" in Pawline.

Honest costs: about 700 MB RAM while the model is loaded, and a suggestion
takes about a second on a fast desktop - expect several seconds on the Dell
(the first suggestion of a session also loads the model, slower still).
While it thinks you see a "thinking" line, you can keep typing, and Esc
stops the wait. Normal commands never touch the model.

This is an experiment: the normal keyword parser stays the default and is
unchanged. Please try your usual "it didn't understand me" phrasings with
laya on and tell me which suggestions read wrong.

## KDE Plasma 6 panel widget (new, optional)

pawline-plasmoid.tar.gz is a Plasma 6 panel widget: a small icon with a
status dot (grey off, green open, cyan working, yellow needs confirmation),
hover for the text, click to open or close Pawline. Install:

    kpackagetool6 --type Plasma/Applet --install pawline-plasmoid.tar.gz

Then right-click the panel, "Add Widgets", find "Pawline", drag it in.
NOT RUNTIME-TESTED: the build sandbox has no Plasma session. The widget is
statically checked only - please do the 30-second check in the repo's
integrations/README.md and tell me if the dot never leaves grey.

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
