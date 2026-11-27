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

## 4d. Screen-vision probe (synthetic 1920x1080 desktop, live)

Synthetic desktop screenshot (niri-style top bar, terminal with readable
command text, browser window on the Pawline repo page, file window) asked
as "what is on this computer screen?":

| config | wall (1-thread sandbox) | prompt tokens | result |
|--------|--------------------------|---------------|--------|
| 256M, downscaled 960px | 14.3 s | 225 | nearly empty: generic "black screen", hallucinated its own prompt text |
| 256M, full 1920px | 41.8 s | 622 | found only the browser text ("rust is no longer required"); missed terminal, top bar, file window |
| 500M, full 1920px | 56.7 s | 622 | found both windows and the "pawline / local-first assistant" page, but hallucinated usernames/passwords |

User decision (2026-11-27): screen captures use a SEPARATE screen-vision
model setting defaulting to SmolVLM-256M with downscale, for speed. The
measured caveat to surface: at 960px the 256M reads almost nothing; the
downscale floor for usable answers is likely 1280-1920px even on the
256M, and small terminal text stays unreliable on both models. The
setting keeps the default image path on the 500M.

## 4e. OCR hybrid vs vision models on screens (live, sandbox)

Same synthetic 1920x1080 desktop as 4d. Pipeline (experiments/screen-fork/
ocr_screen.py): 2x upscale + grayscale + autocontrast, tesseract (Apache-2.0)
~1 s, then the OCR text alone to qwen2.5-0.5b on llama-server.

| approach | wall | prompt tokens | result |
|----------|------|---------------|--------|
| 256M, full 1920px (from 4d) | 41.8 s | 622 | missed almost everything |
| 500M, full 1920px (from 4d) | 56.7 s | 622 | partial, hallucinated credentials |
| OCR hybrid | ~10 s (1 s OCR + 8.9 s text) | 160 | correct window list (terminal, firefox), correct content (repo URL, "208 passed", Downloads files), no hallucination |

