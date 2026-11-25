# Checkpoint 012: movable orb

Branch `feat/image-providers`, frozen as `checkpoint/movable-orb`.
- Drag the closed orb (layer-shell only) to move it. A click without moving (under 6 px) still opens the card. Middle click on the orb puts it back in the corner.
- The open card docks in the top-right corner so it always fits; closing it returns the orb to where it was left.
- Position is kept in memory only (resets when Pawline restarts). Nothing is written to disk.
- Method: layer margins from (position - press position), because sway keeps reporting positions relative to the surface's place at the press. A compositor that updates positions during the drag (possibly niri) could move too far. Not proven on niri/Dell, so the middle-click reset exists.
Proof: sandbox, headless sway, release exe, virtual pointer: drag from the corner to the middle-right, click opens the docked card, Esc returns the orb to the dragged place.
Not proven: Dell/niri, window (xdg) fallback (no drag there), multi-monitor.
