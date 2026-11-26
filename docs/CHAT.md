# Conversation (priority item 9)

Status: PARTIAL. Not counted as done in the checklist percentage.

Offline (pawline-core/src/chat.rs): English only. Jokes, hints, small talk, "forget our chat". Discussing an action never acts.

Online chat (opt-in, per session):
- "chat online" turns it on for this session only (never saved). "chat off" or "forget our chat" clears the lines.
- Commands and offline help always run first. Only text the command parser rejects can become a chat turn.
- Every turn shows a confirm card with the EXACT messages that will be POSTed to https://text.pollinations.ai/openai (system line, up to 6 earlier lines cut to 300 characters, the new line). Cancel sends nothing.
- Not offered at all: credential words (password, pin, card, bank, login...), paths, emails/URLs, 5+ digit runs, real file names (decimals, e.g. and quotes are fine), backticks, an action word in the first 3 words together with a file/folder/app target, lines over 300 characters. Pronouns, names and plain words like "file" are allowed because the card shows the exact text first.
- Earlier lines are recorded only after a successful, confirmed turn. Memory only.
- Replies are labelled AI-written, unchecked, shown only, never run.
- The card shows the JSON fields (model, messages) and each message word for word; the real body is that JSON, nothing else.
- Privacy facts shown on the card come from https://pollinations.ai/privacy (updated 2026-11-17): prompts processed transiently, no training without opt-in, replies may be cached up to ~30 days, free usage logs a keyed IP hash + network prefix + country, users 16+, data may reach service providers. It does not say whether POST bodies are cached differently; Pawline claims nothing beyond that.
- Provider facts (checked 2026-11-26, github.com/pollinations/pollinations APIDOCS.md): free anonymous POST /openai, one request per 45 s per IP, 16,000 character prompt limit, "best effort", subject to their Terms and Privacy Policy (not read in full here).
- Live check in the real app (Xvfb, debug build): two turns, the second follow-up ("Why so many?") was answered in context. A second send inside 45 s is stopped by Pawline with nothing sent. A plain curl POST with context returned HTTP 200 and the reply used the earlier line.

Window title shows [online chat ON] while it is on.

Known gaps: history persistence is item 15/19 and is not built; English only.
