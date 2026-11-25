# X11 window backend: plan (not built)

Status: research only. Nothing here exists in the code or any release. Written 2026-11-25 from reading the sources.

## What is already backend-neutral
- All drawing is tiny-skia + fontdue into an in-memory pixmap (`App::draw`, `draw_card`, `draw_chat`, `draw_confirm`).
  Only the last step copies the pixels into a `wl_shm` buffer (BGRA, premultiplied). An X11 backend can reuse the drawing
  and replace only that copy.
- The terminal mode (`--no-ui`) does not use a display. It has been run only in the sandbox.

## What is Wayland-bound today (pawline-native/src/main.rs, 1234 lines)
- `App` implements smithay-client-toolkit handler traits (compositor, seat, keyboard, pointer, layer shell, xdg shell, shm)
  and holds the wl_shm pool. Window sizing, expand/collapse, focus, key handling and pointer handling live in those
  handlers, mixed with the app state.
- `main` exits to terminal mode when `WAYLAND_DISPLAY` is not set.

## Work needed
1. Split the UI state and input handling (keys, pointer, confirm focus, voice keys, caps) from the Wayland handlers, so a
   backend only delivers events (key with text and modifiers, pointer move/click, focus lost, close, frame) and shows a pixmap.
   This keeps every safety rule (Cancel focused, typing never approves, voice forces confirmation, mic closes on
   focus loss) in one place, tested once.
2. X11 backend: connect to the local display, create a window (ARGB visual only if a compositor runs; otherwise opaque),
   set `_NET_WM_WINDOW_TYPE` (utility/dialog so i3 and similar floating-capable WMs float it), title/class "pawline",
   fixed size hints, handle `WM_DELETE_WINDOW`, FocusOut, keyboard via XKB (xkbcommon is already linked), pointer,
   and draw with XPutImage or MIT-SHM.
3. Library choice. x11rb is not in the offline registry here. `x11-dl` 2.21.0 is in the registry and loads libX11 at run
   time (no link-time dependency, so the one-file executable still starts on a Wayland-only system). Using either adds a
   dependency; this needs a decision against the "avoid unnecessary dependencies" rule. A hand-written protocol client is
   possible but larger and riskier.
4. Selection: use the X11 backend only in a plain local X11 session (same rule as the feh adapter: DISPLAY ":N", no
   WAYLAND_DISPLAY, socket exists). XWayland does not make an existing Wayland-only client run in an X11 session, and a
   Wayland desktop keeps using the Wayland backend.
5. Tests: Xvfb plus a window manager (i3 if it can be obtained offline) in the sandbox, with screenshots and key injection,
   labelled sandbox. Real i3/niri/Dell behaviour stays untested until the user's device.

## niri
- niri is a Wayland compositor. The existing Wayland path (layer-shell with xdg fallback) is the relevant one. It has only
  been run under headless sway. A niri check needs niri itself (not in this sandbox so far).

## Open for the owner
- Whether adding x11-dl (run-time loaded) is acceptable, or a hand-written X11 client is preferred.
