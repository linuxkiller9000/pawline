# 018 search memory: stale and time-sensitive rules
- Logic moved to `searchmem::answer()` (pure, fetch passed in) so offline and stale paths are unit tested without a network.
- Time-sensitive questions (price, stock, availability, hours, weather, news, today, latest, policy, flight, ticket, rate...) are never answered from memory as current: always fetched. A saved copy is shown only if the fetch fails, labelled "may be out of date. Don't rely on it for prices, availability, policies".
- Fresh memory answers now carry the date at the top AND in the last line before "Took" (the card scrolls to the bottom, so the label stays visible).
- Keyword list is a heuristic, not a guarantee. The secret-pattern filter is a safeguard, saved queries may still hold sensitive text; "forget searches" deletes the file.
- Tests: 297 pass, 0 fail (sandbox). Proof (sandbox, Xvfb): repeat "dog" shows trailing "(From memory, saved 2026-11-25.)"; "price of milk" repeated was fetched again, not from memory. Live offline run not done (unit tests cover it).
