# Checkpoint 008: skill menu

Branch `feat/image-providers`. Frozen as `checkpoint/skill-menu`.

Built: typing "/" lists skills. Up/Down select, Tab or Enter fill a starter phrase, Esc closes. Filling never submits.
Starters are plain text that goes through the normal parser, confirm cards and the online-question rules.
"/ask" fills "what is ": general questions on the allowlist are sent without a card, private-looking ones ask first, secret-like text is blocked.

Proof (sandbox, headless sway, uncropped 1280x720): popup, selection, prefix filter, Tab fill, Esc. Unit test for the filter.
Not done: mouse selection in the popup, wrapped prompt box, inline images, thumbnails, replies, movable orb, bar widget.
Not proven: Dell/niri, X11.
