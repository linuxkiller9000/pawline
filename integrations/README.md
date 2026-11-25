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
| Waybar | `waybar/` | the `pawline` commands it calls were tested in a sandbox; **never run inside Waybar** |
| Polybar | `polybar/` | same, **never run inside Polybar** |
| xfce4-panel (Generic Monitor) | `xfce/` | same, **never run inside xfce4-panel** |
| KDE Plasma 6 | `kde/` | **untested**: needs a Plasma session; written from the Plasma 6 plasmoid pattern |

Confirmations still happen in Pawline's own card (Cancel focused). The widget only opens or closes it. Typing into the panel itself is not supported.
