xfce4-panel: add the "Generic Monitor" plugin (xfce4-genmon-plugin), then in its properties set
- Command: `pawline --genmon`
- Period: 5 seconds
Clicking the text runs `pawline --ctl toggle` (genmon's `<txtclick>` tag).
Alternative with no plugin: pin `pawline-toggle.desktop` (in this folder, works in any desktop) as a launcher.
