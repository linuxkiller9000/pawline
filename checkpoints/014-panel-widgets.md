# Checkpoint 014: panel widget plumbing + examples (Waybar, Polybar, xfce genmon, KDE)

Branch `feat/image-providers`, frozen `checkpoint/panel-widgets`.
User (16743641, 17816153, 17821471): widget in the bar, orb closable while the widget stays; panels: KDE, Waybar, Polybar; sometimes xfce (panel not named).
Built: owner-only socket ($XDG_RUNTIME_DIR/pawline.sock) with four fixed words (toggle/show/hide/status); `--ctl`, `--panel-only` (no orb at all), `--open`, `--waybar`, `--polybar`, `--genmon`. A second Pawline refuses to start. `--ctl toggle` on a stopped Pawline starts it in panel-only mode.
Examples in integrations/ (copy by hand, nothing edits a live panel config): Waybar, Polybar, xfce Generic Monitor, KDE plasmoid, and a launcher .desktop.
Proof, sandbox headless sway, release exe: status off/hidden/open, panel-only start leaves a fully blank screen (no orb), toggle opens and closes the card, output of the three formats, socket mode srw-------, bad word refused, second instance refused, toggle auto-starts. Unit test for formats (workspace 288 pass).
Tested with real Waybar 0.9.9 (headless sway) and Polybar 3.5.7 (Xvfb) from Ubuntu jammy packages. Not proven: xfce4-panel, Plasma (KDE plasmoid only JSON-validated, QML untested), Dell/niri. Wayland-only UI: no X11 window backend yet.
