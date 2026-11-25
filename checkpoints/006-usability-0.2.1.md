# Checkpoint 006: usability repair on the 0.2.1 base (candidate, not a release)

Branch `fix/usability-0.2.1` (from tag v0.2.1-beta, 35207e7). Frozen copy: `checkpoint/usability-0.2.1`.
No release, no tag, 0.2.1 assets untouched. No voice, no saved actions, no X11 backend: those stay on `voice-preview` behind their own gates.

## Fixed (each shown in a headless sway window, provenance: sandbox)
- Greeting/help reply; short, no refusal.
- Natural search phrasing ("search for ...", "look up ...") runs the existing explicit search.
- `solve for x, x=19 - 8 * (4)` gives `x = -13`.
- Exact app resolution (desktop entry name / id / Exec basename / exact PATH name; no first-word or near match; ambiguity is a pick list; Terminal=true refused). Launch only after explicit approval: marker file absent before, present after; a real installed GUI app (weston-flower) started only after approval.
- File requests: `make a file called a.py inside pawline`, `make files a.py, b.py in pawline` use the exact names; nameless requests and "3 files.py" ask for exact names and create nothing.
- Assist internals no longer appear in replies. Image requests get "I can't generate images yet."
- Result time shown as "Took N.Ns", rounded up to tenths, planning time plus execution time, confirmation wait excluded (unit-tested: 0.21 + wait + 0.31 -> 0.6s).
- History hint chips sit in reserved rows and do not cover text.
- Prompt box: caret, Home/End/Delete, word jump, selection (highlight), Ctrl+A/C/X/V, Shift+Insert, mouse click/drag. Paste and typing are single-line: newlines and control characters become spaces, 2000 character cap; paste is ignored while a confirm card is open.
- Wayland clipboard both directions with another client (wl-copy/wl-paste test tools). Clicking a reply copies it (discoverability: first message and help mention it).

## Not proven / not built
- Dell, niri, i3/X11 (this branch has no X11 backend and no X11 clipboard), real Zen Browser.
- Mouse selection of part of a result; double-click word select; Shift-click.
- Wrapped multi-line prompt box (voice-preview look) is not ported; the prompt scrolls horizontally.
- Staged, not built: image providers, replies/thumbnails, Pollinations text fallback, /skills menu, widget, movable orb.
