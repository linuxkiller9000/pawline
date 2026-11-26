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

## 2026-11-26 B bad and ambiguous input (SANDBOX, terminal mode, debug build, piped stdin)
- Gibberish: clean "I don't know how to ... yet", no action. Empty line and spaces-only: ignored, no action, no card.
- "delete the file" (no referent): "I don't know what file refers to" - refuses, never guesses a target.
- "open the folder" (no such app): refusal explaining it chose nothing; nothing opened.
- Typo: "maek a folder called typo1" -> suggest-first ("I read maek as make"), nothing done, names never corrected.
- "delete /": refused, "outside your home area". 'make a folder called "../escape"': refused, "looks like a path, not a name".
- Shell-metachar name 'a;b$(touch PWNED)': created LITERALLY inside the root (create is non-destructive, no card by design);
  no shell interpretation anywhere (no PWNED file), payload never rewritten. Undo of it correctly required confirmation (not approved in a pipe, nothing done).
- Undo with empty journal, twice: "there is nothing to undo yet" both times, no state change.
- FINDING (performance, item 18/16): a 10,000-character input went to the local AI assist, which timed out after ~30s
  (total answer time 33.7s) and was then disabled until restart (designed degradation; the message honestly says the
  stuck worker may still use CPU in the background). The plain parser then answered correctly and the app stayed responsive.
  No crash, no file writes. Consider a length gate before the assist so huge inputs skip it.

## 2026-11-26 D offline / network loss (SANDBOX, terminal mode under a pty, debug build)
- Offline mode ON (persisted ui-settings.txt): weather gets the refusal ("Offline mode is on... nothing was sent"),
  NO disclosure card; local math unaffected. (This run exposed and we fixed terminal mode ignoring the setting; before the
  fix the disclosure card still gated everything, so nothing was ever sent unapproved.)
- True no-network start (unshare -rn, no interfaces): local math works; a weather request shows the disclosure card
  FIRST (nothing sent pre-approval), and after a typed "yes" fails clean in 0.1s: "Could not reach the weather service
  (Temporary failure in name resolution). Nothing was shown; check your connection and try again." No hang, no partial
  writes (only the usual frequent-folders.json appeared). App fully responsive afterwards.
- With network (control run through the same pty driver): card -> yes -> real geocoding reply with the place chooser.
- Not covered: a connection dying MID-TRANSFER (both runs fail at lookup/connect time); reboot scenarios (E) open.

## 2026-11-26 C long sessions (SANDBOX, terminal mode under pty + Xvfb GUI, debug build)
- C1 120 exchanges (math): 24.1s wall, no crash, no slowdown. Open fds constant at 3 before/during/after (no fd leak).
  RSS 11.4 -> 15.2 MB over the 120 exchanges (session context growth, ~35 KB/exchange; debug build, sandbox only -
  steady-state and GUI bubble growth remain item 18 numbers to measure separately).
- C2 50 folder creates: all 50 journaled (journal = 50 intents + 50 commits + 1 undo_of = 101 lines, exactly consistent),
  one approved undo removed exactly one folder (49 on disk), a second undo correctly found nothing to undo.
- C3 20 drift toggles + 8 theme cycles (GUI, xdotool): ui-settings.txt round-trips cleanly - drift back ON (even count),
  theme back to Dusk (8 mod 4 themes), all other fields intact, no corruption.

## 2026-11-26 E after reboot, sandbox-adapted to relaunch (SANDBOX, Xvfb + terminal, debug build)
- E1 default settings: no autostart or desktop entries exist (no installer), and the file-diff inventory run already
  proved a first launch writes nothing. Covered by the inventory run, not re-run.
- E2 persistence across relaunch (GUI, pixel proof e2_run2c/e2_run3c): pre-set Ocean theme, offline ON, history ON.
  Relaunch applied all three: Ocean colours visible, "Restored 1 earlier messages..." with the saved line shown,
  weather refused with NO card (offline persisted). ui-settings.txt and history.txt contents verified on disk.
- E3 crash recovery (terminal): a seeded pending journal intent for a completed create_dir settles to
  "recovered ... it had completed" + a commit line; one for a change that never happened settles to
  "it had not completed" + an abort line, nothing created. (Deterministic seeded journal; a live kill -9
  mid-write window is too narrow to time in the sandbox - the journal format is the same one exec writes.)
- A real reboot (kernel, services, autostart reality) stays with the N5010 gate.

## 2026-11-26 item 18 first measurements + item 16 FINDING (SANDBOX, terminal mode, debug build)
- Assist model load: RSS 11.3 MB -> 112.7 MB (+101 MB for the 35 MB model + runtime). Debug build, sandbox CPU.
- FINDING (root cause unverified): two natural-language requests ("could you maybe tidy my stuff somehow please",
  "please would you tidy things up a bit") each produced NO reply within 40s in terminal mode. The designed 30s
  TimedAssist limit should have returned an error reply at ~30s. Candidates: NeedleAssist::embedded() load itself is
  very slow on this CPU, or a runaway inference thread starves the main thread (the honest ASSIST_OFF_NOTE says stuck
  work cannot be stopped). A 120s-window measurement was cut short by the harness limit, so whether a reply eventually
  arrives is UNKNOWN. Short gibberish in scenario B answered in 2.1s, so this is input-shape dependent.
  Next: measure embedded() load time alone, then propose() with a 120s window; if the timeout can be blown past, that
  is an item 16 bug to fix (the UI would hang equally long).
