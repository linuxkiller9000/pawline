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

Not covered (limits): English only; no Arabic typo handling yet (roadmap step 11); questions, small talk, weather, search and
voice text are not corrected; no phonetic or keyboard-layout typos; typos beyond the distance above are not guessed.

Tests: `cargo test -p pawline-core typo` (a 26-sentence typo list plus names/ties/quotes/yes cases) and the bridge test
`typo_suggestion_never_acts_and_a_pick_still_needs_the_card`.

# Roadmap numbering (current; the old item numbers are historical)

1 GitHub sync (old 1), 2 core safety (old 2), 3 Arabic/RTL display (old 3), 4 composer/themes/Markdown (old 4),
5 voice code (old 5), 6 weather and Q&A (old 6), 7 basic presentations (old 9) - scoped done;
8 typo tolerance, 9 conversation/hints/jokes, 10 presentation extras, 11 Arabic commands/weather/voice,
12 voice and audio hardware, 13 desktop polish, 14 Windows (old 8, NOT done), 15 Linux desktop verification (old 7, partial),
16 final integration and packaging (old 10).
