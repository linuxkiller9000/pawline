# Voice audit (item 12), 2026-11-26. Status: OPEN (partial evidence only)

## What was measured today (sandbox, not a user machine)
- Pipeline: the exact command `voice::stt` builds (sherpa-onnx-offline v2.0.3 + Moonshine tiny English int8, 1 thread) run on the 3 sample WAVs shipped with the model (LibriSpeech-style read speech, 4.8 s to 16.7 s, 28 s total).
- Result: all 3 clips transcribed, wall time about 4.6 s total for all three on an AMD EPYC 9655 VM (1 thread). One proper-name error ("Prin" for "Prynne"); the rest matched the reference text apart from case and punctuation.
- This says nothing about: a real microphone, background noise, accents, short spoken commands, an older CPU (the Dell/N5010 are not measured), or any accuracy percentage. 3 clips is not a benchmark.

## Arabic voice (not supported, not claimed)

- Model exists: `moonshine-ai/moonshine-tiny-ar` (27 M, Arabic with code-switching), and sherpa-onnx
  publishes `asr-models` with `moonshine-base-ar-quantized-2026-02-27`. Neither is downloaded, neither
  is run, accuracy is unknown.
- LICENCE, read 2026-11-28 (this corrects the earlier note in this file, which said the Arabic files
  "stay under a NON-COMMERCIAL community licence"; that is not what the licence says):
  - The English model Pawline uses, `moonshine-ai/moonshine-tiny`, is `license: mit` in its model card
    (and so is `UsefulSensors/moonshine-tiny`). That matches what we already claim for English.
  - `moonshine-ai/moonshine-tiny-ar` is `license: other`, and its `LICENSE.txt` is the **Moonshine AI
    Community License Agreement, last updated 15 June 2025**. It is not non-commercial-only:
    research and non-commercial use are free, and commercial use is allowed while annual revenue is
    under US $1,000,000, after which the licence terminates and a paid licence must be requested.
    The grant is revocable, and distribution carries three duties: ship a copy of the agreement,
    ship a "Notice" text file containing "This Moonshine AI Model is licensed under the Moonshine AI
    Community License, Copyright (c) Moonshine AI Ltd. All Rights Reserved", and prominently display
    "Powered by Moonshine AI" in the website, user interface or product documentation.
  - What that means for Pawline: using Arabic Moonshine on his own machine for himself is covered by
    the non-commercial grant. Distributing a build that carries the model would add a licence file, a
    Notice file and a visible "Powered by Moonshine AI" line, which is a change to the app's UI and
    attribution, so it is his decision, not a default (items 11/12/21/25).
  - Not read yet: the licence of the sherpa-onnx `asr-models` Arabic bundle itself (the packaging may
    carry its own terms on top of the model's). Read it before anything is downloaded or recommended.
- Voice transcripts go into the draft only, never auto-sent (unchanged).

## Still open for item 12
Real-mic proof, spoken-command accuracy set with held-out clips, noise and latency measurement on target hardware, TTS (espeak-ng/flite not installed here), Arabic voice, interruption/cancel behaviour on real audio devices, permission prompts.
