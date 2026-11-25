# Checkpoint 009: wrapped prompt box

Branch `feat/image-providers`. Frozen as `checkpoint/wrapped-input`.

Built: the prompt box wraps at spaces onto up to 4 lines, grows upward, then scrolls by line. Caret is a bar, selection is drawn per line.
Up/Down move the caret between lines when the text has more than one line. Mouse click and drag pick a line from y and a glyph boundary from x.
The chat area shrinks as the box grows. The "/" menu sits above the box.

Proof (sandbox, headless sway, uncropped 1280x720 shots): wrapped text, Up then insert, Home then insert, select all.
Unit tests: wrap ranges, caret line at a wrap boundary, click x to glyph boundary.
Not proven: mouse click/drag on wrapped lines in a real window (the sandbox pointer injection did not reach the app), Dell/niri, X11.
Not built: inline images, thumbnails, replies, orb, bar widget.
