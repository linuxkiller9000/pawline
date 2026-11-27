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
- FINDING RESOLVED 2026-11-26 (was reported as a possible no-reply hang; it is not one):
  an ignored timing probe (bridge.rs assist_load_and_propose_timing) shows NeedleAssist::embedded() loads in
  1.42s (debug, sandbox) and each propose() returns in 0.20-0.24s. Watching the live terminal process after
  "could you maybe tidy my stuff somehow please": the app printed an ORGANIZE plan plus the "Type yes to do this"
  confirm prompt within ~5s, then slept idle at the prompt (proc state S, wchan wait_woken, 1.6s total CPU, one
  thread). It was waiting for approval by design, not hung. My earlier "no reply" reading grepped the log only
  for "Took" lines, and the organize confirm path prints no "Took" until the confirm resolves - analysis error,
  retracted. "please would you tidy things up a bit" is a plain parser reject in 1.7s (weak NL understanding,
  honest message, no hang). Conclusion: no model hang exists on this input; propose() returns in ~0.2s so the
  30s TimedAssist path is not engaged here. Item 16 "real model hang" concern: not reproducible on this evidence.

## 2026-11-26 item 15 real kill -9 loop (SANDBOX, terminal mode via pty, debug build) - PARTIAL
- Script: tools/kill9_loop.py DELAY... (creates folders "make a folder called kN" one per ~20ms, SIGKILL after DELAY seconds, relaunch, read journal.jsonl).
- 15 real SIGKILLs at delays 0.11-1.5s (6 to 68 folders in flight). Every run: journal intents == commits == folders on disk,
  0 unsettled intents, 0 corrupt journal lines, relaunch clean, no recovery messages needed.
- LIMIT (honest): in every run the kill landed BETWEEN operations (folders created == lines sent), because each create finishes in
  well under the 20ms pacing. No kill landed inside the intent->commit window, so recovery of a real mid-operation kill is NOT
  proven by this run; the seeded-journal recovery (item 26 E3) remains the only proof of that branch. A wider window needs a
  slow operation (large copy) - queued next. Earlier 500-line unpaced attempts deadlocked my harness (undrained pty), not Pawline.

## 2026-11-26 item 15 kill -9 INSIDE the copy window (SANDBOX, terminal mode via pty, debug build) - PASS
- Setup: 3 GB file, "copy big3.bin to dest" (control run: full copy in 0.5s for 600MB; 3GB copy ~1.8s), SIGKILL mid-copy.
- Kill at 0.5s / 1.0s / 1.5s after the command: partial copies of 711MB / 1.7GB / 2.66GB on disk. On relaunch, each time:
  the interrupted intent was ABORTED in journal.jsonl (never committed), and Pawline printed the honest recovery note
  "an unfinished copy may remain at ~/dest/big3.bin; check it" - the partial file is left for the user, not silently deleted.
- Kill at 2.0s landed after completion: journal shows commit with fingerprint, full 3GB copy intact, no recovery note. Consistent.
- Combined with the 15 fast-create kills (bundle 114, between-ops) and the seeded-journal E3 test, the real kill -9 crash
  window is now covered for both fast and slow operations in the sandbox. Real-hardware crash stays with the N5010 gate.

## 2026-11-26 item 16a pre-AVX assist survival (SANDBOX, qemu-x86_64 7.2.0 CPU emulation, release build of 34d5beb) - PASS
- Question: the prebuilt cactus runtime contains AVX/AVX2/AVX-512/BMI instructions; the Dell target (i3-370M, Westmere) has none. Does the assist SIGILL there?
- Harness validation first: an unconditional-AVX2 test program SIGILLs (signal 4) under `qemu-x86_64-static -cpu Westmere` and exits 0 natively, so the harness does enforce the CPU model.
- Run: pawline-needle example `embedded_proof` under `-cpu Westmere`: cactus_load + cactus_init + a 64-token proposal for "make a folder called kitten paws" COMPLETED with valid create_folder JSON (confidence 0.7128), prefill 3.1 tok/s, decode 1.6 tok/s, peak RSS 107.2 MB, exit 0 in 54 s.
- Conclusion: the runtime dispatches on CPU features (6 cpuid sites) and the assist works on a Westmere-class CPU. No cpuid guard added - a guard would have disabled a working feature. The earlier "may SIGILL, turn AI suggestions OFF first" warning is downgraded.
- Caveats: qemu is not silicon (one prompt, one session; the Dell run remains the confirmation test); emulated speed says nothing about native Dell speed; other prompts exercise the same kernels. Rebuilding cactus from source stays off the table (restricted engine licence, see NOTICE) and is unneeded.
