# Presentations (item 9): what "make a presentation from <file>" does

Local, deterministic, no AI, nothing sent anywhere. The source file is never changed; the .pptx is a new file next to it, shown in a confirm card first.

- Outline with `#`/`##` headings and `-` bullets: used as written.
- Plain notes without headings: first short line = title, each blank-line paragraph = one slide, title = its first words (max about 7 words / 38 characters, "..." when cut), bullets = its sentences (or its lines when it starts with a short heading line). A source with only one paragraph is refused as too short.
- Dense slides are split, not refused or shrunk: more than 6 lines or 420 characters continue on "Title (2/3)". Sub-bullets stay with their parent. Lines over 160 characters are cut at sentence ends; one sentence over 320 characters has no safe cut and is refused. Every word of the source is kept.
- 5 or more slides with 4+ section titles get a "Contents" slide (original titles only).
- Body type size (32 down to 18 pt) comes from an estimate of the wrapped text height. It is an estimate: viewers wrap differently. Autofit is off, so a viewer will not shrink text on its own.
- Arabic/Hebrew paragraphs are marked right-to-left.

Not done: no images, charts, tables, speaker notes, themes or per-slide layouts beyond title + bullets; titles from first words can read awkwardly; no rewriting or summarising (by design); no reading of .docx/.pdf; not tested in PowerPoint (rendered with LibreOffice 7.x/DejaVu Sans in the sandbox only).

## Writing a deck from a topic (online, reviewed)

"write a presentation about <topic>" uses the same free online model path as online questions (Pollinations text, no account), never the on-device Needle model (that one only turns requests into tool calls and cannot write prose).
1. A disclosure card shows the exact request text, that your IP address goes with it, that no files/history/location are sent, and that the reply is AI-written and unchecked. Nothing is sent until you confirm (Do it needs the card scrolled to the end). Topics up to 200 characters; secrets are blocked.
2. The reply is cleaned (preamble, code fences, bold, "Slide 3:" and numbering removed, nothing added) and shown as a DRAFT in the chat, every slide and bullet. Nothing is saved. It is not written to disk by Pawline (task memory never records it): it lives in memory and in the chat window until Pawline is closed, and the next request replaces it. A request that fails, is cancelled or is overtaken leaves no savable draft. Esc stops the wait and discards a late reply but cannot unsend a request already made.
3. "save the presentation" opens a second card with the full file path and every slide. Only confirming it writes the .pptx (new file, never replaces, undoable).
Limits: facts come from the model and are NOT verified or grounded in sources; prompt asks for general facts only, no invented numbers/names, but that cannot be guaranteed. Needs internet and Pollinations availability (free limit, one request per 45 s); fails with a plain message and no file. You cannot edit the draft in the app yet; edit the saved .pptx.
Titles from your own paragraphs: a short first sentence becomes the title and is not repeated; otherwise the first clause (cut at a comma) or the first words are used and the sentence stays whole in the body, so some titles still repeat the start of the first bullet.
