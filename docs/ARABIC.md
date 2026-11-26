# Arabic (item 11), status: PARTIAL (11a)

What works (tests: `pawline-core/tests/arabic_commands.rs`, bridge test `arabic_requests_become_visible_english_commands...`, weather unit test):
- A fixed set of Arabic request SHAPES is translated into the English command the normal parser already understands, and the
  English command is shown ("Understood from your Arabic request as: ..."). Shapes: create folder/file (optional "في المستندات / سطح المكتب / التنزيلات"),
  delete, rename (`غير اسم A الى B`), move, copy, undo (`تراجع`), weather (`كيف الطقس في X؟`), help (`مساعدة`), a few greetings.
- Command words are matched after folding spelling variants: diacritics, tatweel, alef forms (أإآ), ى/ي, ة/ه, ؤ/ئ, Arabic-Indic digits.
  Folding is for matching ONLY. Names, places and topics are passed through byte for byte (a test checks diacritics and digits inside a name survive).
- The normal path then applies its own rules. For example, creating a plain folder in the home area needs no card in English either, and delete shows the card.
- Questions, negations and how-to phrases never run anything (`كيف احذف ملف؟`, `لا تحذف ...`, `هل ...`). They get a short Arabic/English "nothing was run" reply.
- Fail closed: a qualifier or place Pawline does not know before a name marker (`احذف الملف القديم اللي اسمه x`, `اعمل مجلد في مكان غريب باسم x`) is "not understood", never dropped.
- A place after "في" is split from the name only when it is a known place word; `صور في العيد` stays one name.
- Typos: only the first (command) word, only edit distance 1, only when exactly one command word is that close and the rest then parses. It is a suggestion (click to pick), never run directly, and names are never corrected.
- Weather: an Arabic place is looked up with language=ar, because the geocoder returned nothing for Arabic names with language=en (checked live 2026-11-26 for القاهرة, الرياض, دبي). The place text is not rewritten: `القاهره` (ه) found nothing live and says "could not find". `الاسكندرية` (without hamza) also found nothing live.

Evidence quality: the tuned list is regression only. The held-out list (12) and harder list (3) were written by the same author after the code, so they are NOT independent evidence. 20 non-request negative controls and 5 fail-closed cases all stay un-run.

Not done / limits (item 11 stays PARTIAL, not counted done):
- Not general Arabic understanding. Only the shapes above. Present-tense/polite forms are limited ("ممكن تحذف ...").
- No dialect coverage claim (Egyptian, Gulf, Levant words are only partly in the lists, unmeasured).
- Replies, confirm cards and errors stay English (except help, greetings, and the "not understood" lines). No Arabic UI strings.
- No Arabic chat (jokes, hints, discussion) beyond greetings. No Arabic presentation commands ("اعمل عرض عن ...").
- Prefix locations ("في المستندات اعمل مجلد ...") are not understood.
- Arabic voice: not implemented or tested. Moonshine's Arabic status is unverified here. Voice transcripts still go to the draft only.
- Arabic typing in the GUI needs a UTF-8 locale in the test harness. Typing uses xdotool in Xvfb.
- Mixed-direction and shaping readability was checked earlier (item 3, X11). Nothing new measured here.

## 11b: status lines (partial)
After an Arabic command, a fixed Arabic line is added: "تم." after a finished action, or "هذا يحتاج تأكيدك قبل أي تغيير." when a confirm card is shown. Everything else in the reply (paths, card text, errors) stays English. This is not Arabic reply support.

## 11c: presentation shapes (partial)
"اعمل/اكتب عرض تقديمي من <file>" becomes "make a presentation from <file>"; "... عن/حول <topic>" becomes "write a presentation about <topic>" (this goes to the normal online disclosure card, nothing is sent before it). File and topic are passed through verbatim. Questions and negations never run. The test list is author-written after the code, not independent. Edit, image, chart and source commands have no Arabic shapes.

## 11d: small Arabic chat (partial)
A fixed set of whole-message Arabic chat inputs gets a fixed Arabic reply: how are you, who are you, what can you do (the help text), and a tell-me-a-joke set of 4 jokes. Deterministic, nothing runs, nothing is read or sent. It is NOT open Arabic conversation: no follow-up memory, no Arabic discussion of actions (a how-to or question still gets the generic "nothing was run" line), no dialect coverage measured, confirm cards and errors stay English.
