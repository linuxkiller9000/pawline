#!/usr/bin/env python3
"""Cursor crop: answer a visual question from ~500x500px around the pointer.

The user's idea (2026-11-27): do not look at the whole screen. Crop a small
square around the cursor, highlight it, ask "is this what you mean?", and if it
is not, let him select the area himself. This measures the part that can be
measured: does a small native-resolution crop answer the genuinely visual
questions better than the full 1920x1080 frame, and what does it cost?

Two cursor cases, because they are not the same question:
  oracle  the crop is centred on the thing the question is about (best case,
          what happens when the user points at it before asking)
  random  the crop is centred on a random point of the screen (what happens
          when the pointer is somewhere else and the confirm loop has to say
          "not here, show me")

Both are run against the visual families of eval3 (status dot colour, progress
bar colour), where every full-frame path sits near chance.

Usage:
  python3 cursor_crop.py --dir /tmp/synth/eval3 --cursor oracle
  python3 cursor_crop.py --dir /tmp/synth/eval3 --cursor random --size 500

License: MIT.
"""
import argparse
import json
import os
import random
import resource
import statistics
import subprocess
import tempfile
import time

from PIL import Image

import eval_battery as eb

LLAMA = os.path.join(eb.VLM_DIR, "llama-b12811", "llama-mtmd-cli")
MODEL = os.path.join(eb.VLM_DIR, "SmolVLM-256M-Instruct-Q8_0.gguf")
MMPROJ = os.path.join(eb.VLM_DIR, "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf")
VISUAL = ("indicator", "progress")


def object_point(meta, family):
    """Where the thing the question is about actually is (harness-only: the
    scene graph knows; a real session knows the pointer, not this)."""
    W = meta["width"]
    if family == "indicator":
        return (W - 167, 14)          # the status dot drawn in the top bar
    w = meta["windows"][meta["progress_win"]]
    return (w["x"] + w["w"] // 2, w["y"] + w["h"] - 22)


def crop_rect(cx, cy, size, W, H):
    x0 = max(0, min(W - size, cx - size // 2))
    y0 = max(0, min(H - size, cy - size // 2))
    return (x0, y0, x0 + size, y0 + size)


def run(png, rect, question):
    im = Image.open(png).crop(rect)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    try:
        t0 = time.time()
        p = subprocess.run([LLAMA, "-m", MODEL, "--mmproj", MMPROJ,
                            "--image", tmp.name, "-p", question + " Answer briefly.",
                            "--temp", "0", "-n", "48"],
                           capture_output=True, text=True, timeout=600)
        dt = time.time() - t0
        out = p.stdout.strip()
        return (out.splitlines()[-1].strip() if out else ""), dt
    finally:
        os.unlink(tmp.name)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--cursor", choices=["oracle", "random", "window", "border"], default="oracle")
    ap.add_argument("--families", default="indicator,progress",
                    help="comma-separated families (window mode also does color,text)")
    ap.add_argument("--size", type=int, default=500)
    ap.add_argument("--results", default=None)
    args = ap.parse_args()
    results_path = args.results or os.path.join(
        args.dir, f"results_cursor_{args.cursor}.json")

    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    fams = {f.strip() for f in args.families.split(",")}
    qa = [r for r in qa if r["family"] in fams]
    images = sorted({r["image"] for r in qa})
    res = json.load(open(results_path)) if os.path.exists(results_path) else {}
    key = f"cursor_{args.cursor}_{args.size}"
    res.setdefault(key, {})
    rng = random.Random(7)

    for img in images:
        png = os.path.join(args.dir, img)
        meta = json.load(open(os.path.join(args.dir, img.replace(".png", ".json"))))
        for r in qa:
            if r["image"] != img:
                continue
            qid = f"{img}|{r['family']}|{r['question']}"
            if qid in res[key]:
                continue
            if args.cursor == "border":
                # metadata-only sub-crop for "what colour is the border" - the
                # top edge of the focused window, magnified. No pointer needed.
                w = next((x for x in meta["windows"] if x["focused"]), meta["windows"][0])
                cx = min(max(w["x"] + w["w"] // 2, 200), meta["width"] - 200)
                cy = min(max(w["y"], 60), meta["height"] - 60)
                rect = (cx - 200, cy - 60, cx + 200, cy + 60)
                rect = (max(0, rect[0]), max(0, rect[1]),
                        min(meta["width"], rect[2]), min(meta["height"], rect[3]))
            elif args.cursor == "window":
                # window-based version: crop the window the question is about,
                # at native resolution. This is the confirm-loop unit the user
                # asked for first - "is this the window you mean?"
                if r["family"] in ("color", "color_neg", "focus"):
                    w = next((x for x in meta["windows"] if x["focused"]), meta["windows"][0])
                else:
                    import fork_router as fr
                    app = fr.target_app(r["question"])
                    w = next((x for x in meta["windows"] if app and x["app"] == app),
                             next(x for x in meta["windows"] if x["focused"]))
                pad = 6
                rect = (max(0, w["x"] - pad), max(0, w["y"] - pad),
                        min(meta["width"], w["x"] + w["w"] + pad),
                        min(meta["height"], w["y"] + w["h"] + pad))
                cx, cy = (rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2
            elif args.cursor == "oracle":
                cx, cy = object_point(meta, r["family"])
                rect = crop_rect(cx, cy, args.size, meta["width"], meta["height"])
            else:
                cx = rng.randrange(meta["width"])
                cy = rng.randrange(meta["height"])
                rect = crop_rect(cx, cy, args.size, meta["width"], meta["height"])
            got, dt = run(png, rect, r["question"])
            ok = eb.norm(r["answer"]) in eb.norm(got) and eb.norm(r["answer"]) != ""
            peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            res[key][qid] = {"ok": ok, "expected": r["answer"], "got": got,
                             "seconds": round(dt, 1), "peak_kib": peak,
                             "cursor": [cx, cy], "rect": list(rect)}
            print(f"{'OK ' if ok else 'MISS'} [{key}] {img} {r['family']:9s} "
                  f"want={r['answer']!r} got={got!r} ({dt:.1f}s) cursor={cx},{cy}",
                  flush=True)
            json.dump(res, open(results_path, "w"), indent=1)

    rows = list(res[key].values())
    print(f"== {key}: {sum(r['ok'] for r in rows)}/{len(rows)}  "
          f"{statistics.mean(r['seconds'] for r in rows):.1f}s avg  "
          f"{max(r['peak_kib'] for r in rows) / 1024:.0f} MiB peak ==")


if __name__ == "__main__":
    main()
