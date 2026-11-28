# Making Pawline independent of an external llama-server

Asked 2026-11-28: "can you make it independent, without llama". This file is the
research result: what the options are, what they cost, and what was measured
before recommending anything.

## What "independent" can mean

1. **No separate server process** - Pawline starts, and the vision and text
   models run inside it. No `llama-server`, no port, no second terminal, no
   second thing to keep alive.
2. **No third-party engine at all** - Pawline's own Rust code runs the model,
   with no C++ library inside it either.
3. **Nothing to install** - (1) plus the model weights shipped in the download,
   so the machine needs no GGUF files at all.

These are different jobs with very different costs. Level 1 is a build change.
Level 2 is writing an inference engine. Level 3 is level 1 plus about 280 MB of
weights in the download.

## What the measurements say (sandbox, 1 vCPU, SmolVLM-256M Q8)

| path | first question | repeat on same screenshot |
|---|---|---|
| `llama-server`, separate process, HTTP | 7.24 s | 0.30 s |
| `llama-mtmd-cli`, separate process, PNG on disk | 7.41 s | 6.8 s (no cache) |
| in-process (same engine, no HTTP, model already loaded) | ~6.8-7.0 s expected | ~0.3 s |

The differences between those rows are **process start (0.38 s of model load,
measured from the CLI's own timeline) and HTTP (a few milliseconds)**. The
question of independence is not a speed question: the 7 s is the vision tower,
two passes of ~3.1 s at one thread, and every option below pays it identically
unless the engine changes.

Two more measured numbers that decide the memory budget on a 6 GB machine:

- SmolVLM-256M Q8 server resident: **420-471 MiB**.
- qwen2.5-0.5b Q8 (the OCR text model) server resident: **760 MiB**.
  Keeping both warm is ~1.2 GB. Loading the text model only when a text
  question arrives, and keeping SmolVLM warm, is the sane default.

## Option A - link llama.cpp into Pawline (recommended)

Vendor llama.cpp's source, build `libllama` + `libmtmd` + ggml statically, and
call them from Rust. Same GGUF files as today, same speed, one binary, no
server, no port.

Facts checked, not assumed:

- **Licence: MIT** (`LICENSE` in the llama.cpp release: "MIT License, Copyright
  (c) 2023-2026 The ggml authors"). Attribution only, same as the repo already
  does for llm.asm.
- **No-AVX CPUs are covered.** The release ships one CPU backend per
  instruction set, including `sse42` and the `x64` floor next to `alderlake`,
  `haswell`, `zen4` and the rest. A single binary can keep that dispatch with
  `GGML_CPU_ALL_VARIANTS`, so the i3-370M picks `sse42` at runtime instead of
  dying on an illegal instruction. This matters: the vendored cactus runtime is
  AVX-heavy and only survives there because it happens to run under emulation in
  our tests.
- **Size**: `libllama` 5.7 MB, `libmtmd` 2.9 MB, `libggml-base` 0.9 MB, all CPU
  variants 21 MB unstripped. Expect the 84.8 MB binary to grow by roughly
  20-30 MB.
- **Build**: C++17, which the project already does (static libc++ for the cactus
  link). CI gains a C++ compile step; the existing `build-native` job already
  installs libc++ and ninja.
- **Bonus**: with the context in-process we own the KV cache, so the prefix
  reuse that today depends on llama-server's `cache_prompt` becomes explicit
  code we control and can test.

Cost: one vendored C++ dependency and its build time. No runtime dependency.

## Option B - a pure-Rust engine (candle or similar)

No C++ and no external process, but on a 2-core Westmere the CPU kernels will
be slower than llama.cpp's hand-written ones, and SmolVLM support means porting
the image preprocessor, the pixel-shuffle and the cross-attention path, then
matching accuracy. It is weeks of work for a slower result, and candle is still
a third-party crate, so it only satisfies level 2 on a technicality.

## Option C - ONNX Runtime

The repo already has a workflow that fetches ORT source. A SmolVLM ONNX export
plus an SSE-only ORT build is more work than A, ships more code, and has no
speed advantage on this CPU. Not recommended.

## Recommendation

Do **A**: llama.cpp in-process, static, CPU variants kept, same GGUF files.
That removes the server, the port, the second terminal and the "is it running?"
failure mode - the actual friction he hit - and keeps every measured number we
have.

Then decide level 3 separately: bundling SmolVLM-256M Q8 (175 MB) plus its
mmproj (104 MB) makes the download about 380 MB and needs no model files on the
machine at all. That is a product call, not an engineering one, so it goes to
him rather than being assumed.

## Interim: Pawline starts the runner itself (implemented)

Until the engine is inside Pawline, Pawline starts `llama-server` itself, so
nothing is launched by hand:

- the local llama.cpp provider plus a vision file the user pointed at is enough
- a server that already answers on that address is never started twice
- the child carries a kernel parent-death signal, so it cannot outlive Pawline
- a missing `llama-server`, or one that exits straight away because the port is
  taken by something else, is reported in one sentence instead of a 90 second wait

Verified in the running app, not only in tests: with the provider pointed at
127.0.0.1:8098 and the real SmolVLM-256M files, a screen question started the
runner (/props answered 200), and two seconds after Pawline exited the same
endpoint answered nothing (000). The module test does the same thing with a real
llama-server: start, answer, no second copy, gone after a signal.

## Status

The managed runner above is implemented and in the app. The in-process engine in
the first half of this file is not: the shipped build still talks to
llama-server over HTTP, it just starts it now.
