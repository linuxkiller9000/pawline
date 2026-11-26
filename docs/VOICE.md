# Voice audit (item 12), 2026-11-26. Status: OPEN (partial evidence only)

## What was measured today (sandbox, not a user machine)
- Pipeline: the exact command `voice::stt` builds (sherpa-onnx-offline v2.0.3 + Moonshine tiny English int8, 1 thread) run on the 3 sample WAVs shipped with the model (LibriSpeech-style read speech, 4.8 s to 16.7 s, 28 s total).
- Result: all 3 clips transcribed, wall time about 4.6 s total for all three on an AMD EPYC 9655 VM (1 thread). One proper-name error ("Prin" for "Prynne"); the rest matched the reference text apart from case and punctuation.
- This says nothing about: a real microphone, background noise, accents, short spoken commands, an older CPU (the Dell/N5010 are not measured), or any accuracy percentage. 3 clips is not a benchmark.

## Arabic voice (not supported, not claimed)
- Public sources list Arabic Moonshine models (Hugging Face moonshine-tiny-ar; sherpa-onnx release asr-models has moonshine-base-ar-quantized-2026-02-27). NOT downloaded, NOT run, accuracy unknown.
- Licence: the Moonshine README says models are MIT by default except legacy non-streaming non-English models, which stay under a NON-COMMERCIAL community licence. The moonshine-tiny-ar page shows "license: other". Which licence applies to each Arabic file is unverified; it must be read before any Arabic model is recommended or bundled (ties to items 21/25).
- Voice transcripts go into the draft only, never auto-sent (unchanged).

## Still open for item 12
Real-mic proof, spoken-command accuracy set with held-out clips, noise and latency measurement on target hardware, TTS (espeak-ng/flite not installed here), Arabic voice, interruption/cancel behaviour on real audio devices, permission prompts.
