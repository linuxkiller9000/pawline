# Conversation (priority item 9)

Status: PARTIAL. Not counted as done in the checklist percentage.

Offline (pawline-core/src/chat.rs): English only. Jokes, hints, small talk, "forget our chat". Discussing an action never acts.

Online chat (opt-in, per session):
- "chat online" turns it on for this session only (never saved). "chat off" or "forget our chat" clears the lines.
- Commands and offline help always run first. Only text the command parser rejects can become a chat turn.
- Every turn shows a confirm card with the EXACT messages that will be POSTed to https://text.pollinations.ai/openai (system line, up to 6 earlier lines cut to 300 characters, the new line). Cancel sends nothing.
- Not offered at all: secrets, paths, addresses, 5+ digit numbers, file names, quotes, local action words, lines over 300 characters.
- Earlier lines are recorded only after a successful, confirmed turn. Memory only.
- Replies are labelled AI-written, unchecked, shown only, never run.
- Provider facts (checked 2026-11-26, github.com/pollinations/pollinations APIDOCS.md): free anonymous POST /openai, one request per 45 s per IP, 16,000 character prompt limit, "best effort", subject to their Terms and Privacy Policy (not read in full here).
- Live check: a plain curl POST with context returned HTTP 200 and the reply used the earlier line.

Known gaps: no on-screen "online chat is on" indicator outside the messages; history persistence is item 15/19 and is not built; English only.
