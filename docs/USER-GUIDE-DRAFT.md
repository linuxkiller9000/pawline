# Pawline user guide (DRAFT, item 25 prep, PARTIAL)

Status: written from code inspection at the integrated successor line. Not a release document. Items marked UNVERIFIED have not been run on the real target.

## Where Pawline keeps files (inventory from the source)
Data folder: $PAWLINE_DATA, else $XDG_DATA_HOME/pawline, else ~/.local/share/pawline (Windows: %LOCALAPPDATA%\pawline, Windows UNVERIFIED, never run).
Files the code writes there (names found in pawline-core and pawline-native):
- search-memory.json: remembered searches.
- task-memory.json: steps-only task memory (no request text, hashed key). Written only after a success.
- frequent-folders.json: folder usage counts.
- ui-settings.txt: UI settings.
- history.txt: chat history. Written ONLY if the user turns history on (default OFF).
- models/ (you put model folders here yourself, e.g. moonshine-*, ggml .bin). Pawline downloads nothing for voice. Also sherpa/ if you install sherpa-onnx there.
Runtime: $XDG_RUNTIME_DIR/pawline-swaybg.pid while a wallpaper helper runs.
Not in this inventory, check before claiming: caches or model files created by the AI engine library, desktop entries, autostart, Start Menu entries. The installer does not exist, so none are created by us.
Files the user asks Pawline to create or move (documents, folders, decks) are the user's and are NOT removed by uninstall.

## Clean uninstall (Linux)
1. Quit Pawline (and its wallpaper helper if running).
2. Delete the binary you installed.
3. Delete the data folder above. This removes memory, settings, history and any models you placed there. Move models out first if you want to keep them.
4. Nothing else is written by Pawline itself. UNVERIFIED by an actual before/after file diff on a real install: do that run before release.

## Build
cargo build --release -p pawline-native (Linux, needs the native libraries of the AI engine; see README.md). The sandbox build needed pkg-config paths and a static engine library; exact environment is in docs/WAYLAND-SMOKE.md and the shipping notes. Windows: cross-build check only, never run.

## Offline
The app works without a network for local features. Anything online asks first with a disclosure card. The software offline gate is not a firewall. Per-route privacy statements are in docs/SECURITY-AUDIT.md; not a blanket claim.

## Platforms and limits
Verified: headless sway/Wayland and an X11 virtual display in the sandbox. UNVERIFIED: KDE, XFCE, niri, real GPU, fractional scale, multi-monitor, orb drag, paste into Pawline, Windows, and the user's N5010.
Dependency warnings visible (unmaintained, no fix available): rustybuzz 0.20.1 RUSTSEC-2026-0206, ttf-parser 0.25.1 RUSTSEC-2026-0192.

## Troubleshooting (draft)
- No voice input: STT tools/models not installed; Pawline says so and downloads nothing.
- Nothing remembered: history is off by default.
- Wayland input problems: see docs/WAYLAND-SMOKE.md.
