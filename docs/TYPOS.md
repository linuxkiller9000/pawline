> NOTE: the roadmap numbering in this file is outdated. The master numbering (1-29) and statuses live in docs/MASTER-CHECKLIST.md.

# Typo tolerance (roadmap step 8)

Scope: misspelled COMMAND WORDS only (pawline-core/src/typo.rs). Local, deterministic, no model, no network.

- Corrected: the command verb (make, create, delete, remove, trash, rename, move, copy, open, launch, set, change, undo,
  write, draft, organize, shrink, compress, list, show) and the fixed words right after it (folder, file, directory,
  presentation, slideshow, wallpaper, called, named, inside, into, about). A polite lead-in ("please", "pleaes", "can you")
  is dropped.
- Never corrected: file and folder names, anything after "called"/"named", anything after "folder"/"file", quoted text,
  paths, words with digits, dots, "_" or non-English letters, the words after a five-word window.
- Distance: 1 edit (swap, missing, extra or wrong letter) for words of 4 to 7 letters, 2 for 8 and more, never for 3 or
  fewer letters. A tie between two candidates is never guessed. A fixed list of real English words (snow, mode, code, ...) is not treated as a typo.
- The corrected sentence must be understood by one of Pawline's own parsers, otherwise nothing is suggested.
- A changed word never runs by itself. The reply is "Did you mean: <corrected request>" as a clickable pick. The pick refills
  the input; Enter then shows the normal confirm card with the full path (a picked suggestion is also forced to confirm).
- Typing never approves anything (a misspelled "yes" does nothing); Enter on a card still means the focused button, Cancel by default.

Also covered (step 8 extension): the weather word ("wether in cairo"), search words ("serach wikipdia for cats"), the question word
("waht is gravity"), and a single misspelled greeting/help word (answered directly, text only). The place, query, question text, topic,
names and quoted text are payload and stay byte for byte; the suggestion is shown first and the existing online disclosure cards still apply.

Not covered (limits): English only; no Arabic typo handling yet (roadmap step 11); conversation text (step 9) is not corrected yet;
voice text is not corrected; no phonetic or keyboard-layout typos; typos beyond the distance above are not guessed.

Held-out set (pawline-core/tests/typo_heldout.rs, written before the weather/search/question scanners and not tuned on): 38/40 positives
as expected and 29/29 negative controls left alone. Known misses, left as they are: "mkae folder reports" (the corrected "make folder reports" is not
understood by the parser, so nothing is suggested) and "copy the flie a.txt to docs" (the original already parses as copying a thing named
"flie a.txt", so it is not touched; it then fails safe as not found). This is a small set, not an accuracy rate.

## Re-measured 2026-11-28 (how-to typos)
The two how-to misses recorded in the chat section below ("how do i delte a folder", "how can i reanme a file") no longer miss: both now
get the hint for the word they meant (Delete / Rename) with "Nothing was done", because the hint lookup already tolerates a one-edit
command word. Re-measured directly on the chat state, not through the GUI. The note below is history, not current behaviour.

One real gap found and left open: "how do i mve a file" gets the MAKE hint, not the move hint, because a 3-letter misspelling is never
corrected by policy (distance 0 for words of 3 or fewer letters) and the word "file" then matches the make entry. Short words are left
alone on purpose; the cost is that a very short misspelled verb can pick the wrong topic. A fix would have to relax that rule, so it stays
a decision, not a patch. Decision (main, 2026-11-28): short words stay safe, so the wrong-topic hint for a 3-letter
misspelling is accepted and documented rather than fixed.

Tests: `cargo test -p pawline-core typo` (a 26-sentence typo list plus names/ties/quotes/yes cases) and the bridge test
`typo_suggestion_never_acts_and_a_pick_still_needs_the_card`.

# Roadmap numbering (current; the old item numbers are historical)

1 GitHub sync (old 1), 2 core safety (old 2), 3 Arabic/RTL display (old 3), 4 composer/themes/Markdown (old 4),
5 voice code (old 5), 6 weather and Q&A (old 6), 7 basic presentations (old 9) - scoped done;
8 typo tolerance, 9 conversation/hints/jokes, 10 presentation extras, 11 Arabic commands/weather/voice,
12 voice and audio hardware, 13 desktop polish, 14 Windows (old 8, NOT done), 15 Linux desktop verification (old 7, partial),
16 final integration and packaging (old 10), 27 documentation (install, build, offline behaviour, platforms and limits, troubleshooting,
clean uninstall from a verified inventory, user documents kept by default), 28 real-world testing (normal and wrong/ambiguous commands, long sessions,
offline, after reboot, on the actual N5010 - a VM does not replace it). 27 and 28 are closeout gates; numbers 17-26 are not used.


## Chat typo coverage (held-out, 2026-11-26)
pawline-core/tests/chat_heldout.rs: 22 typo/phrase positives and 15 negative controls written after chat.rs, before any tuning.
First run: 13/22 positives, 4 negatives wrongly triggered ("the jokes on you", "that was a thin joke book", "my friend tells jokes", "i like thanks giving").
After fixing (joke needs a request form; thanks must be first or last word; added a few swap typos): 19/22 positives, 0/15 negatives. After tuning this set is regression only.
Known misses: "how do i delte a folder" and "how can i reanme a file" (typo'd action word in a how-to) get the safe "discussion" reply instead of the hint; "tel me a joke please" was fixed by adding "tel".
Voice: transcripts only fill the draft box; sending goes through the same typo gate as typed text, so a misheard command word is a suggestion, never a direct action. No voice-specific typo set exists yet (item 12).