OCR extraction quality: raw tesseract on the untreated image reads almost
nothing; the load-bearing step is the 2x upscale, and the interpolator
matters: Lanczos3 reads small terminal text, bilinear garbles it
(measured side by side). Autocontrast turned out to be a NO-OP in the
winning Python pipeline (the resample already spans 0-255) and a real
stretch reads slightly worse, so the shipped Rust pipeline
(pawline-core/src/screencap.rs prepare_for_ocr) is grayscale + Lanczos3
2x -> PGM, verified to match the Python reference line for line except
one low-contrast green prompt line. Rust preprocess takes 10.6 s in
release for 1920x1080 on the 1-thread sandbox (PIL's C path takes ~1 s).

Boundary test (honest limit): asked "what color is the terminal
background, which window is on top?", the hybrid answered "blue / firefox"
- wrong, and it answered instead of saying the OCR text does not show it.
OCR+text wins on text-content questions and is blind to visual/spatial/
color questions; the system prompt alone does not stop it from guessing.
A production version needs either the vision model as a visual fallback
or explicit refusal training. This is also where a screen-tuned fork
(experiments/screen-fork/RESEARCH.md) could add value over both.

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

Routing-gap fixes (same harness, same 42-message battery). The 4 narrow()
misses were fixed by widening family regexes in laya.rs (mirrored in the
bench): weather gained jacket/snow/wind/windy/fog/storm/degrees/temperature/
warm, math gained percent and "how much|how many", search gained "look up"
and "tell me about", volume gained loud\w*/muting forms, and open_app
gained the phrasing "open the calculator". Measured before/after on the
full battery: at the old thresholds (0.80/0.90) 33/42 -> 38/42; at the
shipped thresholds (0.96/0.96) 37/42 -> 42/42 measured live (not an
offline re-score). Fixed rows: "launch the calculator" p=0.962, "do i need
a jacket tonight" p=0.981, "how much is 15 percent of 80" p=0.961, "look
up the capital of iceland" p=0.965. Zero false positives. New margin note:
the lowest true positive is now p=0.961, only 0.001 over the threshold -
fragile; revisit with a bigger battery before trusting it. The
empty-window fast path (29 ms chatter answers, p=0.000 rows) is still
deliberately kept: falling back to all intents there would cost chatter
latency and re-introduce false-positive risk; revisit only with data.

Bigger battery (66 messages: the 42 above + 15 harder paraphrase
positives + 9 adversarial none-cases, same harness, measured live at
0.96/0.96): 61/66 with ZERO false positives. One failure was a narrow()
miss ("quieter" did not match quiet\b; fixed by widening to quiet\w*/
mute\w*, verified passing at p=0.973). The other 5 are threshold-side
misses: real commands scoring under 0.96 ("what files are in my
documents" 0.626, "start the terminal" 0.957, "run spotify" 0.781, "tell
me about the roman empire" 0.830, "look up the population of tokyo"
0.921). Measured finding: the true-positive and false-positive bands
overlap - "start the terminal" scores 0.957 while the worst false
positive ("i deleted my ex's playlist yesterday") scores 0.954 - so no
single threshold separates them. 0.96 buys zero false positives at the
cost of rejecting some genuine paraphrases (they fall through to normal
chat, no wrong card offered), which matches the refuse-rather-than-guess
design. Per-intent thresholds (e.g. a lower bar for open_app, which has
no observed false positives) are a possible follow-up but data-starved
at 66 messages; not applied.

Caveat on speed: per-message routing in this sandbox takes 3.5-6 s
(fp32, tract, 2 GB RAM VM). That is a sandbox floor, not a Dell number;
tools/dell_bench.sh is where the real timing comes from. A sub-second Dell
result would make Laya viable as an always-on pre-classifier; a multi-second
result says keep it opt-in.

## Honest labels

All numbers above are sandbox numbers (a 2026 cloud VM, AVX2, 2 GB RAM) unless a row says otherwise. Nothing here was measured on the Dell;
`tools/dell_bench.sh` exists to produce those numbers.

## 4f. Screen-fork infrastructure + strict baselines (synthetic desktop set, live, sandbox)

The fork now has all four pieces (experiments/screen-fork/): a niri-style
synthetic desktop generator with exact labels (synth_desktop.py: 6 app
types, 1920x1080 + 1366x768, 10 question families), the Colab notebook
mixing it 1:1 with ScreenQA, a fixed eval battery (eval_battery.py:
stock 256M / stock 500M / OCR hybrid / fork, same scenes + questions),
and RUN.md (one-page how-to). Anti-leakage is structural, not just
disjoint seeds: the eval set uses held-out corpus items (25% of every
text corpus), held-out accent colors (teal/pink), and held-out question
phrasings, so a memorizing fork cannot score by recall. Refusal is
trained AND tested: the "absent" family asks about UI that does not
exist with the fixed answer "not visible on the screen", and "color_neg"
probes border-color hallucination.

Strict-set baselines so far (86 questions over 8 held-out 1920x1080
scenes, normalized containment scoring, sandbox 1-core VM):

| family | ocr hybrid | stock 256M @960px |
|--------|-----------|-------------------|
| absent (refusal) | 0/8 | 0/8 |
| app | 3/8 | 0/8 |
| clock | 3/8 | 1/8 |
| color | 0/8 | 0/8 |
| color_neg | 5/8 | 7/8 |
| count | 2/8 | 1/8 |
| focus | 1/8 | 0/8 |
| presence | 8/8 | 3/8 |
| spatial | 1/8 | 0/8 |
| text | 6/14 | 1/14 |
| TOTAL | 29/86 | 13/86 |
| avg sec | 1.4 | 9.7 |
| peak MiB | 793 (server+tesseract) | 534 |

Three findings: (1) the OCR hybrid beats stock 256M overall and is 7x
faster, matching 4e - but on a 2010 i3 M370 the 256M's per-question cost
is the binding constraint, not RAM (both fit 6 GB). (2) BOTH baselines
score 0/8 on absent - system prompts do not produce refusal, measured
again; the fork's refusal training data is the only fix in the pipeline.
(3) Both score 0/8 on true color naming - the 256M's 7/8 on color_neg is
a "say no" bias, not color vision (it also answers "no" to 5/8 presence
questions where the app IS there). The fork column and the stock-500M
column (86 questions x ~57 s sandbox) run next; the fork ships only if
it beats BOTH baselines including on absent and color.

## 4g. No-training paths: what actually beats stock (2026-11-27)

The user has no GPU and will not train, so the question stopped being "how do
we fine-tune SmolVLM" and became "what can be made better without touching a
single weight". Three pieces were built and measured on the 4f strict battery
(86 questions, 8 held-out 1920x1080 scenes, eval-only corpora/colours/phrasings),
plus a new set that adds genuinely visual questions (eval3, 12 families, 100
questions).

What was built:

- `fork_router.py` - no model at all. Answers from the compositor's window list
  (the same fields niri IPC reports: app_id, title, is_focused, layout
  column/row), a border-pixel sample, and per-window tesseract OCR.
- `fork_infer.py` - stock SmolVLM-256M Q8 with inference-side changes only:
  crop the named window to native size, inject the closed answer set (window
  titles, app names, palette) into the prompt, constrain generation with a GBNF
  grammar. Weights untouched.
- `scene_meta.py` - dumps the scene graph for the eval scenes, so the router can
  be measured with the metadata a real niri session provides. It refuses to
  write a sidecar when the regenerated PNG differs from the file on disk, so a
  mismatched graph cannot be used silently.

eval2, 86 questions:

| method | total | avg s/q | peak MiB |
|--------|-------|---------|----------|
| router (no model) | 84/86 | 0.27 | 81 |
| OCR hybrid | 29/86 | 1.4 | 793 |
| stock 256M @960px | 13/86 | 9.7 | 534 |
| stock 256M + crop + candidates + grammar | 36/86 | 16.2 | 539 |
| stock 500M @960px | 19/86 | 18.5 | 930 |

| family | stock 256M | +crop+cand+gram | router |
|--------|-----------|-----------------|--------|
| absent (refusal) | 0/8 | 0/8 | 8/8 |
| app | 0/8 | 4/8 | 8/8 |
| clock | 1/8 | 1/8 | 8/8 |
| color | 0/8 | 3/8 | 8/8 |
| color_neg | 7/8 | 8/8 | 8/8 |
| count | 1/8 | 2/8 | 8/8 |
| focus | 0/8 | 4/8 | 8/8 |
| presence | 3/8 | 3/8 | 8/8 |
| spatial | 0/8 | 6/8 | 8/8 |
| text | 1/14 | 5/14 | 12/14 |

Four findings:

1. The inference-only fork is a real 2.8x on the model path (13 -> 36 of 86),
   and it fixes exactly the families that need looking: spatial 0 -> 6/8,
   focus 0 -> 4/8, app 0 -> 4/8, color 0 -> 3/8. Cost: 16.2 s/question against
   9.7 s, because grammar validation and a longer prompt add per-token work.
   On a 2010 i3 M370 that matters more than the accuracy does.
2. It still cannot refuse. 0/8 on absent even with a grammar that offers "not
   visible on the screen" as an option; the model emits a colour or a number
   instead ("1.", "23.3.", "Nothing to Commend."). Counting stays at 2/8, the
   clock at 1/8. Refusal and counting are what training buys; no prompt or
   grammar bought them here.
3. The router wins outright on this battery: 84/86, 60x faster than the tuned
   model, 6.5x smaller, and it fails only on two browser URLs where tesseract
   read `linuxkiller9000` as `lLinuxkiller9000`.
4. eval3 makes the split explicit: the router answers 84/84 of the state, text
   and refusal questions and 0/16 of the two visual families (a top-bar status
   dot and a window progress bar), because those ask for colour in pixels that
   neither metadata nor OCR carries. It says so instead of guessing. OCR hybrid
   on eval3: 24/100.

5. Stock 500M is better than 256M (19/86 vs 13/86) and the best of the stock
   sizes on presence (6/8) and count (3/8), but it costs 18.5 s/question and
   930 MiB, and it is still 0/8 on refusal, colour, focus and app. It does not
   change the conclusion: the router beats it by 65 questions for a fraction of
   the time and memory.

The two genuinely visual families (eval3: top-bar status dot, window progress
bar), 16 questions:

| method | visual families |
|--------|-----------------|
| router | 0/16 (refuses, by design) |
| OCR hybrid | 5/16 |
| stock 256M | 5/16 |
| stock 256M + crop + candidates + grammar | 6/16 |

With eight possible colours, chance is 2/16. Every path is near chance and none
of them knows it, which is why the router refuses: a colour question about a
pixel it never looked at is exactly the question that produces a confident wrong
answer. These families are what a trained fork is for, and they are the reason
the Colab run is still worth one more attempt.

The router's 0.27 s/question is measured alone on an idle box: median 0.02 s for
a metadata answer, and the first question on each new scene pays that scene's
tesseract pass (max 1.86 s). OCR is charged once per image, not per question.

Caveats: every timing here is this sandbox (1 vCPU, 2 GB), not the N5010. The
router's window metadata comes from `scene_meta.py`, which supplies the same
fields niri IPC supplies in production, so these are "perfect metadata on
synthetic scenes". fork_infer uses that metadata too, for crops and candidates.

### CPU LoRA: attempted, and the honest result

`lora_probe.py` loads SmolVLM-256M-Instruct through transformers, attaches LoRA
(r=4, q/k/v/o) to the language backbone, and runs forward+backward+step on a real
scene. On this sandbox (2 GB) it is killed by the OOM killer before the first
step: once in fp32, and again in bf16 with a 384px image. The fp32 weights alone
are ~1.05 GB and the loader peaks above the 1.8 GB the box has free.

So there is no s/step number in this document, because the only one available
would come from a smaller proxy model and would not be this model's number. What
the attempt does establish: a 2 GB machine cannot adapt SmolVLM-256M at all, and
a 6 GB machine (the N5010) has room for the weights but not for a claim about
speed. Training on his CPU stays unmeasured and unquoted.

The free Colab T4 remains the only sensible route to the visual families, which
is what the notebook is for. The router needs no training and covers 84 of the
86 state, text and refusal questions on its own.

### Why a low training loss is not alarming here

The light-config training set (320 scenes) yields 4564 QA rows over 373 distinct
answers, and the ten most common answers cover 52.6% of all rows: "no" 14.0%,
"yes" 7.0%, "not visible on the screen" 7.0%, then six palette names at ~3.5%
each. A 256M model that learns that distribution fits the bulk of the training
set quickly, so 15 -> 0.23 in one epoch is expected rather than suspicious.
leakage_check.py separately rules out the eval set being in the data. What a low
loss does not tell us is whether the model generalises to new layouts and fonts;
only the held-out battery can, which is what fork_eval.sh measures.

## 4h. Crop around the object: the window first, the pointer second (2026-11-27)

Every no-training path in 4g sits at chance on the pixel-colour families, and the
reason is resolution, not prompting: a 1366x768 frame squeezed into the model's
960px budget makes a 14px focused border a handful of pixels wide. Four crop sources were measured,
all with stock SmolVLM-256M and the same plain question. All numbers: sandbox,
1 vCPU / 2 GB. The first four rows are eval3's 16 visual questions (indicator and
progress families); the border row is eval2's 16 border-colour questions.

