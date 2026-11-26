# Presentations (item 9): what "make a presentation from <file>" does

Local, deterministic, no AI, nothing sent anywhere. The source file is never changed; the .pptx is a new file next to it, shown in a confirm card first.

- Outline with `#`/`##` headings and `-` bullets: used as written.
- Plain notes without headings: first short line = title, each blank-line paragraph = one slide, title = its first words (max about 7 words / 38 characters, "..." when cut), bullets = its sentences (or its lines when it starts with a short heading line). A source with only one paragraph is refused as too short.
- Dense slides are split, not refused or shrunk: more than 6 lines or 420 characters continue on "Title (2/3)". Sub-bullets stay with their parent. Lines over 160 characters are cut at sentence ends; one sentence over 320 characters has no safe cut and is refused. Every word of the source is kept.
- 5 or more slides with 4+ section titles get a "Contents" slide (original titles only).
- Body type size (32 down to 18 pt) comes from an estimate of the wrapped text height. It is an estimate: viewers wrap differently. Autofit is off, so a viewer will not shrink text on its own.
- Arabic/Hebrew paragraphs are marked right-to-left.

Not done: no images, charts, tables, speaker notes, themes or per-slide layouts beyond title + bullets; titles from first words can read awkwardly; no rewriting or summarising (by design); no reading of .docx/.pdf; not tested in PowerPoint (rendered with LibreOffice 7.x/DejaVu Sans in the sandbox only).
