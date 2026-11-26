# Update and maintenance design (item 24, DESIGN ONLY, nothing implemented)
Facts: no update server exists; the app has no version display (CARGO_PKG_VERSION is not used in the source). No signing or release commitment.
Proposal, each step needs owner approval before implementation:
1. Show version: add the package version to Settings/About text. Local, no network. (Can be done now.)
2. Version check: OFF by default, user-triggered only, shows a disclosure card first (what is sent: nothing but the request itself, no identifiers) per item 14 rules. Needs a hosting location the owner picks; GitHub release page is the only candidate, unconfirmed.
3. Update: never automatic. Download to a temp file, verify a checksum shown to the user, keep the previous binary as pawline.previous next to the new one. Signature verification needs a signing key: owner decision, not assumed.
4. Settings and data live in the data folder (see USER-GUIDE-DRAFT.md) and are never touched by replacing the binary. Add a format version to each data file before any format change; a newer file must be left unread, not overwritten.
5. Rollback: swap pawline.previous back. Test with a deliberately broken new binary.
Not testable here: real updates, Windows replace-while-running.