| crop source | correct | s/q | peak RSS |
|---|---|---|---|
| none (full frame, 960px) | 5/16 | 9.7 | 534 MiB |
| oracle: crop centred on the object | 11/16 | 7.2 | 530 MiB |
| random position, same crop size | 1/16 | 7.0 | 530 MiB |
| window: crop = the whole window the question is about | 15/40 | 15.2 | 559 MiB |
| border strip: top edge of the focused window (metadata only) | 13/16* | 7.1 | 530 MiB |

The oracle crop doubles accuracy and is 25% faster than the full frame (fewer
image tokens). The random crop proves the gain comes from the crop's content and
not from crop size or a shorter prompt: at the same 500px it scores at chance.
Chance on these families is 2/16. (*The border row is eval2's 16 colour
questions, not eval3's 16 visual ones, so it is scored out of 16 with chance
at 4/16; see the per-family paragraph below.)

Per family, on eval2's 40 window-scoped questions (stock 256M, full frame ->
window crop): focus 0/8 -> 2/8, colour of the focused border 0/8 -> 0/8, "is the
border yellow" 7/8 -> 6/8, text in a named window 1/16 -> 7/16. On eval3's
progress family the window crop does better than any other crop source measured:
7/8, against 4/8 full frame, 6/8 with the oracle crop and 1/8 with the random
crop. So the window crop is worth about +7 questions on eval2 and +3 on
progress, almost all of it text and progress-bar reading, and it costs 15.2 s/q
against 9.7 s/q because a window-sized crop carries more image tokens than a
960px full frame.

