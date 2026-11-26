# Item 26 real-world results log

Each entry: build + HEAD, environment, what ran, observed, evidence. Sandbox runs never count as the N5010 run.

## 2026-11-26 A1 cold start (SANDBOX, Xvfb 1366x768, debug build, xdotool, empty data/root dirs)
- Window up 0.21s after launch; first reply rendered 2.44s after the first message's keypress
  (engine self-reported "Took 0.1s" once warm; the wall difference is cold init).
- No hang, no half-written files: data folder EMPTY after a plain exchange (history off, no settings change).
- RSS 31 MB right after first exchange (debug build, sandbox only - not an item 18 number).
- Evidence: /downloads/a1_replyc.png. Method note: an earlier probe misread the cursor blink as a
  reply and lost keystrokes by typing before focus settled; fixed probe polls the chat area only.

## 2026-11-26 A2 mixed session (SANDBOX, Xvfb, debug build, xdotool)
- Sequence: create alpha (card, approved), create beta (card, approved), rename alpha->gamma (no card, by design),
  "what is 12*12" (local math), delete gamma (card, approved) -> root had only beta; undo (card, approved) ->
  gamma restored from the Pawline trash; delete beta -> Enter on the DEFAULT focus (Cancel) -> "Cancelled. Nothing was done.", beta kept.
- Data folder after: frequent-folders.json + task-memory.json only (history off -> no history.txt;
  no settings change -> no ui-settings.txt). Store: journal.jsonl present, trash empty after the undo.
- Evidence: /downloads/a2_card2.png (delete card, Cancel focused), /downloads/a2_endc.png (undo + cancel trail).
- All confirmations needed an explicit focus move to "Do it"; typing alone never approved.
