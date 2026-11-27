# Experiments and engine research (November 2026)

This branch (`work/llm-vision-experiments`) collects engine research, benchmarks
and the pieces that came out of them. Everything here is a work branch: no
stable release, no force-push, free services only, attribution kept.

## 1. llm.asm evaluation (can it be Pawline's engine? No.)

Source: https://github.com/alesha-pro/llm.asm (MIT, first published 2026-11-25).
Credit and thanks to the author; the license is kept and this note is the
attribution. We evaluated it, we did not vendor it.

What it is: a from-scratch x86-64 assembly LLM (Qwen3-0.6B hardwired), with a
matching trainer and math kernel tests. We built all three binaries in the
sandbox (sizes match its README), ran its kernel tests (pass), ran real
Qwen3-0.6B inference (2.3 tok/s in the sandbox), and ran a training smoke test.

Why it cannot be Pawline's engine:

- The Linux binaries require AVX2 + FMA + F16C with no runtime dispatch. On the
  target Dell (i3-M370, SSE4.2, no AVX) they die with SIGILL at startup.
- The model is hardwired (Qwen3-0.6B only); Pawline needs provider freedom.
- It is a CLI, not a library; embedding would mean forking its build.

What we took: the confirmation that a tiny model is usable for Pawline-scale
tasks, and the push to benchmark our own fallback paths (next section). The
patched local export script (streaming weight export, math untouched) stayed
in the sandbox; nothing was pushed to their repo.

Side findings: the vendored libcactus static archive is also AVX-heavy (466
AVX instruction sites, no dispatch). needle-rs (our other vendored engine)
has proper CPUID dispatch with a scalar fallback, so it is Dell-safe.

## 2. SIMD: the Dell does not need hand-written SSE4.2 kernels (measured)

Question: needle-rs's fast path is AVX2; the Dell has only SSE4.2. Do we need
to write SSE4.2 kernels?

Method: `NEEDLE_SIMD=off` environment hook in `quant::has_avx2()` forces the
portable fallback on AVX2 hardware (std builds only; committed). The bench
example grew slicing flags (config prefix argument, `BENCH_MAX_NEW` cap,
tok/s printout) so slow machines can measure one slice at a time.

Numbers (sandbox, release, needle3 cact, 51-63 new tokens, AVX2 vs forced
fallback):

| config   | AVX2 tok/s | fallback tok/s |
|----------|-----------|----------------|
| d20 f32  | 3.25      | 3.46           |
| d20 int8 | 3.22      | 3.45           |
| d12 f32  | 6.00      | 6.40           |

Confidence values are identical in both modes, so the dispatch does not
change numerics. The kernel is the same source recompiled wider, and the hot
loop streams packed nibbles from RAM: it is bandwidth-bound, not
SIMD-width-bound. Writing SSE4.2 kernels would gain nothing measurable, so we
did not write them. The deciding measurement for the Dell itself is
`sh tools/dell_bench.sh` (one command; prints CPU flags, dispatch decision,
and per-config tok/s into dell-bench-results.txt).

## 3. Local LLM chat (shipped on this branch)

Provider-agnostic chat: Pollinations (default, unchanged), Ollama, llama.cpp,
LM Studio, or any OpenAI-compatible endpoint. Config at
$XDG_DATA_HOME/pawline/chat-provider.json (owner-only, atomic), API keys only
ever in the Authorization header, never typed into chat. The confirm card
names the actual destination; the executor re-reads the config and refuses if
the provider changed after confirmation. Commands: "chat provider",
"use ollama model X", "use llama.cpp model X", "use lm studio model X",
"use pollinations". Verified end-to-end against a mock OpenAI-compatible
server (tools/mock_llm.py): request shape, model field, auth header
presence/absence, error text.

## 4. Vision model choice: SmolVLM-256M (the default and only one)

Requirement: a free vision model that runs on the Dell's CPU.

- SmolVLM-256M-Instruct (Apache-2.0) - CHOSEN. Smallest multimodal model that
  exists; ~2.9 GB RAM fp32 on CPU (measured by third parties), much less
  quantized; official ONNX and official ggml-org GGUF builds.
- Moondream2 (Apache-2.0) - rejected: 1.86B params, ~2 GB int8 on disk; too
  heavy next to the OS on a 6 GB machine.
- Florence-2-base (MIT) - rejected: a task-token captioner (caption/OCR
  prompts), not a conversational answerer.
- Native tract port of the SmolVLM ONNX exports - tested and rejected: the
  official exports need com.microsoft contrib ops (60 RotaryEmbedding nodes
  plus GroupQueryAttention carrying the KV-cache attention) that tract 0.23
  has no translator for; the vision encoder also fails shape analysis on a
  symbolic Flatten. Decomposing contrib attention by hand is a multi-hour
  rewrite with real wrong-result risk, for zero user-visible gain over the
  route below.

How it ships: the vision model runs on the user's local runner (llama.cpp
with the official ggml-org SmolVLM-256M Q8_0 GGUF + its mmproj file) and
Pawline talks to it through the same provider layer as chat: say
"look at this picture <path>", confirm the card (it names the file, the size
cap and the destination), and the image goes only to the configured provider.
With a loopback provider nothing leaves the machine.

## 4b. Larger-context / image-tiling check (live, llama.cpp b12811)

Same provider path as section 4, server started with -c 4096. Flat-color
test images (red/blue/green/yellow quadrants, white circle, black triangle,
white square) at three sizes, temperature 0.0:

| image | prompt tokens (image share) | wall (1-thread sandbox) | answer quality |
|-------|------------------------------|--------------------------|----------------|
| 256 px | 149 (~120) | 9.4 s | correct: "a blue square" |
| 1024 px | 380 (~350) | 24.2 s (23.2 s prompt) | 3/4 quadrant colors (yellow read as black, likely the black divider lines); black triangle correctly located top-right; white circle/square missed |
| 2048 px | 1151 (~1120) | 80.8 s | same colors answer plus one hallucinated detail ("labeled home") |

Findings:
- llama.cpp tiling works through the provider layer unchanged: image tokens
  scale ~120 -> ~350 -> ~1120 across 256/1024/2048 px. A 2048 px image plus
  chat history plus the 1200-char description cap still fits in -c 4096.
- The quality ceiling on multi-part spatial questions is the 256M model
  itself, not the transport. Simple one-sentence descriptions are solid;
  quadrant-level attribution is unreliable.
- Sandbox timings are a 1-thread VM floor, not Dell numbers.

## 4c. SmolVLM-500M comparison (live, same harness, temperature 0.0)

Same probes re-run on ggml-org SmolVLM-500M-Instruct Q8_0 (model 437 MB,
mmproj 109 MB vs 175 MB / 104 MB for the 256M). Identical token counts
(same tiling), ~25% slower wall on the 1-thread sandbox:

| probe | 256M answer | 500M answer |
|-------|-------------|-------------|
| 256px simple description (9.4s / 11.2s) | "a blue square" (background missed) | "a single blue square as its background" (also imperfect) |
| 1024px quadrant colors (24.2s / 30.4s) | 3/4 colors (yellow read as black), 1/3 shapes | 4/4 colors, all 3 shapes found and placed (square read as "rectangle"); one repeated-triangle slip |
| 1024px multi-part spatial question (24.4s / 5.0s cached) | vague, wrong | 4/4 colors, 3/4 quadrants fully correct, one hallucinated extra triangle |
| 2048px colors (80.8s / 95.2s) | colors + 1 hallucinated detail | quadrants described correctly up to the 60-token cutoff |

Verdict: the 500M is a straight quality upgrade on spatial/multi-object
questions (the 256M failure mode from 4b) for +370 MB on disk and ~25%
latency on a 1-thread floor. Same llama.cpp drop-in path, same provider
code, free download.

DECIDED (user, 2026-11-27): SmolVLM-500M is the single vision default;
SmolVLM-256M is documented as the lighter option for tight installs.

## 5. Laya

The opt-in Laya intent router (150M) is verified end to end against the real
files:

- Pinned download: model (302,276,803 bytes) and tokenizer (2,132,967 bytes)
  match the sha256 pins in laya_setup.rs exactly.
- Conversion: the in-app converter (pawline-core/examples/laya_convert.rs)
  turns the contrib-op ONNX into a tract-runnable fp32 graph in 12.3 s; the
  result is 96 nodes (~640 MB fp32, loaded once, kept in memory).
- Forward pass: 347 ms on the converted graph, finite logits (sandbox).
- Routing quality (experiments/laya-route-bench): the real converted model,
  the real tokenizer and a faithful port of laya.rs route_inner over a
  22-message battery: 18 intent phrases across 12 intents plus 4 adversarial
  none-cases. Result: 21/22 pass. Every positive phrase routed to the right
  intent at p=0.966-0.982 (threshold 0.80), all four none-cases correctly
  declined except one: "my folder of memories from school" fired make_folder
  at p=0.950. That is a genuine model false positive on an adversarial
  phrase, worth a follow-up (threshold tuning or an extra negative phrasing).
  Known harness limitation: it exercises only the common fits-in-window
  tokenization path, not the over-budget per-phrasing truncation branch.

Threshold tuning (same harness, battery extended to 42 messages: 12 more
paraphrased positives and 8 more adversarial none-cases). At the shipped
thresholds (0.80 / 0.90 destructive): 33/42 with 4 false positives
(p=0.814-0.954, including a delete at 0.954 on "i deleted my ex's
playlist yesterday" - above the old destructive threshold) and 5 false
negatives. Re-scored offline from the logged probabilities: threshold
0.96 for both classes gives 37/42 with ZERO false positives and the same
5 false negatives (lowest true positive is p=0.965, so the margin is
0.005 - revisit with a bigger battery before changing it again). The 5
false negatives are not threshold-fixable: 4 are narrow() family-regex
misses (the model never gets to vote; p=0.000 rows) and 1 is a
model-level miss (none won at 0.969 on "how much is 15 percent of 80").
Follow-up: widen FAMILIES or fall back to all intents on an empty window.
Applied: THRESHOLD and THRESHOLD_DESTRUCTIVE both 0.96 in laya.rs.

Caveat on speed: per-message routing in this sandbox takes 3.5-6 s
(fp32, tract, 2 GB RAM VM). That is a sandbox floor, not a Dell number;
tools/dell_bench.sh is where the real timing comes from. A sub-second Dell
result would make Laya viable as an always-on pre-classifier; a multi-second
result says keep it opt-in.

## Honest labels

All numbers above are sandbox numbers (a 2026 cloud VM, AVX2, 2 GB RAM) unless a row says otherwise. Nothing here was measured on the Dell;
`tools/dell_bench.sh` exists to produce those numbers.
