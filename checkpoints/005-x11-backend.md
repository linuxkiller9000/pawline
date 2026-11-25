# Checkpoint 005: X11 window backend (bare Xvfb)

- Branch: `checkpoint/x11-backend` (frozen). Date: 2026-11-25. Code: pawline-native/src/x11.rs, selection in main.rs.
- What: a second windowing backend for the shared `Ui` using Xlib loaded at run time (crate x11-dl 2.21.0, MIT, notice
  added to THIRD-PARTY-RUST.txt). Selected only in a plain local X11 session (DISPLAY ":N", no WAYLAND_DISPLAY, socket
  exists; same rule as the feh adapter). `PAWLINE_NO_X11=1` forces the old behaviour. If libX11 or the display fails,
  it prints the reason and starts terminal mode.
- Window: _NET_WM_WINDOW_TYPE_DIALOG, WM_DELETE_WINDOW, fixed size hints, 32-bit ARGB visual when the server offers
  one (otherwise composited over the card colour), key repeat Release+Press pairs swallowed so a held F9 stays held,
  FocusOut (normal mode only) closes the mic.
- Host runtime dependencies (measured with readelf on the sandbox build): the executable's NEEDED list is only
  libc, libm, ld-linux (no libX11 at link time). At run time, in an X11 session, it dlopens libX11.so.6, which on the
  sandbox host pulls libxcb, libXau, libXdmcp (and libbsd/libmd). On a machine without libX11 the dlopen fails and
  terminal mode starts (that fallback is by reading the code, not run).
- Evidence (provenance: sandbox, Xvfb 1366x768x24, no window manager, xdotool input, screenshots by xwd of the
  root): compact orb; Enter expands; "delete the folder kitten" -> confirm card with Cancel focused -> Enter keeps the
  folder; "delete the folder del2" -> Tab (Do it focused) -> y -> folder deleted; Esc collapses. Screenshots inspected.
- Untested: any real window manager (i3 not installable here: no apt lists), compositor transparency, real focus
  changes, held-F9 repeat, voice, IME or dead keys (keys come from XLookupString keysyms), non-US layouts, the Dell.
  Without a compositor the card's rounded corners show as black.