Colour of the focused border is the family the window crop cannot fix, and the
reason is geometric: the border is a 4px frame around the *edge* of the window,
so a crop of the whole window puts it at the margin of the image and the model
answers "Black" 8 times out of 8. A metadata-only sub-crop does fix it: the top
edge of the focused window, 400x120 centred on the edge midpoint, magnified by
the model's own resize, scores 5/8 on the colour and 8/8 on "is it yellow" at
7.1 s/q - the fastest and most accurate of the four crop sources, with no
pointer involved.

So the crop is what buys visual accuracy, and the remaining problem is entirely
one of knowing *where* to crop. That is now answered from metadata alone: the
window rectangle for text and progress questions, the border strip of the focused
window for border colour. Neither needs a pointer position, and neither needs the
model to see the full frame. That is a UI problem, not a model problem, and
it splits into the two stages below.

### Stage 1 - the window (this is the route Pawline takes)

The router already knows every window's app id, title, geometry and focus from
`niri msg --json windows` (`screencap::parse_niri_windows`), so for a question
that names a window or the focused window, the crop rectangle is already in
hand with no pointer involved. `cursor_crop.py --cursor window` reproduces that:
it picks the focused window for colour/focus questions and the named app's
window for text questions, pads by 6px and crops at native resolution.

No geometry maths is needed even for the crop itself: niri's IPC `Action` enum
carries `ScreenshotWindow` (and `ScreenshotScreen`), both with a `path` and
`show_pointer` argument, so `niri msg action screenshot-window --path ... --show-pointer false`
writes a window-sized PNG directly. The window rectangle is then only needed for
the confirm card, and `tile_size` / `window_size` from the same window list
supply its size.

