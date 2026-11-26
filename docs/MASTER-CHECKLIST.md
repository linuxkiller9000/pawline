# Pawline master checklist

Source: the user's Telegram messages of 26 Nov 2026 (03:03 to 03:08, the checklist sections and the "final order" table). The numbers in the
priority table are the execution numbers. The section numbers the user wrote (17 to 28) are topic labels only; they are mapped below.
Every bullet is kept as written. A status line says what exists today. Built partial code is never counted as completing a bullet.
Publishing a release needs the user's separate approval (priority 29). No release, tag or CI is authorized.

Legend: DONE = done for the scope written, not hardware-verified. PARTIAL = some of it exists. OPEN = not started.
"Runtime gate" = needs hardware or a machine that is not available in the build sandbox, so it stays open until actually run there.

## Priority order (execution numbers)

| Priority | Work | Topic label in the user's sections | Status |
| --- | --- | --- | --- |
| 1 | GitHub source sync | (earlier list, old 1) | DONE: work is on feature branches, remote main untouched, no release |
| 2 | Core action safety | (old 2) | DONE for tested Linux file operations |
| 3 | Arabic/RTL display | (old 3) | DONE in the checked X11 build |
| 4 | Composer, folder picker, Markdown, basic themes | (old 4) | DONE |
| 5 | Voice implementation | (old 5) | DONE as code, virtual-audio tested only |
| 6 | Weather and Q&A | (old 6) | DONE in English |
| 7 | Basic presentations | (old 9) | DONE as a checkpoint |
| 8 | Typo tolerance | new | PARTIAL: command, weather, search and question words (docs/TYPOS.md) |
| 9 | Conversation, hints and jokes | new | DONE for session-only scope (accepted by main 3:35): offline chat, hints, jokes, opt-in online chat with exact-text card. Separate: persistent history (15/19), Arabic (11) |
| 10 | Presentation extras | new | PARTIAL: themes done (theme: line, 6 themes, contrast-tested, LibreOffice render checked); tables done (| a | b | lines, native table, limits 8x6, ragged/oversize refused, RTL cells, LibreOffice render checked); local images, draft editing, charts, fact-check support, non-LibreOffice check OPEN |
| 11 | Arabic functionality | new | OPEN |
| 12 | Voice/audio completion | new | OPEN (runtime gates) |
| 13 | Desktop polish and accessibility | section 25 + extras | OPEN |
| 14 | Privacy and permissions | section 17 | OPEN (parts exist, see below) |
| 15 | Failure recovery | section 18 | OPEN (parts exist) |
| 16 | AI reliability | section 19 | OPEN (parts exist) |
| 17 | Filesystem safety stress testing | section 20 | OPEN (parts exist) |
| 18 | Performance and resource management | section 21 | OPEN |
| 19 | Conversation management | section 22 | OPEN |
| 20 | Settings and customization | section 23 | OPEN |
| 21 | Security audit | section 26 | OPEN |
| 22 | Windows compatibility | old 8 | PARTIAL: builds, never run (runtime gate) |
| 23 | Linux desktop verification | old 7 | PARTIAL: XFCE and KDE X11 headless; niri and KDE Wayland open (runtime gate) |
| 24 | Update and maintenance | section 24 | OPEN |
| 25 | Documentation | section 27 | OPEN (drafted alongside) |
| 26 | Real-world testing | section 28 | OPEN (runtime gate: actual N5010) |
| 27 | Final integration | old 10 | OPEN |
| 28 | Packaging | old 10 | OPEN (no installer or signing is authorized) |
| 29 | Approval before release | | The user approves; nothing is published before that |

Carried from earlier scope (kept, not dropped): panel and multi-monitor limits (X11 work area read at place time; Windows primary monitor only),
Moonshine on the older CPU, real espeak intelligibility, Arabic voice and Arabic weather, the weather chooser using typed numbers instead of chips,
the voice-bar overlap in the chat window, presentation limits (no in-app editing, no images, charts, tables or themes yet, AI text not fact-checked,
not opened in PowerPoint), main-branch divergence reconciliation (belongs to priority 27).

## 13. Desktop polish and accessibility (user section 25: Accessibility, plus earlier extras)
Earlier extras: multiple monitors, reacting to panel changes, theme performance, reduced motion.
- Keyboard-only navigation.
- Screen-reader-friendly elements.
- Text scaling.
- Contrast options.
- Reduced-motion option.
- Focus indicators.
Status: OPEN. Existing: keyboard flow for cards (Cancel focused by default), focus ring on the card buttons.

## 14. Privacy and permissions (user section 17)
- Clear permission controls for microphone, files, internet, and AI.
- Show exactly what text will be sent online before sending.
- Ability to revoke permissions.
- No sending personal files or private information without explicit approval.
- Offline mode that genuinely disables network requests.
Status: OPEN. Existing: every online feature (questions, weather, search, presentation writing) shows a disclosure card with the text first and
sends nothing before Do it; secret-looking text is blocked with no approval path; files and chat history are never attached. Missing: a permission
screen, revocation, one offline switch that disables every network path (needs a single network gate and a test that proves zero requests).

