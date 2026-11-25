# Checkpoint 015: panel adapters tested against real Waybar and Polybar; socket safety tests

Follows 014 (frozen, not moved). Branch `feat/image-providers`.
- Socket safety (ctl.rs, 3 new unit tests, workspace 291 pass): folder must be ours and not group/world-writable; path must be a socket owned by us; stale own socket is replaced; a live one is never replaced; a regular file, a symlink, a shared folder and a different uid are refused and left untouched. The client also refuses a foreign path.
- Real Waybar 0.9.9 (Ubuntu jammy .deb, extracted in the sandbox, headless sway): module text shows dim when off, green when ready, pink when open; tooltip "Pawline: open"; a click opens the card, a second click closes it.
- Real Polybar 3.5.7 (.deb, Xvfb): text "Pawline (off)" then "Pawline"; click-left toggles (status open then hidden, read through --ctl status).
- KDE plasmoid: only metadata.json parsed as valid JSON. No Plasma, qmllint or kpackagetool here. UNTESTED.
- xfce Generic Monitor: not tested.
- Big limit found: Pawline's UI is Wayland-only. On a pure X11 session (where Polybar/xfce usually run) it starts in terminal mode and a widget could only say "off". The Polybar test ran Polybar under Xvfb next to a Wayland Pawline.