The confirm step the user asked for ("is this it?" -> else let him pick) is the
same rectangle shown back to him as a card before any pixels are sent anywhere,
and `slurp` is the fallback when the answer is no: it is a rectangle picker, so
the user's own drag supplies the rect and no pointer API is needed at all.

### Stage 2 - the pointer (research only, not shipped)

If the crop should instead be centred on where the cursor is, the crop code is
already written (`cursor_crop.py`); what is missing is a way to *read* the
pointer position without a click. Findings as of 2026-11-27:

- niri merged `niri msg pointer` (PR #1924), which reports the pointer location
  and can set it. Reading it needs a niri new enough to have that subcommand.
- `window-under-cursor` as a single IPC call is still an open PR (#4189), so the
  compositor will not hand back "which window is under the pointer" today; the
  same result comes from `msg pointer` plus the window rects the router already
  has (point-in-rect over `parse_niri_windows` output).
- `niri msg pick-window` exists (PR #1031) but needs a click, so it is a
  user-pick path, not an ambient one.
- `grim` + `slurp` is the established pair for user-chosen regions: `slurp`
  returns a rectangle, `grim -g` crops to it. That is the "else let him pick"
  branch, and it works on any wlroots compositor, not just niri.

Conclusion: the window crop is available today from metadata alone and needs no
new compositor feature; the pointer crop is worth having only as a convenience
over the same code, and it depends on a niri version with `msg pointer`.

## 4i. Why not train a tiny desktop-only model from scratch (2026-11-27)

The question: instead of fine-tuning SmolVLM, train a small model only on desktop
content - words, colours, UI elements - and the user's own suspicion that such a
model would not know about things it has never seen.

The suspicion is right, and the measurements in 4g and 4h say something stronger:
what we are still getting wrong is not a knowledge gap. It is a resolution gap,
and it closes with crops, not with training.

### What from-scratch actually requires

The ViT paper is the cleanest statement of the data hunger: trained on a
mid-sized dataset like ImageNet without strong regularization, ViT "yield[s]
modest accuracies of a few percentage points below ResNets of comparable size",
and only pre-trained on ImageNet-21k or JFT-300M does it approach or beat the
state of the art. A vision encoder does not get its features from architecture;
it gets them from data volume.

The closest thing to "a model trained only on UI" is Google's ScreenAI, and it is
not trained from scratch. Its 670M variant is a ViT-B/16 (92M) plus mT5-base
(583M), both started from pre-trained unimodal checkpoints, then pre-trained on a
corpus of desktop, mobile and tablet screenshots collected by crawling apps and
web pages, annotated automatically with a screen schema and expanded into QA
pairs by PaLM 2-S. The 2B and 5B variants start from the same kind of checkpoints
and, for 5B, from PaLI-3's multimodal checkpoint. So even the UI specialist with
Google's data pipeline treats "UI-only" as a specialization stage on top of
general pre-training, not as a substitute for it.

Apple's Ferret-UI Lite makes the same choice at the small end: a 3B model built
on an internal 3B model already pretrained on text and vision-language data, then
SFT for 10K steps and RL for 1.5K steps on a curated mixture of real and synthetic
GUI data. It reaches 91.6 on ScreenSpot-V2 and 53.3 on ScreenSpot-Pro, and its
paper lists image cropping and zoom-in as one of the three things that make a
small model work at GUI resolution. That is an independent confirmation of the 4h
finding, from a team with far more data than we have.

### What fine-tuning costs, and why it is still the cheaper direction

LoRA does learn less than full fine-tuning: on code and maths, standard low-rank
settings substantially underperform full fine-tuning, and the perturbations full
fine-tuning learns have rank 10-100x higher than typical LoRA configurations. It
also forgets less, more so than weight decay or dropout. The scaling-laws study
puts a number on the trade: an inverse linear relationship between fine-tuning
performance and the amount of forgetting, and it holds for LoRA too.

So specializing always spends generality. Fine-tuning spends less of it than
training from scratch, and LoRA spends less than full fine-tuning. That is the
whole argument for fine-tuning over from-scratch, before compute enters it.

### What our own numbers say the gap is

On the same 86-question battery (sandbox, 1 vCPU / 2 GB, all in 4f/4g):

| path | correct | s/q | note |
|---|---|---|---|
| metadata router, no model | 84/86 | 0.27 | state, text, focus, refusal |
| OCR hybrid | 29/86 | 1.4 | |
| stock SmolVLM-500M, full frame | 19/86 | 18.5 | |
| stock SmolVLM-256M, full frame | 13/86 | 9.7 | visual 5/16 |
| stock 256M, metadata-chosen crop | progress 7/8, border colour 5/8 | 7.1-14.7 | zero training |

Every no-training path sits near chance on pixel colour (5-6 of 16 against a
2/16 chance line) until the crop is right, and then a stock model answers most of
them. Nothing in that table is a missing fact about desktops. The model already
knows what a progress bar is and what teal looks like; it could not see them.

A model trained only on desktop content would be trained to solve a problem the
router and the crops already solve, while giving up the language prior that makes
"which window is focused" and "I cannot see that" work at all. Section 4g also
measured that refusal does not come from prompting (0/8 with a grammar that
offers the refusal string), so a specialized model would answer out of scope
confidently unless the router keeps that job anyway.

### What it would cost on the N5010

The CPU LoRA probe in 4g is the honest scale marker: SmolVLM-256M cannot even be
adapted on 2 GB, killed by the OOM killer before the first step at fp32 and again
at bf16 with a 384px image. A 6 GB machine has room for the weights but not for a
claim about speed, and from-scratch pre-training is orders of magnitude past
adaptation: ScreenAI-scale work means millions of screenshots plus a labeling
pipeline that itself needs a large model. We also do not have the corpus: 320
synthetic scenes and 4564 QA rows is a fine-tuning set, not a pre-training set.

### Recommendation

1. Do not train from scratch. Nothing in the published UI-model literature does,
   the smallest credible UI specialists are pretrained backbones plus large
   auto-labelled corpora, and we have neither the data nor the compute.
2. Ship stock SmolVLM-256M plus the metadata router plus metadata-chosen crops.
   It is the only path measured to beat the OCR hybrid on latency and the full
   frame on visual accuracy at the same time, and it needs no training at all.
3. If training happens, it is LoRA on the free Colab T4, on the visual families
   only, and it ships only if it beats the crop baseline (progress 7/8, border
   colour 5/8, indicator 5/8 with the oracle crop) on held-out scenes and does
   not lose eval2's refusal families. `fork_eval.sh FORK.gguf MMPROJ.gguf` runs
   exactly that comparison, and `leakage_check.py` guards the data.
4. For unseen apps and themes, prefer mechanisms that need no training: metadata
   for structure, OCR for text, crops for colour. They transfer to an app nobody
   has seen, which is the one thing a trained-only model cannot promise and his
   battery cannot test until he meets one.

Sources:
- ViT, "An Image is Worth 16x16 Words" - https://arxiv.org/abs/2010.11929
- ScreenAI - https://arxiv.org/html/2402.04615 and Google's summary,
  https://research.google/blog/screenai-a-visual-language-model-for-ui-and-visually-situated-language-understanding/
- Ferret-UI Lite (Apple) - https://arxiv.org/html/2509.26539
- "LoRA Learns Less and Forgets Less" - https://arxiv.org/abs/2405.09673
- "Scaling Laws for Forgetting When Fine-Tuning Large Language Models" -
  https://arxiv.org/abs/2401.05605

## 4j. Colab outcome, and what a manual pick is worth (2026-11-27)

### Colab training: closed without an artifact

The free Colab T4 run trained, and its loss curve fell from 15.05 to 0.075. The
adapter was then lost when the runtime disconnected: no checkpoint was kept, so
there is no fork to evaluate. Colab is closed at the user's request. Kaggle's
free GPU tier was considered as the alternative and not used. Nothing in 4g/4i
depends on that artifact: every shipped number is stock SmolVLM-256M.

The 15.05 -> 0.075 curve is consistent with 4g's explanation of the earlier
15 -> 0.23 run: 4564 training rows over 373 distinct answers, with the ten most
common answers covering 52.6% of them, is a distribution a 256M model fits fast.
It says nothing about generalisation, which only the held-out battery measures.

### Manual selection vs metadata selection

The question was whether a pick made by hand raises accuracy a lot. Measured with
stock SmolVLM-256M, temperature 0, same questions, sandbox 1 vCPU / 2 GB. "Tight
box" is a 500px crop centred on the thing being asked about; "strip" is the
400x120 band on the focused window's top edge; "user-picked window" is the window
that actually contains the thing.

eval3, the 16 genuinely visual questions:

| crop source | indicator | progress | total | s/q | peak |
|---|---|---|---|---|---|
| full frame | 1/8 | 4/8 | 5/16 | 9.7 | 534 MiB |
| metadata-chosen window | 1/8* | 7/8 | 8/16 | 14.7 | 560 MiB |
| user-picked window | 2/8 | 7/8 | 9/16 | 15.1 | 559 MiB |
| tight box on the thing | 5/8 | 6/8 | 11/16 | 7.2 | 530 MiB |
| random box, same size | 0/8 | 1/8 | 1/16 | 7.0 | 530 MiB |

*the metadata route has no window for a top-bar dot, so it keeps the full frame.

eval2, the 40 window-scoped questions:

| crop source | focus | border colour | "is it yellow" | text | s/q |
|---|---|---|---|---|---|
| full frame | 0/8 | 0/8 | 7/8 | 1/16 | 9.7 |
| metadata-chosen window | 2/8 | 0/8 | 6/8 | 7/16 | 15.2 |
| user-picked tight box | 1/8 | 0/8 | 8/8 | - | 10.3 |
| border strip (400x120) | - | 5/8 | 8/8 | - | 7.1 |

What this says:

- A hand-made pick is worth a lot only when it is tight. Picking a whole window
  gains 1 question over the metadata pick on eval3 (8/16 -> 9/16) and gains
  nothing on border colour (0/8 either way, because the border is a 4px frame at
  the window's edge and a window-sized crop leaves it at the margin).
- A tight box is the best source measured: 11/16 against 5/16 for the full frame,
  and it is also the fastest (7.2 s/q) because a 500px crop is fewer image tokens
  than a 960px frame.
- Magnification matters more than centring. A 500px box centred on the border
  still answers "Black" 8/8; the 400x120 strip, which the model's own resize
  magnifies about 4x more, answers 5/8. Same content, different scale.
- Window picks are the right choice for text (1/16 -> 7/16) and progress bars
  (4/8 -> 7/8), where the thing is a normal-sized element inside the window.

So the picker in Pawline offers both, and the card says which it will capture:
the route narrows to the window when the question names one, offers the window
option on the card when it does not, and the tight region is what `slurp` gives
when the user draws it. A colour question on a border is answered from the strip,
not from the window.

### What shipped (branch work/llm-vision-experiments)

- `screencap::ShareTarget` (whole screen, one window with an optional top strip,
  or a drawn region) with `describe()`, so the confirm card names exactly what
  will be captured: "It captures only the \"Inbox - Thunderbird\" window
  (Thunderbird), not the whole screen, then deletes the capture."
- `window_capture_command` (`niri msg action screenshot-window --path ... --id N`)
  and `region_capture_command` (`grim -g "x,y wxh"`) with `slurp_command` and
  `parse_slurp_geometry` for the picker's drawn region.
- `crop_png_top` for the border strip, and `crop_for_question` /
  `names_a_window` for choosing between window, strip and frame from the window
  list alone (no pointer needed).
- The picker's second half: `share_target_from_phrase` (core, tested) turns the
  phrases the card offers into a target - "share the whole screen", "share that
  window", "share only the kitty window", "pick a region" - and the bridge
  re-plans the same question with that target behind the ordinary confirm card.
  "pick a region" runs slurp and captures exactly the rectangle the user draws;
  a named window that is not open is named back and nothing is captured. The
  region is drawn only after a screen question exists, because the phrase
  re-plans that question rather than starting a new one.
- Unsure-answer fallback: `answer_is_unsure` and `fallback_search_query` build a
  short query from the question's own words plus the app name (never screen text,
  never a window title), and the reply offers "search it". That builds a card
  naming the exact text and the free DuckDuckGo endpoint; nothing is sent until
  the user confirms, and there is no automatic search. A unit test drives the
  card with a mock pending query and asserts the plan is unexecuted and that the
  card names the text and the service.

Tests: `cargo test -p pawline-core --lib screencap` 17 passed;
`cargo test -p pawline-native --bin pawline` 171 passed, 0 failed.

That last run found a real bug, not a stale expectation. The failing case was
`"look at ~/notes.txt for me"`: the "look at <path>" prefix sent every path to
the image route, so a text file produced a card promising to send the document to
a vision model, which then failed on the format. The route now takes the image
path only for `.png/.jpg/.jpeg/.gif/.webp` and falls through otherwise, and
`only_image_files_get_an_image_card` covers both sides (a text file gets no card,
a real image still does). It had gone unnoticed because that test binary did not
compile before this work: a stale `ChatOnline` initializer was missing its
`provider` field, so none of the 168 tests in it had been running.

Not verified here: the niri and slurp argv shapes are covered by unit tests but
have not been run against a live niri or slurp, because this sandbox has neither.
The first live check on the Dell should be one window question and one region
question.
