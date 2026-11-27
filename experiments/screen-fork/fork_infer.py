#!/usr/bin/env python3
"""No-training "fork" v2: inference-side adaptation of stock SmolVLM.

Nothing here touches the weights - it changes only what the stock GGUF is fed
and how its answer is constrained. Four levers, each measurable on its own:

  crop        the question names a window (or the focused one) -> feed that
              window at native size instead of the whole 1920px frame, so the
              same vision-token budget lands on the pixels that matter
  candidates  the closed answer set comes from metadata/OCR (window titles, app
              names, palette) and the prompt says "answer with one of these"
  grammar     a GBNF grammar constrains generation to that set, so a made-up
              answer cannot be produced at all (this is what buys refusal and
              a stable answer format)
  tokens      --image-max-tokens caps vision tokens per image (latency lever)

Variants (--variant):
  full             stock behaviour: whole frame, plain prompt
  crop             crop + plain prompt
  crop+cand        crop + candidate list in the prompt
  crop+cand+gram   crop + candidates + GBNF-constrained output  (the full fork)

PROVENANCE: window geometry and titles come from scene_meta.py (the same
fields niri IPC gives in production). The family label in qa.jsonl is never
used for answering - routing is from the question text via fork_router.route().

Usage:
  python3 fork_infer.py --dir /tmp/synth/eval2 --variant crop+cand+gram
  python3 fork_infer.py --dir /tmp/synth/eval2 --report

License: MIT. Free-only: llama.cpp + tesseract + Pillow.
"""
import argparse
import collections
import json
import os
import re
import resource
import subprocess
import tempfile
import time

from PIL import Image

import eval_battery as eb
import fork_router as fr

LLAMA = os.path.join(eb.VLM_DIR, "llama-b12811", "llama-mtmd-cli")
MODEL = os.path.join(eb.VLM_DIR, "SmolVLM-256M-Instruct-Q8_0.gguf")
MMPROJ = os.path.join(eb.VLM_DIR, "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf")
COLORS = [a[0] for a in fr.ACCENTS]


def gbnf(options):
    return "root ::= " + " | ".join('"%s"' % o.replace('"', '\\"') for o in options) + "\n"


GBNF_REFUSE = ('root ::= "not visible on the screen" | short\n'
               'short ::= [a-zA-Z0-9 ,.\'-]{1,40}\n')
# fixed width: the bar clock is always HH:MM, and a fixed grammar is cheaper
# to validate per token than a bounded repetition
GBNF_CLOCK = 'root ::= [0-9] [0-9] ":" [0-9] [0-9]\n'


def build(q, meta):
    """-> (prompt, crop_rect_or_None, grammar_or_None, note)"""
    r = fr.route(q)
    wins = meta["windows"]
    titles = [w["title"] for w in wins]
    apps = [w["app_name"] for w in wins]
    focused = next((w for w in wins if w["focused"]), wins[0])
    if r == "count":
        return (q + " Answer with a single digit, nothing else.", None,
                'root ::= [0-9]\n', "count")
    if r == "focus":
        return (q + " Choose one of these window titles: " + "; ".join(titles) +
                ". Answer with the exact title only.", None, gbnf(titles), "focus")
    if r == "color":
        return (q + " Choose one of: " + ", ".join(COLORS) +
                ". Answer with one word only.", _crop(focused), gbnf(COLORS), "color")
    if r == "color_neg":
        return (q + " Answer yes or no.", _crop(focused),
                'root ::= "yes" | "no"\n', "color_neg")
    if r == "clock":
        return (q + " Answer with the time as HH:MM only.", _bar(meta),
                GBNF_CLOCK, "clock")
    if r == "spatial":
        return (q + " Choose one of these window titles: " + "; ".join(titles) +
                ". Answer with the exact title only.", None, gbnf(titles), "spatial")
    if r == "app":
        return (q + " Choose one of: " + ", ".join(sorted(set(apps))) +
                ". Answer with one name only.", None, gbnf(sorted(set(apps))), "app")
    if r == "presence":
        return (q + " Answer yes or no.", None, 'root ::= "yes" | "no"\n', "presence")
    if r == "text":
        app = fr.target_app(q)
        w = next((w for w in wins if w["app"] == app), focused)
        return (q + " Answer with the exact text shown on screen, no explanation.",
                _crop(w, body_only=True), None, "text")
    return (q + " If the screen does not show it, answer exactly: "
            "not visible on the screen.", None, GBNF_REFUSE, "absent")


