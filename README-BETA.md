# Pawline 0.2.0-beta (native build)

One file, no installs. Make it executable and run it:

    chmod +x pawline-0.2.0-beta-linux-x86_64
    ./pawline-0.2.0-beta-linux-x86_64            # floating orb and chat (Wayland)
    ./pawline-0.2.0-beta-linux-x86_64 --no-ui    # terminal mode, same commands

Pawline works inside your home folder (or the folder in PAWLINE_ROOT).

## What it needs from your system

- Linux x86-64 with glibc 2.35 or newer (built on Ubuntu 22.04). Arch is newer.
- For the window: a Wayland compositor. It uses wlr-layer-shell (niri, sway,
  Hyprland) and falls back to a normal window with PAWLINE_NO_LAYER=1.
  No OpenGL, GTK, WebKit or other libraries are needed.
- Without a Wayland display it starts terminal mode and tells you so.

## Safety

- Plans from the AI assist always need your confirmation. Typing never approves.
  In the window, Cancel is focused; press Right then Enter (or y), or click Do it.
  In terminal mode only the typed word "yes" at a real terminal approves; piped
  input never does.
- Delete moves items to ~/.local/share/pawline/trash. Undo asks first.
- No shell commands are run. File actions are confined to your home folder.

## Known limits

- One target per file command. "delete a, b, c" is treated as ONE name that
  contains the commas, so it fails safely and removes nothing. File names can
  contain commas and the word "and", so Pawline does not guess where they split.
  Quoted multi-target syntax is planned.
- The assist is slow on old CPUs (see below).

## Not tested yet (read this)

- Not tested on the Dell or on niri. Tests so far: headless sway in a sandbox.
- A pre-AVX CPU (your i3-M 370): the window and simple commands ran under a
  qemu Nehalem emulator. The AI assist model on that CPU: see release notes for
  the result. qemu proves instruction compatibility only, not speed.
- The Rust-only Needle engine is experimental and not part of this build.
- Voice, settings, KDE integration and themes are not built yet.

## Assist limit

The optional model assist can clip or change what you wrote (seen: "pets" came back as "ets"). Before any assist plan is shown, Pawline checks it against your own words and refuses, with a message, when:

- a name or path is not in your words as a whole name (a clipped, extended or invented name; part of a longer path or file name);
- a file type was added that you did not say;
- your words do not clearly ask for exactly one operation, or the model chose a different one (for example delete instead of make);
- for move, copy and rename, the source and target cannot be confirmed in the order you wrote them.

Nothing is corrected or guessed. This does not prove the assist understood you: it removes known mismatches only. Plain commands ("make a folder called pets") do not use the assist. Every assist plan still needs your explicit confirmation. Sentences with several operations are refused by the assist.
