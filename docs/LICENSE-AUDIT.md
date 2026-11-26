# License audit (item 21), 2026-11-26

Scope: every non-code asset bundled in the repo/binary, plus statically linked runtimes.
Method: claims in NOTICE / NOTICE-fonts.txt verified against the actual files, upstream sources fetched live.

## Verified

| Asset | Claim | Verification | Result |
|---|---|---|---|
| pawline-needle/needle3.cact (35,335,380 B) | Needle3 weights, Apache-2.0 (huggingface.co/Cactus-Compute/needle3) | Local sha256 c9d915eca282ed42d1a09b143b592adb4cc6744ffe2d294adf5cfc5548170c38 == upstream LFS oid (HF API, 2026-11-26); HF repo tagged license:apache-2.0, carries its own LICENSE | BYTE-IDENTICAL to the genuine Apache-2.0 artifact |
| pawline-needle/libcactus.a | cactus runtime prebuilt, Apache-2.0 | NOTICE documents source + the restricted engine-repo distinction | Documented; binary not independently re-verified (no upstream checksum published) |
| pawline-native/assets/font.ttf | Liberation Sans | fc-scan: family "Liberation Sans"; name table: Copyright 2007 Red Hat, Liberation Fonts license (GPL v2 + exceptions) | True |
| pawline-native/assets/arabic.ttf | Noto Sans Arabic, OFL 1.1 | fc-scan: family "Noto Sans Arabic"; name table: Copyright 2015-2020 Google LLC, OFL 1.1 | True |
| pawline-ui/icons/icon.png | project icon | 32x32 PNG, added in the initial project commit (3fdb32b) | Project-authored, no third-party terms |
| vendored needle-rs | MIT | vendor/needle-rs/LICENSE + NOTICE present and consistent | True |
| static libc++/libc++abi/libunwind | LLVM Apache-2.0 w/ exceptions | ldd target/debug/pawline: no dynamic libc++/unwind (static, as claimed) | Consistent |

## Gaps fixed in this commit

- OFL 1.1 requires the license text to accompany the font: added pawline-native/assets/OFL-NotoSansArabic.txt
  (canonical text from google/fonts ofl/notosansarabic/OFL.txt, copyright line matched to the embedded font's own notice).
- Liberation Fonts license text added: pawline-native/assets/LICENSE-LiberationFonts.txt
  (verbatim from fedoraproject.org/wiki/Licensing/LiberationFontLicense).
- NOTICE-fonts.txt now points to both full texts.
- Apache-2.0 full text added at licenses/Apache-2.0.txt (verbatim from the upstream needle3 repo LICENSE);
  covers needle3.cact and libcactus.a redistribution terms (Apache-2.0 section 4).

## Remaining honest notes

- pawline-native/testdata/t.jpg (660 B, 16x8 JPEG): test fixture added by the author in d51c549; assumed
  project-authored, pixel origin not provable from the repo. Not shipped in the binary.
- GPL-3.0 (Pawline) + Apache-2.0 + OFL 1.1 + Liberation (GPLv2+exceptions) are compatible for this use:
  fonts are embedded as data, not linked code; Apache-2.0 is GPLv3-compatible.
