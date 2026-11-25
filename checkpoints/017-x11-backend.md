# 017 X11 backend
- New `pawline-native/src/x11.rs` (x11rb 0.14, shape feature, pure Rust). Same tiny-skia drawing as Wayland.
- Used when there is no WAYLAND_DISPLAY but DISPLAY exists, or PAWLINE_BACKEND=x11. Wayland code path is unchanged.
- Window: override-redirect (no border, no taskbar), shaped from the alpha channel (rounded corners without a compositor), top-right corner, orb drag, middle-click reset.
- Keyboard: keycode to keysym from the X keymap (Shift, Caps, AltGr level). No compose/dead keys, no IME.
- Clipboard: CLIPBOARD owner (UTF8_STRING/STRING/TEXT/TARGETS) and paste with 0.5 s wait, 64 KB max, no INCR.
- Panel control: `--ctl`, `--panel-only`, socket work unchanged. Hidden = window unmapped.
- Proof (sandbox, Xvfb 1366x768x24, host x86, no window manager): click opens card, typing, Enter, "make a folder" ran, click-copy read back by xclip, paste from xclip, orb drag (1286,12 -> 1066,266), middle-click reset, right-click toggles strip, ctl hide/toggle/status, --panel-only starts unmapped. qemu Westmere (pre-AVX instruction set): card opens and "what is 12 + 30" = 42.
- NOT proven: real XFCE session or xfwm4 or xfce4-panel, Dell, multi-monitor (uses whole X screen size), window stays above other windows (raised only when shown), keyboard focus under a real window manager (we call SetInputFocus), confirm-card keys on X11 (shared code), compositor look, genmon plugin.
