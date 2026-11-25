# Checkpoint 013: talk-to-it strip (bar widget, first version)

Branch `feat/image-providers`, frozen as `checkpoint/bar-widget`.
User ask (Telegram 16743641): orb movable, and "showing up in the bar (not as an app but as a widget that I can talk to it from)", with the orb closable while the widget stays.
Built: the closed form can be a slim strip (300x40) with a small orb and an "Ask Pawline..." line. Right-click the closed orb or strip to switch between orb and strip. Click the strip, type, Enter: the card opens with the confirm card or answer. Click the small orb end to open the card. The strip drags like the orb. Esc closes the card back to the same closed form.
This is Pawline's own layer-shell strip. It is NOT a panel/bar module (no Waybar/niri bar/KDE panel integration). The user's actual bar is unknown. In-bar integration needs that answer.
Memory only: strip/orb choice and position reset on restart.
Proof: sandbox headless sway, release exe, virtual pointer + wtype. Not proven: Dell/niri, xdg fallback (no strip), the strip over a real bar, caret in the middle of a long line.