## 15. Failure recovery (user section 18)
- If an operation fails halfway through, recover gracefully.
- Don't leave partially created folders/files without explaining.
- Undo should survive application restarts where possible.
- Recover from crashes without losing conversation or pending tasks.
Status: OPEN. Existing: an on-disk undo journal with recovery notes at start-up; no-replace atomic publish; cancel cleanup of our own temp files.
Scope conflict to settle with the user: "recover conversation and pending tasks after a crash" needs conversation text on disk, while the current
designs promise memory only (chat lines, presentation drafts). Plan: off by default, an explicit setting to keep the conversation locally, accurate
disclosure of what is stored where, a delete control, and the draft/private text is never saved silently.

## 16. AI reliability (user section 19)
- Don't pretend to know something it doesn't.
- Distinguish verified facts from AI-generated answers.
- Ask for clarification when a command has multiple interpretations.
- Never let AI bypass the safety validator.
- Handle model failures, timeouts, and corrupted model data gracefully.
Status: OPEN. Existing: model/online text is labelled and shown as data; generated text cannot choose a save path or run a tool (tested); assist-derived
plans go through the same validator and confirmation; "Did you mean" and folder choosers for ambiguity; online failures give plain messages.

## 17. Filesystem safety stress testing (user section 20)
- Test paths containing Arabic, spaces, emojis, and unusual characters.
- Test symbolic links, permissions, read-only folders, and inaccessible locations.
- Test duplicate filenames and overwrite conflicts.
- Ensure it cannot accidentally escape its allowed directories.
- Test undo after files have been externally modified.
Status: OPEN. Existing: home confinement through handle-relative operations, no-replace publish, undo fingerprint checks; part of this is covered by tests, no full matrix yet.

## 18. Performance and resource management (user section 21)
- Measure RAM, CPU, startup time, idle usage, and response time.
- Avoid freezing the UI during AI inference or file operations.
- Stop unnecessary background processes.
- Handle low-memory situations gracefully.
- Test extended sessions to detect memory leaks.
Status: OPEN. No measured numbers exist yet; none will be claimed before they are measured (sandbox numbers are not the N5010's).

## 19. Conversation management (user section 22)
- New conversation / clear conversation.
- Conversation history persistence.
- Ability to forget or delete conversations.
- Context limits and handling of long conversations.
- Prevent old context from causing unintended actions in new conversations.
Status: OPEN, partly started in priority 9: a few recent lines are kept in memory only, "forget our chat" clears them, and conversation context never
creates an action (a polite request becomes a pick that still needs sending and a confirm card). Persistence needs the explicit controls above.

## 20. Settings and customization (user section 23)
- Proper settings screen.
- Adjustable voice, language, themes, animations, and AI behavior.
- Model management: download, remove, update, verify integrity.
- Restore default settings.
- Settings survive updates.
Status: OPEN. Existing: a small settings file (ui-settings.txt) and a theme choice; no model management or integrity verification yet.

## 21. Security audit (user section 26)
- Review every file operation and permission boundary.
- Review network requests.
- Check dependencies and their licenses.
- Ensure model output cannot execute arbitrary code.
- Check for accidental logging of private data.
Status: OPEN. The audit will report tested scope and residual limits, not a broad safety claim.

## 22. Windows compatibility (old 8)
Status: PARTIAL. A Windows executable builds and links (cross-compiled, debug); it has never run on Windows. See docs/WINDOWS-PORT.md. Runtime gate.

## 23. Linux desktop verification (old 7)
Status: PARTIAL. XFCE and KDE X11 (headless) checked. niri and KDE Wayland, the Wayland layer-shell path and a physical desktop are open. Runtime gate.

## 24. Update and maintenance (user section 24)
- Version checking.
- Safe update mechanism.
- Preserve user settings and data during updates.
- Rollback if an update breaks the app.
- Clear changelog and version information.
Status: OPEN. No update server exists; any version check is a network request and must follow the permission rules in 14. No signing or release commitment.

## 25. Documentation (user section 27)
- Installation guide.
- Build instructions.
- Offline functionality explained.
- Supported platforms and known limitations.
- Troubleshooting guide.
- How to uninstall cleanly without leaving unwanted files.
Status: OPEN, drafted alongside the features. The uninstall guide is written from a verified inventory of the files, folders, models and settings Pawline
really creates; user-created documents are kept by default.

## 26. Real-world testing (user section 28)
- Test normal usage, not just successful demo cases.
- Test incorrect commands and ambiguous requests.
- Test long sessions.
- Test with internet disconnected.
- Test after reboot.
- Test on your actual N5010, not just virtual machines.
Status: OPEN. Sandbox and virtual-machine runs are labelled as such and do not count as the N5010 test. Runtime gate.

## 27. Final integration, 28. Packaging, 29. Approval before release
OPEN. Integration also reconciles the diverged main branch (not merged without checking the repo authority for that operation). Packaging makes no installer,
signing or release commitment without approval. Publishing needs the user's explicit approval.


## Tracked gaps (file safety, item 17)
- Rename with a quoted name containing spaces (rename the folder "a b" to c) fails: the quotes become part of the name. Delete/remove with quoted names works since 2026-11-26.

## Tracked cross-item notes (from main, 3:35)
- 14 privacy: claims for the earlier GET routes (Q&A, deck draft, search) and the new POST chat route must be stated per route, not generalized from one. The privacy facts on the chat card come from pollinations.ai/privacy (2026-11-17) only.
- 18 performance: debug-build GUI typing latency is slow (long input redraw); measure and fix, no performance claim yet.
- 21/25: provider terms and privacy pages must be read in full before release; the 2026-11-26 read was a skim (no full legal review claimed).
