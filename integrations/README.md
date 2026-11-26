# Panel widgets for Pawline

Pawline works as a small panel widget that stays in your bar even when the floating orb is closed. These files are **examples to copy by hand**. Nothing here edits your panel config for you, and nothing is installed by Pawline.

How it works: a running Pawline listens on a local socket (`$XDG_RUNTIME_DIR/pawline.sock`, owner-only). It accepts only four fixed words: `toggle`, `show`, `hide`, `status`. It never runs text from the socket.

```
pawline --panel-only        start with no orb; the card opens only from the widget
pawline --ctl toggle        open or close the card (starts Pawline in panel-only mode if it is not running)
pawline --ctl status        off | hidden | idle | open | busy | confirm
pawline --waybar            JSON for a Waybar custom module
pawline --polybar           one line of text for a Polybar script module
pawline --genmon            text + click for the xfce4 Generic Monitor plugin
```

Put `pawline` on your PATH first (for example `~/.local/bin/pawline`), or use the full path in the examples.

| Panel | Folder | Status |
|---|---|---|
| Waybar | `waybar/` | tested with real Waybar 0.9.9 (Ubuntu package) under headless sway: text, colour class, tooltip, click opens and closes the card |
| Polybar | `polybar/` | tested with real Polybar 3.5.7 (Ubuntu package) under Xvfb: text changes and a click toggles Pawline. Note: Pawline itself ran on Wayland |
| xfce4-panel (Generic Monitor) | `xfce/` | same, **never run inside xfce4-panel** |
| KDE Plasma 6 | `kde/` | **not runtime-tested** (no Plasma 6 session in the build sandbox): icon + status dot + tooltip, polls `pawline --ctl status` every 5 s, click toggles. Statically checked only |

Confirmations still happen in Pawline's own card (Cancel focused). The widget only opens or closes it. Typing into the panel itself is not supported.


## 30-second KDE check (please run after installing)

```
kpackagetool6 --type Plasma/Applet --install integrations/kde/org.pawline.widget
```

Then right-click the panel, "Add Widgets", find "Pawline", drag it in.

- A terminal icon with a grey dot appears; the dot turns green/cyan/yellow as
  Pawline opens, works or asks for a confirmation (hover for the text).
- Clicking it opens and closes the Pawline card.
- If the widget never appears in the list: `kpackagetool6 --type Plasma/Applet --show org.pawline.widget`
  should print the metadata; if it does not, the install path was wrong.
- If the dot never leaves grey: `pawline --ctl status` in a terminal should
  print one of off/hidden/idle/open/busy/confirm; if that works but the dot
  stays grey, the widget's polling is broken, tell me.

## Note: Wayland and X11

Pawline's window works on Wayland and on X11 (it picks Wayland when a Wayland
display exists; PAWLINE_BACKEND=x11 forces X11). The socket the widgets use is
the same on both. On KDE the widget works in a Plasma Wayland session; under
Plasma X11 it should work the same way, but that combination has not run in
the sandbox at all (no Plasma session of either kind).

Socket safety: Pawline only uses `$XDG_RUNTIME_DIR/pawline.sock` if the folder is yours and not writable by others and the path is a socket owned by you. It replaces only your own stale socket. Anything else at that path is left alone and the widget shows `off`.
