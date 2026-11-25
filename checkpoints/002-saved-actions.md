# Checkpoint 002: saved actions

Saved fixed actions: save, run, list, show, delete. Fixed templates, deterministic re-check, drift refusal,
fresh confirmation before every run, case-exact names. Not in the released v0.2.1-beta.

## v1 (frozen): `checkpoint/saved-actions` at commit 1dcdd00
- Earlier test results were reported, but re-running `cargo test -p pawline-core` on exactly 1dcdd00 on 2026-11-25
  (sandbox) FAILED one unit test: `actions::tests::roundtrip_and_no_overwrite`. The test still expected names to
  collide case-insensitively, but names are case-exact since that commit. The product behaviour was the intended one;
  the test was stale. 24 passed, 1 failed in the lib tests, so the integration tests did not run on that commit.
- Treat v1 as: behaviour as designed, one stale failing unit test.

## v2: `checkpoint/saved-actions-v2` at commit 8893a43
- Fix: the test now asserts that "one" twice collides and "ONE" is a distinct name.
- Checked 2026-11-25 15:07 (sandbox): `cargo test --offline -p pawline-core`, all test binaries passed
  (25 lib tests, plus the integration tests including tests/actions.rs).

## Untested limits
- Dell and niri. The terminal and window flows were exercised only in the sandbox.
- Voice cannot dictate saved-action commands (by design); not tested with a real microphone.
