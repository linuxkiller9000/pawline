# Changelog (DRAFT, successor line, unreleased)
Package version in pawline-native/Cargo.toml is still 0.2.1-beta. No new version number is assigned: that needs the owner's approval.
## Unreleased (successor line)
- Settings screen now shows the running package version (local build-time value, no network).
- Rename parses quoted targets with filler words (rename the folder "a b" to c) and quoted names containing " to " correctly; both used to refuse safely.
- Inputs over 1000 characters skip the local AI assist (was: 30s timeout burn, then self-disable); the parser answers with a note. Terminal mode now honours the persisted offline switch like the GUI.
- Copying a FIFO or other special file is refused up front instead of hanging on a blocking open; moving one stays allowed (rename only).
- History switched on mid-session now keeps only what is submitted from then on; the user guide inventory was verified by a live file-diff run and now lists the working-folder journal/trash/wallpapers area.
- Held-key repeat on Wayland; sctk 0.21 migration (memmap2 advisory gone).
- Chat/history list caps and paging; opt-in history (off by default); reset settings.
- Safer file operations and Windows name rules (Windows never run).
- Wayland smoke scripts and security audit docs.
Known: see docs/MASTER-CHECKLIST.md for open items.
