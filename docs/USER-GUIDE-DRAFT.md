# Pawline user guide (DRAFT, item 25 prep, PARTIAL)

Status: written from code inspection at the integrated successor line. Not a release document. Items marked UNVERIFIED have not been run on the real target.

## Where Pawline keeps files (inventory verified by a live file-diff run)
Data folder: $PAWLINE_DATA, else $XDG_DATA_HOME/pawline, else ~/.local/share/pawline (Windows: %LOCALAPPDATA%\pawline, Windows UNVERIFIED, never run).
Files the code writes there (verified 2026-11-26 by running the app with an empty temporary data folder and exercising features, Xvfb debug build):
- ui-settings.txt: UI settings. First launch writes nothing; this appears with the first settings change.
- frequent-folders.json: folder usage counts (written after a folder action).
- task-memory.json: steps-only task memory (no request text, hashed key). Written only after a success.
- search-memory.json: remembered searches (not exercised in the diff run; written on search use).
- history.txt: chat history. Written ONLY if the user turns history on (default OFF), and only what is submitted from then on. Switching it off deletes the file.
- history-named-<name>.txt: a conversation you named and switched away from ("name this conversation X", "switch to conversation X"). Only with history on.
- history-archived-<number>.txt: a dead copy made when you say "new conversation". Only with history on.
- current-conversation: which named conversation is live (one line). Only with history on.
All four are plain text, not encrypted, readable by anyone who can read your account files. Turning history OFF, or "delete our saved conversation", removes every one of them. "list conversations", "search history for X" and "export history" read them; export asks first and writes one new file where you say.
- models/ (you put model folders here yourself, e.g. moonshine-*, ggml .bin). Pawline downloads nothing for voice. Also sherpa/ if you install sherpa-onnx there.
Working folder ($PAWLINE_ROOT, else your home folder) has its own Pawline area at .local/share/pawline/ (by design: the undo journal and trash must stay on the same filesystem as the files they track):
- journal.jsonl: the undo journal (write-ahead records of file changes).
- trash/: what Pawline deleted, for undo.
- wallpapers/: copies made when setting wallpapers.
With default settings the data folder and this area are the same place (~/.local/share/pawline); they differ only if you set PAWLINE_DATA or XDG_DATA_HOME elsewhere.
Runtime: $XDG_RUNTIME_DIR/pawline-swaybg.pid while a wallpaper helper runs.
Not in this inventory, check before claiming: caches or model files created by the AI engine library, desktop entries, autostart, Start Menu entries. The installer does not exist, so none are created by us.
Files the user asks Pawline to create or move (documents, folders, decks) are the user's and are NOT removed by uninstall.

## Clean uninstall (Linux)
1. Quit Pawline (and its wallpaper helper if running).
2. Delete the binary you installed.
3. Delete the data folder above. This removes memory, settings, history and any models you placed there. Move models out first if you want to keep them.
4. Delete .local/share/pawline/ inside your working folder (the undo journal, the Pawline trash and wallpaper copies). Check trash/ first if you want anything back.
5. Nothing else is written by Pawline itself. Verified by a sandbox before/after file diff with temporary folders (2026-11-26); a real-install diff is still owed before release.

## Build
cargo build --release -p pawline-native (Linux, needs the native libraries of the AI engine; see README.md). The sandbox build needed pkg-config paths and a static engine library; exact environment is in docs/WAYLAND-SMOKE.md and the shipping notes. Windows: cross-build check only, never run.

## Settings (the 12 rows, defaults first)
Theme (Dusk/Ocean/Forest/Rose/Contrast - Contrast is the high-contrast one), Colours drift (on), Reduced motion (off; pauses drift whatever its switch says), Speak replies aloud (off), Text size (100/125/150%), Offline mode (off; Pawline's own online requests only, not apps you open), Microphone (allowed; BLOCKED records nothing), Keep conversation on this computer (OFF), AI suggestions (on; the small embedded local model, never the network), Speech rate (normal), File changes (allowed; BLOCKED = read-only, no card and no change), Voice (engine default; espeak voices only). The settings file holds only these switches, no message text. "reset settings" restores text size, theme, drift, spoken replies and voice; it does not touch offline mode, the microphone block, the saved-conversation switch, reduced motion or the file-changes block, and it deletes nothing.
Keyboard: Ctrl+, opens and closes Settings (Up/Down choose, Enter applies). Ctrl+C copies the last reply (clicking a reply copies it), Ctrl+V pastes, Ctrl+R replies to a message.

## Offline
The app works without a network for local features. Anything online asks first with a disclosure card. The software offline gate is not a firewall. Per-route privacy statements are in docs/SECURITY-AUDIT.md; not a blanket claim.

## Platforms and limits
Verified: headless sway/Wayland and an X11 virtual display in the sandbox. UNVERIFIED: KDE, XFCE, niri, real GPU, fractional scale, multi-monitor, orb drag, paste into Pawline, Windows, and the user's N5010.
Dependency warnings visible (unmaintained, no fix available): rustybuzz 0.20.1 RUSTSEC-2026-0206, ttf-parser 0.25.1 RUSTSEC-2026-0192.

## Troubleshooting (draft)
- No voice input: STT tools/models not installed; Pawline says so and downloads nothing.
- Nothing remembered: history is off by default.
- Wayland input problems: see docs/WAYLAND-SMOKE.md.
