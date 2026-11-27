#!/usr/bin/env python3
"""Does cropping to the window help OCR read screen text?

Text questions do not go to the vision model at all in Pawline: they go to
tesseract on the capture (docs/EXPERIMENTS.md 4e). The window crop that helps
the vision model (4j) should help OCR too, because a 1366x768 frame squeezed
into an OCR pass spends most of its pixels on things nobody asked about.

This measures retrieval, not end-to-end accuracy: does the expected string
appear in the OCR text, for the whole frame and for the window the question
names. That isolates the crop's effect on OCR quality without re-running the
text model on top.

    python3 ocr_crop.py --dir /tmp/synth/eval2

License: MIT. Free-only: tesseract + Pillow.
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time

from PIL import Image

import eval_battery as eb

VLM_DIR = eb.VLM_DIR
LLAMA = os.path.join(VLM_DIR, "llama-b12811")


def ocr_retrieval(image, expected, scale=2):
    im = Image.open(image).convert("L")
    if scale != 1:
        im = im.resize((im.width * scale, im.height * scale), Image.LANCZOS)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    try:
        p = subprocess.run(["tesseract", tmp.name, "stdout", "--psm", "3"],
                           capture_output=True, text=True, check=True)
        text = p.stdout
    finally:
        os.unlink(tmp.name)
    return eb.norm(expected) in eb.norm(text), text


def window_rect(meta, app):
    w = next((x for x in meta["windows"] if app and x["app"] == app),
             next(x for x in meta["windows"] if x["focused"]))
    pad = 6
    return (max(0, w["x"] - pad), max(0, w["y"] - pad),
            min(meta["width"], w["x"] + w["w"] + pad),
            min(meta["height"], w["y"] + w["h"] + pad))


def ask_text(port, ocr, question):
    import urllib.request
    body = {"messages": [
        {"role": "system", "content":
         "You answer questions about the user's screen using ONLY the OCR "
         "text provided. If the answer is not in the OCR text, say that the "
         "OCR text does not show it. Do not invent anything."},
        {"role": "user", "content":
         "OCR text from my screen:\n" + ocr + "\n\nQuestion: " + question}],
        "max_tokens": 60, "temperature": 0.0}
    t0 = time.time()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/v1/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    r = json.load(urllib.request.urlopen(req, timeout=300))
    return r["choices"][0]["message"]["content"].strip(), time.time() - t0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", default="/tmp/synth/eval2")
    ap.add_argument("--scale", type=int, default=2)
    ap.add_argument("--end-to-end", action="store_true")
    args = ap.parse_args()

    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    qa = [r for r in qa if r["family"] == "text"]
    import fork_router as fr

    port = None
    if args.end_to_end:
        qwen = os.path.join(VLM_DIR, "qwen2.5-0.5b-instruct-q8_0.gguf")
        subprocess.Popen([os.path.join(LLAMA, "llama-server"), "-m", qwen,
                          "--port", "18091", "-c", "4096"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(60):
            if eb.server_up(18091):
                break
            time.sleep(1)
        else:
            sys.exit("llama-server did not come up")
        port = 18091

    res = {}
    t0 = time.time()
    for r in qa:
        png = os.path.join(args.dir, r["image"])
        meta = json.load(open(os.path.join(args.dir, r["image"].replace(".png", ".json"))))
        app = fr.target_app(r["question"])
        rect = window_rect(meta, app)
        tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
        Image.open(png).crop(rect).save(tmp.name)
        try:
            full_ok, full_text = ocr_retrieval(png, r["answer"], args.scale)
            win_ok, win_text = ocr_retrieval(tmp.name, r["answer"], args.scale)
        finally:
            os.unlink(tmp.name)
        row = {"expected": r["answer"], "app": app, "rect": list(rect),
               "full": full_ok, "window": win_ok,
               "full_chars": len(full_text), "window_chars": len(win_text)}
        if port:
            fa, fdt = ask_text(port, full_text, r["question"])
            wa, wdt = ask_text(port, win_text, r["question"])
            row.update({"full_answer": fa, "window_answer": wa,
                        "full_ok_e2e": eb.norm(r["answer"]) in eb.norm(fa),
                        "window_ok_e2e": eb.norm(r["answer"]) in eb.norm(wa),
                        "full_s": round(fdt, 1), "window_s": round(wdt, 1)})
            print(f"   e2e full={'OK ' if row['full_ok_e2e'] else 'MISS'} ({fdt:.1f}s) "
                  f"{fa[:60]!r}\n   e2e win ={'OK ' if row['window_ok_e2e'] else 'MISS'} ({wdt:.1f}s) "
                  f"{wa[:60]!r}", flush=True)
        key = f"{r['image']}|{r['question']}"
        res[key] = row
        print(f"{'OK ' if full_ok else 'MISS'} full / {'OK ' if win_ok else 'MISS'} window "
              f"[{r['image']}] want={r['answer']!r} app={app} "
              f"chars {len(full_text)}->{len(win_text)}", flush=True)

    n = len(res)
    full = sum(v["full"] for v in res.values())
    win = sum(v["window"] for v in res.values())
    dt = time.time() - t0
    print(f"\n== retrieval on {n} text questions, {args.scale}x upscale: "
          f"full frame {full}/{n}, window crop {win}/{n}, {dt/max(1,n)/2:.2f}s per OCR pass")
    if port:
        fe = sum(v["full_ok_e2e"] for v in res.values())
        we = sum(v["window_ok_e2e"] for v in res.values())
        fs = sum(v["full_s"] for v in res.values()) / n
        ws = sum(v["window_s"] for v in res.values()) / n
        print(f"== end-to-end answers: full frame {fe}/{n} at {fs:.1f}s, "
              f"window crop {we}/{n} at {ws:.1f}s")
    out = os.path.join(args.dir, "results_ocr_crop.json")
    json.dump({"scale": args.scale, "n": n, "full": full, "window": win, "rows": res},
              open(out, "w"), indent=1)
    print(f"wrote {out}")


if __name__ == "__main__":
    sys.exit(main())
