# 016 search memory (stage 1 of learn-through-use)
- Local JSON file `search-memory.json` (mode 0600) in the Pawline data folder. Holds engine, query, source, text, date, hits.
- 200 entries max, 2000 chars per text. Wikipedia kept 7 days, web 1 day. Secret-like queries are never saved.
- Repeat search shows "From memory (saved <date> from <source>)". "refresh: <request>" fetches again. Offline: stale copy, labelled.
- "search memory" lists it. "forget searches" deletes the file.
- Proof (sandbox, headless sway, x86 host): search, repeat from memory in 0.1s, list, forget removes the file. 291+ tests pass.
- Not proven: Dell, offline stale path in a live run. Stage 2 (steps memory) not built.