def _crop(w, pad=4, body_only=False):
    # body_only: skip the title bar, so a text question cannot be answered with
    # the window title (measured: the model returned "Thunderbird." otherwise)
    y0 = w["y"] + (30 if body_only else -pad)
    return (max(0, w["x"] - pad), max(0, y0), w["w"] + 2 * pad,
            w["h"] - (30 if body_only else 0) + 2 * pad)


def _bar(meta):
    return (0, 0, meta["width"], 32)


def prep_image(png, rect, max_side=1024):
    im = Image.open(png)
    if rect:
        im = im.crop((rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]))
    w, h = im.size
    if max(w, h) > max_side:  # keep vision tokens bounded on big crops
        s = max_side / max(w, h)
        im = im.resize((int(w * s), int(h * s)), Image.LANCZOS)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    return tmp.name


def run(png, q, meta, variant):
    prompt, rect, gram, note = build(q, meta)
    use_crop = variant != "full"
    use_cand = variant in ("crop+cand", "crop+cand+gram")
    use_gram = variant == "crop+cand+gram"
    if not use_cand:
        # strip the injected candidate sentence, keep the task
        prompt = re.sub(r" Choose one of[^.]*\.", "", prompt)
    img = prep_image(png, rect if use_crop else None)
    cmd = [LLAMA, "-m", MODEL, "--mmproj", MMPROJ, "--image", img,
           "-p", prompt, "--temp", "0", "-n", "48"]
    gpath = None
    if use_gram and gram:
        gpath = tempfile.NamedTemporaryFile(suffix=".gbnf", delete=False, mode="w")
        gpath.write(gram)
        gpath.close()
        cmd += ["--grammar-file", gpath.name]
    try:
        t0 = time.time()
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
        dt = time.time() - t0
        out = p.stdout.strip()
        got = out.splitlines()[-1].strip() if out else ""
        peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        return got, dt, peak, note
    finally:
        os.unlink(img)
        if gpath:
            os.unlink(gpath.name)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--variant", default="crop+cand+gram",
                    choices=["full", "crop", "crop+cand", "crop+cand+gram"])
    ap.add_argument("--results", default=None)
    ap.add_argument("--report", action="store_true")
    args = ap.parse_args()
    results_path = args.results or os.path.join(args.dir, "results_infer.json")

    if args.report:
        res = json.load(open(results_path))
        for m in sorted(res):
            rows = res[m]
            fams = collections.defaultdict(lambda: [0, 0])
            for k, v in rows.items():
                f = k.split("|")[1]
                fams[f][1] += 1
                fams[f][0] += v["ok"]
            tot = sum(v["ok"] for v in rows.values())
            avg = sum(v["seconds"] for v in rows.values()) / max(1, len(rows))
            peak = max((v.get("peak_kib", 0) for v in rows.values()), default=0) / 1024
            print(f"== {m}: {tot}/{len(rows)}  {avg:.1f}s avg  {peak:.0f} MiB peak")
            print("   " + " ".join(f"{f}={fams[f][0]}/{fams[f][1]}" for f in eb.FAMILY_ORDER if f in fams))
        return

    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    images = sorted({r["image"] for r in qa})
    res = json.load(open(results_path)) if os.path.exists(results_path) else {}
    key = args.variant
    res.setdefault(key, {})

    for img in images:
        png = os.path.join(args.dir, img)
        meta = json.load(open(os.path.join(args.dir, img.replace(".png", ".json"))))
        rows = [r for r in qa if r["image"] == img]
        for r in eb.pick_questions(rows):
            qid = f"{img}|{r['family']}|{r['question']}"
            if qid in res[key]:
                continue
            got, dt, peak, note = run(png, r["question"], meta, args.variant)
            ok = eb.norm(r["answer"]) in eb.norm(got) and eb.norm(r["answer"]) != ""
            res[key][qid] = {"ok": ok, "expected": r["answer"], "got": got,
                             "seconds": round(dt, 1), "peak_kib": peak, "route": note}
            print(f"{'OK ' if ok else 'MISS'} [{key}/{note}] {img} {r['family']:9s} "
                  f"want={r['answer']!r} got={got!r} ({dt:.1f}s)", flush=True)
            json.dump(res, open(results_path, "w"), indent=1)

    rows = list(res[key].values())
    print(f"== {key}: {sum(r['ok'] for r in rows)}/{len(rows)} ==")


if __name__ == "__main__":
    main()
