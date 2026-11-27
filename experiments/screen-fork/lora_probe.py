#!/usr/bin/env python3
"""CPU LoRA feasibility probe: can SmolVLM be adapted with no GPU at all?

The user has no GPU and does not want to train from scratch, so the honest
question is not "would LoRA help" but "what does one optimizer step cost on a
CPU, and how many hours is a small adapter". This measures that instead of
guessing:

  * loads SmolVLM-256M-Instruct at fp32 (the CPU-safe dtype)
  * attaches a tiny LoRA (r=4 on the LM's q/k/v/o projections)
  * runs N forward+backward+step passes on real synthetic desktop scenes with
    real QA targets, single-threaded (one core, like the worst case)
  * reports seconds/step, peak RSS, and the projection for a small adapter

It is a probe, not a training run: N steps cannot converge, and the timing is
a sandbox CPU (1 vCPU, 2 GB), not the user's N5010. Label any number it
produces with that.

Usage:
  python3 lora_probe.py --steps 4
  python3 lora_probe.py --model HuggingFaceTB/SmolVLM-500M-Instruct --steps 2

License: MIT.
"""
import argparse
import json
import os
import resource
import time

import torch
from PIL import Image


def peak_mib():
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="HuggingFaceTB/SmolVLM-256M-Instruct")
    ap.add_argument("--steps", type=int, default=4)
    ap.add_argument("--lora-r", type=int, default=4)
    ap.add_argument("--image", default="/tmp/synth/eval2/10000.png")
    ap.add_argument("--dtype", default="float32", choices=["float32", "bfloat16"],
                    help="base dtype. fp32 needs ~1.05 GB for the weights alone; "
                         "on a 2 GB box it does not fit, bf16 halves that")
    ap.add_argument("--max-px", type=int, default=0,
                    help="downscale the image to this max side before the processor")
    ap.add_argument("--question", default="What color is the border of the focused window?")
    ap.add_argument("--answer", default="purple")
    args = ap.parse_args()

    torch.set_num_threads(1)
    out = {"model": args.model, "lora_r": args.lora_r, "threads": 1}

    t0 = time.time()
    from transformers import AutoProcessor
    proc = AutoProcessor.from_pretrained(args.model)
    try:
        from transformers import AutoModelForImageTextToText as AutoVLM
    except ImportError:  # older transformers
        from transformers import AutoModelForVision2Seq as AutoVLM
    model = AutoVLM.from_pretrained(args.model, dtype=getattr(torch, args.dtype))
    out["load_seconds"] = round(time.time() - t0, 1)
    out["load_peak_mib"] = round(peak_mib())

    from peft import LoraConfig, get_peft_model
    lcfg = LoraConfig(r=args.lora_r, lora_alpha=args.lora_r * 2, lora_dropout=0.0,
                      bias="none", task_type="CAUSAL_LM",
                      target_modules=["q_proj", "k_proj", "v_proj", "o_proj"])
    model = get_peft_model(model, lcfg)
    trainable = sum(p.numel() for p in model.parameters() if p.requires_grad)
    total = sum(p.numel() for p in model.parameters())
    out["trainable_params"] = trainable
    out["total_params"] = total
    out["trainable_pct"] = round(100 * trainable / total, 3)

    img = Image.open(args.image).convert("RGB")
    if args.max_px:
        img.thumbnail((args.max_px, args.max_px))
    msgs = [{"role": "user", "content": [{"type": "image"},
                                         {"type": "text", "text": args.question}]}]
    prompt = proc.apply_chat_template(msgs, add_generation_prompt=True)
    full = prompt + args.answer + "<end_of_utterance>"
    enc = proc(text=[full], images=[img], return_tensors="pt", padding=True)
    labels = enc["input_ids"].clone()
    # only supervise the answer tokens
    plen = proc(text=[prompt], images=[img], return_tensors="pt")["input_ids"].shape[1]
    labels[:, :plen] = -100
    enc["labels"] = labels

    opt = torch.optim.AdamW([p for p in model.parameters() if p.requires_grad], lr=1e-3)
    model.train()
    times = []
    for i in range(args.steps):
        t = time.time()
        loss = model(**enc).loss
        loss.backward()
        opt.step()
        opt.zero_grad(set_to_none=True)
        times.append(time.time() - t)
        print(f"step {i + 1}/{args.steps}: {times[-1]:.1f}s loss={loss.item():.3f} "
              f"peak={peak_mib():.0f} MiB", flush=True)

    med = sorted(times)[len(times) // 2]
    out["seconds_per_step"] = round(med, 1)
    out["step_times"] = [round(t, 1) for t in times]
    out["train_peak_mib"] = round(peak_mib())
    out["seq_tokens"] = int(enc["input_ids"].shape[1])
    for n in (500, 2000):
        out[f"hours_for_{n}_examples_1epoch"] = round(n * med / 3600, 2)
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
