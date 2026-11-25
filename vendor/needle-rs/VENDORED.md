# Vendored: needle-rs

Source: https://github.com/Geekgineer/needle-rs  commit 4de50494fd60f417b24c37e4d972f95d128f8a0f
Crates copied (src/ only, unmodified): needle-core, needle-infer.
License: MIT (see LICENSE). Copyright (c) 2026 Abdalrahman Ibrahim.
Only the Cargo.toml manifests are rewritten (workspace inheritance removed,
benches/dev-dependencies dropped). The upstream author states the code is an
independent re-implementation of Cactus's Needle written from the published
Apache-2.0 sources and paper, with no upstream code copied (see NOTICE).
That is the author's statement; Pawline has not independently verified it.
Needle weights remain Apache-2.0 (Cactus Compute).

## Pawline patches (the only edits to upstream src)
- needle-core/src/v3/cache.rs: `#[derive(Clone)]` on `LayerCache` and `V3Cache`.
- needle-infer/src/v3_engine.rs: `PrefixState`, `make_prefix`, `generate_prefixed`,
  and a private `generate_inner` (the old `generate_with` body, with an optional
  cloned prefix cache; suffix tokens are stepped with `decode_step`).
  Checked bit-identical to full prefill (pawline-needle-native/examples/prefix_test.rs).
  Falls back to the unpatched full-prefill path when the prefix does not match.
