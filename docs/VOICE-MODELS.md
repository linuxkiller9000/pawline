# The voice model: what to bundle, and what has been checked

Asked 2026-11-28: bundle the voice model too, and if a trained-from-scratch model
would be better, that is fine. This file separates what is verified today from
what still has to be measured on his machine.

## What Pawline does now

Voice input tries, in order: `PAWLINE_STT_CMD` (a command the user sets),
sherpa-onnx with a Moonshine folder in the models dir, then whisper.cpp with a
ggml file. Nothing is ever downloaded and no audio leaves the computer. As of
HEAD 8dde393 the whisper file can be pointed at directly (Settings row "Voice
model (whisper)", or "use speech model <path>"), ahead of auto-detection.

## Sizes, checked today by HTTP HEAD against the real download URLs

| file | download | licence |
|---|---|---|
| whisper ggml-tiny.en.bin | 77,704,715 B (74.1 MiB) | MIT (whisper.cpp MIT, OpenAI weights MIT) |
| whisper ggml-base.en.bin | 147,964,211 B (141.1 MiB) | MIT |
| whisper ggml-small.en.bin | 487,614,201 B (465 MiB) | MIT |
| sherpa-onnx Moonshine tiny en int8 (.tar.bz2) | 107,600,538 B (102.6 MiB) | MIT (code and English models; legacy non-English models are non-commercial) |
| sherpa-onnx Moonshine base en int8 (.tar.bz2) | 250,807,309 B (239.2 MiB) | MIT |

So bundling whisper tiny.en adds 74 MiB to the download, base.en adds 141 MiB.
With the vision weights (175 + 104 MiB) that puts a fully self-contained build at
roughly 460 MB with tiny.en, or 530 MB with base.en, before compression.

## The trap on his CPU: AVX is not free

His i3 M370 is Westmere: no AVX at all. Two different risks, both real:

1. **whisper.cpp / ggml.** ggml's CMake defaults `GGML_NATIVE=ON` when not
   cross-compiling, which compiles for the build machine with `-march=native`.
   A binary built that way on a modern machine (or an AUR package built on his
   own machine - that one is fine, it is built *for* his CPU) will die with an
   illegal instruction if it is shipped prebuilt. A binary Pawline ships must be
   built with `GGML_NATIVE=OFF` and `GGML_CPU_ALL_VARIANTS=ON`, which is exactly
   what llama.cpp does for the vision side (it ships an `sse42` and a plain
   `x64` variant next to the AVX ones). Verified in ggml's own CMakeLists today.
2. **Moonshine through sherpa-onnx.** The Arch package depends on the
   `onnxruntime` package, and onnxruntime's x86-64 baseline is not something we
   control. It may or may not need AVX. This is unverified and matters, because
   a SIGILL crash looks like "Pawline is broken" to him.

Cheap test he can run on the Dell (nothing to install, no audio recorded):

    sherpa-onnx-offline --help >/dev/null && echo "sherpa ok" || echo "sherpa crashed"
    whisper-cli --help >/dev/null && echo "whisper ok" || echo "whisper crashed"

A crash there is the answer, and it is worth knowing before any of this is
bundled.

## What has NOT been measured yet

Nothing about speed or accuracy on his machine, and nothing about speed here
either: no whisper or sherpa binary exists in this sandbox, and building one
takes longer than a single work run. The next measurement pass is:

1. Build whisper.cpp here with `GGML_NATIVE=OFF`, and separately with
   `-march=westmere` to stand in for his CPU.
2. Same 3-5 short command utterances (a few seconds each, synthesised once and
   reused) through tiny.en and base.en, one thread, and record wall time and the
   transcript.
3. Compare against Moonshine tiny through sherpa-onnx if it can be built here
   without AVX.
4. Put the table in this file with the machine's `nproc` and CPU model, labelled
   sandbox, and only then decide what to bundle.

Until that exists, any claim that one is faster on his CPU is a guess.

## Training one from scratch

Not viable and not worth trying: a 2-core Westmere with 6 GB cannot train a
usable recogniser; even fine-tuning a small model there is days of compute, and
the result would be worse than MIT models that already run in well under a
second per short utterance on modern hardware. If accuracy ever becomes the
problem, the sensible move is a smaller or distilled existing model, not a new
one.

## Recommendation (to confirm with measurements)

Bundle whisper tiny.en (74 MiB) built AVX-free as the default speech model,
because it is the smallest MIT model that needs no extra runtime package, and it
is the same engine family as the vision side. Keep Moonshine as the alternative
for anyone who already has sherpa-onnx installed, which is what Pawline already
supports today.

## Status

Research only. No voice model is bundled and no code changed for this file.
