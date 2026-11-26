# Item 26 real-world test plan (DRAFT, 2026-11-26)

Scope: turns checklist item 26 into runnable scenarios. Sandbox runs are labelled SANDBOX and never
count as the N5010 run. Every scenario records: build (debug/release + HEAD), environment
(Xvfb / headless sway / real desktop), steps, expected, observed, evidence file names.

## A. Normal usage (not demo cases)
1. Cold start, first message ever (engine cold load): time to first reply, no hang, no half-written files.
2. Mixed session: folder create/rename/delete with confirm cards, a search, a question, a deck draft,
   settings changes, window resize mid-task. Expected: every destructive step previews first; cancel leaves nothing.
3. Idle 30 min, then commands again: no leaked helper processes, no growth in open files, memory stable
   (record RSS at start/end; item 18 owns the numbers).
4. Quit and relaunch: settings persist; history only if opted in; no session restore otherwise.

## B. Incorrect and ambiguous input
1. Gibberish, empty input, only spaces, very long input (10k chars): no crash, no action, no card.
2. Ambiguous: "delete the file" (which one?), "open the folder" (none named): must ask or refuse, never guess a target.
3. Near-miss typos of commands: suggestion shown first; typo tolerance never rewrites the payload (path/names verbatim).
4. Contradictory sequences: create then immediately cancel; approve then undo; undo twice.
5. Injection-ish text pasted as a command ("delete /", names with .., quotes, glob chars, newline):
   target resolution stays inside the working folder; quoted-name rename gap is tracked in MASTER-CHECKLIST.

## C. Long sessions
1. 100+ exchanges: history cap (100 messages) holds, no unbounded growth, scroll still works.
2. 50 file operations in one session: journal grows but recovery/undo still correct; no fd leaks (count /proc/self/fd before/after).
3. Repeated settings toggles and theme cycles: no state drift, file round-trips.

## D. Offline / network loss
1. Start with no network: local features work; online features show the disclosure-then-refusal path, nothing sent.
2. Network drops mid-session and mid-request: no hang, no partial file writes, queued request fails safe.
3. Offline mode ON, then an online attempt, then OFF: gate counter shows no attempt passed while ON.

## E. After reboot
1. Reboot with default settings: no autostart (none installed), no leftover helpers, no files recreated.
2. Reboot with settings changed: they persist; history.txt loads only if it was ON.
3. Reboot with a pending journal entry (kill -9 mid-operation): startup recovery reports and settles it.

## F. N5010 real hardware (RUNTIME GATE, still owed)
All of A-E on the user's Dell N5010 with its real CPU/GPU, plus: real mic and speakers (item 12),
fractional scaling and multi-monitor if available (item 13/23), battery/thermal watch (item 18).
Not started. Nothing in A-E counts toward F.

Status: plan only. No scenario in A-E has a recorded full pass yet; partial runs exist in the
successor-line reports (file-diff inventory run, history toggle run, repeat-key runs).
