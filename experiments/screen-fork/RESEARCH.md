# Screen-tuned SmolVLM fork: feasibility research

Question (user, 2026-11-27): should we fork SmolVLM into something screen-tuned
for "what's on my screen?"? Researched honestly; the measured OCR-hybrid
numbers below say the fork is probably NOT the first thing to build.

## Bottom line

A screen-tuned SmolVLM LoRA fork is feasible with free data and free compute,
but the OCR hybrid (this directory, tested in the sandbox) already beats both
stock SmolVLM sizes on text-content screen questions by a wide margin at
roughly 5x lower latency. Fine-tuning would close the fidelity gap on
visual/spatial questions, not text reading, and it is a multi-day project
with real quality risk. Recommend: ship the OCR hybrid path first, revisit
the fork only if visual-question quality turns out to matter in use.

## Free datasets (all verified 2026-11-27)

| dataset | size | license | notes |
|---------|------|---------|-------|
| ScreenQA (google-research-datasets/screen_qa) | 86K QA over 35K Rico screenshots | CC BY 4.0 (annotations) | QA pairs are permissive with attribution, BUT the underlying Rico screenshots are under Rico's own research terms (interactionmining.org copyright.txt) - read before training |
| ScreenQA-Complex (HF bevaya/RICO-ScreenQA-Complex) | 11.8K multi-region reasoning QA | CC-BY-4.0 (HF card) | test-only split; counting/comparison questions |
| WebSight v0.2 (HF HuggingFaceM4/WebSight) | 1.92M web screenshot+HTML pairs | CC-BY-4.0 + must disclose use when training a model | pairs are screenshot->HTML, not QA; useful as vision-side pretraining mix, not directly for "what's on my screen" QA |

Domain gap, stated plainly: ScreenQA/Rico are MOBILE app screenshots,
WebSight is web pages. Neither is a Linux desktop. The ScreenQA paper
(NAACL 2025) reports positive transfer mobile -> web, so partial transfer
to desktop is plausible, but a fork tuned only on these may still miss
desktop idioms (title bars, terminals, dark themes). There is no large
free desktop-screenshot QA set that I could find.

## Free compute

- Google Colab free tier: T4-class GPU (16 GB), session-limited. Enough for
  LoRA on SmolVLM-256M/500M with gradient checkpointing and small batches.
- Kaggle: ~30 GPU-hours/week (P100 16 GB / T4x2 at time of writing).
- Quotas and hardware change; check the current tier before promising a
  schedule.
- The sandbox (2 GB CPU) cannot fine-tune; training happens in one of the
  notebooks above. Conversion back to GGUF (llama.cpp convert script) is
  CPU-only and runs anywhere.

## Sketch: LoRA fork recipe (for when/if we do it)

1. Data: ScreenQA train (68,951 QA) reformatted to chat messages, plus
   ScreenQA-Complex for multi-region reasoning. Optional: a small
   self-built desktop set (our synthetic generator + templated QA, a few
   thousand samples) to close the desktop domain gap for free.
2. Method: TRL SFTTrainer + PEFT LoRA (r=16-64) on the language backbone,
   vision tower frozen, bf16, gradient checkpointing, seq len 2048,
   effective batch ~32. The WebSight paper used DoRA r=64 on an 8B;
   SmolVLM-256M is far smaller and trains in hours on a T4.
3. Eval: hold out ScreenQA val + our synthetic desktop battery
   (docs/EXPERIMENTS.md 4d/4e probes) so the fork has to beat BOTH stock
   SmolVLM and the OCR hybrid on the same images before it ships.
4. License stack for the resulting model: SmolVLM base Apache-2.0, ScreenQA
   CC BY 4.0 (attribution), WebSight CC BY 4.0 + disclosure clause. All
   compatible with MIT code + a NOTICE file; the model card must carry the
   attributions and the WebSight disclosure.

## Expected gain vs risk

- Gain: better answers on visual/spatial/color questions (where OCR is
  blind); possibly better small-text reading than the 256M.
- Risk: fine-tunes of 256M models overfit fast; the WebSight authors found
  val loss a poor indicator and selected checkpoints by hand. A bad fork
  looks fluent while being wrong - the worst failure mode for a
  screen-reading assistant. Mitigation: the shared eval battery above.
- Cost: days of iteration even on free compute, plus GGUF conversion and
  re-verification of the whole vision path.

## Measured comparison on our synthetic desktop (sandbox, 1-thread VM)

| approach | wall | quality |
|----------|------|---------|
| SmolVLM-256M, full 1920px | 41.8 s | missed almost everything |
| SmolVLM-500M, full 1920px | 56.7 s | partial, hallucinated credentials |
| OCR hybrid (tesseract + qwen2.5-0.5b) | ~10 s | correct windows + content, no hallucination on text questions; blind to visual questions |

Details and the boundary test in docs/EXPERIMENTS.md section 4e.
