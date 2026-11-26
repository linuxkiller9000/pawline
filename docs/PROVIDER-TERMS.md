# Provider terms and privacy pages - full read record (item 14)

Every online route Pawline knows, the exact pages read, when, and what matters for the
disclosure cards. Reading is the developer's, not legal advice. Re-read before any release;
providers change these pages (Pollinations rewrote both of theirs on 2026-11-17).

All routes send a descriptive user-agent: `Pawline/0.1 (personal desktop assistant;
https://github.com/linuxkiller9000/pawline)` (checked in pawline-core, 2026-11-26). That meets
Wikimedia's user-agent policy for API clients.

## Weather: Open-Meteo (geocoding-api.open-meteo.com, api.open-meteo.com)

Read 2026-11-26: https://open-meteo.com/en/terms (combined Terms & Privacy page).

- Free tier is non-commercial only, with rate limits (10,000/day, 5,000/hour, 600/minute per IP
  address and application); they may block applications and IPs that misuse the service without
  notice. Pawline is a free personal app with no ads or subscriptions, which their examples class
  as non-commercial; the per-IP limits are far above anything Pawline sends (one geocode + one
  forecast per request, user-initiated only).
- Data is CC-BY 4.0. The weather reply carries the required credit: "Source: Open-Meteo.com,
  https://open-meteo.com/ (data CC BY 4.0, free non-commercial service)" (weather.rs, tested).
- Privacy: no cookies/tracking on the website; server logs (which can contain the coordinates
  requested) are kept 90 days and not shared; IP addresses are collected for technical reasons
  but "not linked to user identities". GDPR, controller OpenMeteo GmbH, Switzerland.
- What Pawline sends: the place name to geocoding; only the chosen coordinates to forecast. The
  card says this and that Open-Meteo's retention is its own policy - accurate.

## Wikipedia: en.wikipedia.org (read-only)

Read 2026-11-26: https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use and
https://foundation.wikimedia.org/wiki/Policy:Privacy_policy .

- Reading is free without an account; the Terms' conditions that matter here are lawful use and
  not harming their infrastructure (Pawline sends single user-initiated requests with a
  descriptive user-agent, which their API policy asks for).
- Privacy: they collect standard web access data (IP, user-agent) but never sell personal
  information, keep data "the shortest possible time", and reading without an account provides
  no personal information by choice.
- Pawline only reads (search + extracts); it never contributes, so the contribution licensing
  terms do not engage.

## Web search: api.duckduckgo.com (Instant Answer API)

Read 2026-11-26: https://duckduckgo.com/privacy (last updated June 3, 2026).

- "We don't save or share your search, chat, or browsing history": IP addresses are used
  transiently to deliver content and never logged to disk tied to queries; no unique identifiers
  are stored alongside searches; only anonymous aggregate query trends are kept.
- No separate public terms page for the Instant Answer API was found (the general privacy policy
  covers the service); the card already says the result "may be empty or out of date".
- The card's "What DuckDuckGo keeps is its own policy" is accurate and now verified read.

## Online answers, presentations, chat and images: Pollinations "Classic"
## (text.pollinations.ai, image.pollinations.ai - no account)

Read 2026-11-26: https://pollinations.ai/terms and https://pollinations.ai/privacy
(both last updated 2026-11-17; operator Myceli.AI OÜ, Estonia).

- The privacy policy explicitly covers "(d) the Free API Classic at text.pollinations.ai and
  image.pollinations.ai, which needs no account".
- Free Classic usage logging: timestamps, endpoints, request status, IP address and country,
  and the referring app name; usage records keep a keyed hash and the network prefix of the IP,
  not the full address. The IP enforces the free rate limit.
- Prompts/outputs: "processed transiently", NOT used to train models without explicit opt-in
  (section 6). Generated response caches (text, image, audio, video): typically up to 30 days,
  plus any downstream public/browser caches.
- Community models: if a request is served by an externally hosted community model, the request
  goes to that provider and any fallbacks under THEIR terms and privacy policies; Pollinations
  does not control or verify them. This caveat is now in the online-chat card.
- Terms are beta terms: models, limits and features can change or be suspended at any time.
- Correction made 2026-11-26: the online-chat card used to say "the service is for people 16 or
  older". The 16+ rule is in section 15, which covers only the Polli Discord bot, not the
  Classic API; the clause was removed from the card. The same read confirmed the card's
  "cached up to about 30 days" claim ("typically up to 30 days").
- The GET-route cards (presentations, one-shot questions) used to claim "Pollinations says the
  same address returns the same cached answer to anyone who has it". The current policy pages do
  not say that; the cards now state the verified fact (answers can be cached, typically up to
  30 days, so the same address can return a saved answer).

## Images (alternative provider): AI Horde (aihorde.net)

Read 2026-11-26: https://aihorde.net/terms/ (last updated 07 January 2023) and
https://aihorde.net/privacy (last updated 18 Mar 2024).

- Anonymous use is explicitly allowed. Usage Data collected includes the IP address AND the
  prompts used. Generation statistics: kept 30 days, then reduced to daily totals. Kudos
  movement records: 30 days in the database, then archived with their object-storage provider
  "without a fixed end".
- Moderation records (rejected or reported prompts) keep the prompt text, account/proxied
  account, IP address and a keyed-hash pseudonym of the IP; the IP is removed after 30 days
  unless the record is under moderation action.
- Terms are boilerplate (as-is, liability cap 100 EUR, 13+ only, Luxembourg).
- Pawline's Horde card already says "prompts may be logged" - consistent with this read.
