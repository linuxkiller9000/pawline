#!/usr/bin/env python3
"""Server vision: does one warm llama-server beat one CLI spawn per question?

Measured 2026-11-28 (sandbox, 1 vCPU, stock SmolVLM-256M Q8):
  * llama-mtmd-cli costs ~7.2 s per question and the cost does NOT depend on
    the crop: 64px and 700px both take 7.4 s, because SmolVLM always runs the
    vision tower twice (global image + one tile), ~3.1 s each at 1 thread.
  * a warm llama-server answering a second question about the SAME screenshot
    costs 0.3 s instead of 7.2 s, but only when the image part comes FIRST in
    the message: then the [image] prefix is cached (cache_n=138) and only the
    question tokens are new. Text-first (what pawline sends today) caches
    nothing and pays the full encode every time.

This script measures both orderings on the visual families of an eval set:
accuracy (same questions, same crops) and seconds per question, cold and warm.

Usage:
  python3 server_vision.py --dir /tmp/synth/eval4 --size 500
  python3 server_vision.py --dir /tmp/synth/eval4 --orders image --families indicator,progress

License: MIT.
"""
import argparse
import base64
import io
import json
import os
import statistics
import subprocess
import sys
import time
import urllib.request

from PIL import Image

import eval_battery as eb
import cursor_crop as cc

LLAMA = os.path.join(eb.VLM_DIR, "llama-b12811", "llama-server")
MODEL = os.path.join(eb.VLM_DIR, "SmolVLM-256M-Instruct-Q8_0.gguf")
MMPROJ = os.path.join(eb.VLM_DIR, "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf")
PORT = 18110


def start():
    p = subprocess.Popen([LLAMA, "-m", MODEL, "--mmproj", MMPROJ,
                          "--port", str(PORT), "-c", "2048", "--no-warmup"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(180):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{PORT}/health", timeout=1)
            return p
        except Exception:
            time.sleep(0.5)
    p.kill()
    sys.exit("llama-server did not come up")


def ask(b64, text, order):
    img = {"type": "image_url", "image_url": {"url": "data:image/png;base64," + b64}}
    txt = {"type": "text", "text": text}
    content = [img, txt] if order == "image" else [txt, img]
    body = json.dumps({"messages": [{"role": "user", "content": content}],
                       "temperature": 0, "max_tokens": 48,
                       "cache_prompt": True}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}/v1/chat/completions",
                                 data=body, headers={"Content-Type": "application/json"})
    t0 = time.time()
    d = json.loads(urllib.request.urlopen(req, timeout=600).read())
    dt = time.time() - t0
    tm = d.get("timings", {})
    return (d["choices"][0]["message"]["content"].strip(), dt,
            tm.get("prompt_n"), tm.get("cache_n"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--families", default="indicator,progress")
    ap.add_argument("--size", type=int, default=500)
    ap.add_argument("--orders", default="text,image")
    ap.add_argument("--results", default=None)
    ap.add_argument("--warm", action="store_true",
                    help="also ask a second, different question on the same crop")
    args = ap.parse_args()
    path = args.results or os.path.join(args.dir, "results_server.json")
    res = json.load(open(path)) if os.path.exists(path) else {}
    orders = [o.strip() for o in args.orders.split(",")]
    fams = {f.strip() for f in args.families.split(",")}
    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    qa = [r for r in qa if r["family"] in fams]
    proc = start()
    try:
        for img in sorted({r["image"] for r in qa}):
            png = os.path.join(args.dir, img)
            meta = json.load(open(os.path.join(args.dir, img.replace(".png", ".json"))))
            for r in qa:
                if r["image"] != img:
                    continue
                ox, oy = cc.object_point(meta, r["family"])
                rect = cc.crop_rect(ox, oy, args.size, meta["width"], meta["height"])
                im = Image.open(png).crop(rect)
                buf = io.BytesIO()
                im.save(buf, format="PNG")
                b64 = base64.b64encode(buf.getvalue()).decode()
                for order in orders:
                    key = f"{order}_{args.size}"
                    qid = f"{img}|{r['family']}|{r['question']}"
                    res.setdefault(key, {})
                    if qid in res[key]:
                        continue
                    got, dt, pn, cn = ask(b64, r["question"] + " Answer briefly.", order)
                    ok = eb.norm(r["answer"]) in eb.norm(got) and eb.norm(r["answer"]) != ""
                    res[key][qid] = {"ok": ok, "expected": r["answer"], "got": got,
                                     "seconds": round(dt, 2), "prompt_n": pn, "cache_n": cn}
                    print(f"{'OK ' if ok else 'MISS'} [{key}] {img} {r['family']:9s} "
                          f"want={r['answer']!r} got={got!r} ({dt:.2f}s cache={cn})", flush=True)
                    json.dump(res, open(path, "w"), indent=1)
                if args.warm:
                    q2 = "How many windows are on the screen? Answer briefly."
                    got, dt, pn, cn = ask(b64, q2, "image")
                    k = f"image_{args.size}_warm"
                    qid = f"{img}|warm"
                    res.setdefault(k, {})
                    res[k][qid] = {"got": got, "seconds": round(dt, 2), "cache_n": cn}
                    print(f"   warm follow-up {dt:.2f}s cache={cn} :: {got[:50]}", flush=True)
                    json.dump(res, open(path, "w"), indent=1)
    finally:
        proc.terminate()
    for key, rows in res.items():
        vals = [v for v in rows.values() if "ok" in v]
        if not vals:
            continue
        print(f"== {key}: {sum(v['ok'] for v in vals)}/{len(vals)}  "
              f"{statistics.mean(v['seconds'] for v in vals):.2f}s avg ==")


if __name__ == "__main__":
    main()
