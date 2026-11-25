# Checkpoint 014: panel widget plumbing + examples (Waybar, Polybar, xfce genmon, KDE)

Branch `feat/image-providers`, frozen `checkpoint/panel-widgets`.
User (16743641, 17816153, 17821471): widget in the bar, orb closable while the widget stays; panels: KDE, Waybar, Polybar; sometimes xfce (panel not named).
Built: owner-only socket ($XDG_RUNTIME_DIR/pawline.sock) with four fixed words (toggle/show/hide/status); `--ctl`, `--panel-only` (no orb at all), `--open`, `--waybar`, `--polybar`, `--genmon`. A second Pawline refuses to start. `--ctl toggle` on a stopped Pawline starts it in panel-only mode.
Examples in integrations/ (copy by hand, nothing edits a live panel config): Waybar, Polybar, xfce Generic Monitor, KDE plasmoid, and a launcher .desktop.
Proof, sandbox headless sway, release exe: status off/hidden/open, panel-only start leaves a fully blank screen (no orb), toggle opens and closes the card, output of the three formats, socket mode srw-------, bad word refused, second instance refused, toggle auto-starts. Unit test for formats (workspace 288 pass).
Not proven: running inside Waybar, Polybar, xfce4-panel or Plasma (none available here); the KDE plasmoid is untested. xfce4-panel as his xfce panel is an assumption (he did not name it). Dell/niri unproven.
