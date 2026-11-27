#!/usr/bin/env python3
"""Fixed eval battery for the Pawline screen-question fork.

Compares, on the SAME synthetic desktop scenes and questions (exact labels
from synth_desktop.py), the four approaches Pawline can route between:

  smol256   stock SmolVLM-256M Q8 via llama-mtmd-cli (image downscaled,
            default 960px - the current screen-model default in the app)
  smol500   stock SmolVLM-500M Q8 via llama-mtmd-cli (full 1920px)
  fork      any fine-tuned GGUF via llama-mtmd-cli (--model/--mmproj)
  ocr       the OCR hybrid: 2x Lanczos upscale -> tesseract -> qwen2.5-0.5b
            through llama-server (same pipeline as ocr_screen.py, 4e)

Scoring: normalized containment (lowercase, alphanumerics only). Every
question's answer is known-exact from the generator's scene graph.

Runs ONE method per invocation and merges into results.json, so long
passes can be split across sessions/machines. Deterministic question
selection: per image it takes (count, focus, color, clock, spatial, app,
first text, first presence) = up to 8 questions.

License: MIT. Free-only: llama.cpp + tesseract + Pillow.

Usage:
  python3 synth_desktop.py --out data/eval --count 8 --seed-base 10000
  python3 eval_battery.py --dir data/eval --method ocr
  python3 eval_battery.py --dir data/eval --method smol256
  python3 eval_battery.py --dir data/eval --method smol500
  python3 eval_battery.py --dir data/eval --method fork --model fork.gguf --mmproj mmproj.gguf
  python3 eval_battery.py --dir data/eval --report   # table from results.json
"""
import argparse
import json
import os
import re
import resource
import subprocess
import sys
import tempfile
import time
import urllib.request

VLM_DIR = os.environ.get("PAWLINE_VLM_DIR", "/downloads/vlm")
LLAMA = os.path.join(VLM_DIR, "llama-b12811")
FAMILY_ORDER = ["count", "focus", "color", "color_neg", "clock", "spatial", "app",
                "text", "presence", "absent"]


def norm(s):
    return re.sub(r"[^a-z0-9]+", "", s.lower())


def pick_questions(rows):
    """Deterministic per-image subset: first question of each family, plus
    a second text question when present."""
    by_fam = {}
    for r in rows:
        by_fam.setdefault(r["family"], []).append(r)
    picked = []
    for fam in FAMILY_ORDER:
        if fam in by_fam:
            picked.append(by_fam[fam][0])
    if len(by_fam.get("text", [])) > 1:
        picked.append(by_fam["text"][1])
    return picked


def run_mtmd(model, mmproj, image, question, scale):
    img_path = image
    tmp = None
    if scale:
        from PIL import Image
        im = Image.open(image)
        w, h = im.size
        im = im.resize((int(w * scale), int(h * scale)), Image.LANCZOS)
        tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
        im.save(tmp.name)
        img_path = tmp.name
    try:
        t0 = time.time()
        p = subprocess.run(
            [os.path.join(LLAMA, "llama-mtmd-cli"), "-m", model,
             "--mmproj", mmproj, "--image", img_path,
             "-p", question + " Answer briefly.",
             "--temp", "0", "-n", "48"],
            capture_output=True, text=True, timeout=600)
        dt = time.time() - t0
        # Peak RSS across child processes so far (KiB on Linux) - this is a
        # cumulative max, so it reports the largest run to date; per-method
        # peaks are what the comparison table prints.
        peak_kib = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        out = p.stdout.strip()
        # mtmd-cli echoes nothing extra at tail; keep last non-empty line(s).
        return (out.splitlines()[-1].strip() if out else ""), dt, peak_kib
    finally:
        if tmp:
            os.unlink(tmp.name)


def ocr_text(image):
    from PIL import Image
    im = Image.open(image).convert("L")
    w, h = im.size
    im = im.resize((w * 2, h * 2), Image.LANCZOS)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    try:
        p = subprocess.run(["tesseract", tmp.name, "stdout", "--psm", "3"],
                           capture_output=True, text=True, check=True)
        return p.stdout
    finally:
        os.unlink(tmp.name)


def server_up(port):
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=2)
        return True
    except Exception:
        return False


def run_ocr_hybrid(image, question, port, ocr_cache):
    if image not in ocr_cache:
        t0 = time.time()
        ocr_cache[image] = (ocr_text(image), time.time() - t0)
    text, ocr_dt = ocr_cache[image]
    body = {"messages": [
        {"role": "system", "content":
         "You answer questions about the user's screen using ONLY the OCR "
         "text provided. If the answer is not in the OCR text, say that the "
         "OCR text does not show it. Do not invent anything."},
        {"role": "user", "content":
         "OCR text from my screen:\n" + text + "\n\nQuestion: " + question}],
        "max_tokens": 60, "temperature": 0.0}
    t0 = time.time()
    req = urllib.request.Request(f"http://127.0.0.1:{port}/v1/chat/completions",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    r = json.load(urllib.request.urlopen(req, timeout=300))
    dt = time.time() - t0
    peak_kib = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    return r["choices"][0]["message"]["content"].strip(), dt + ocr_dt, peak_kib


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True, help="eval dir with PNGs + qa.jsonl")
    ap.add_argument("--method", choices=["smol256", "smol500", "fork", "ocr"])
    ap.add_argument("--model"), ap.add_argument("--mmproj")
    ap.add_argument("--scale", type=float, default=None,
                    help="image scale factor (smol256 default 0.5 = 960px)")
    ap.add_argument("--results", default=None, help="results.json path (default: <dir>/results.json)")
    ap.add_argument("--report", action="store_true")
    args = ap.parse_args()
    results_path = args.results or os.path.join(args.dir, "results.json")

    if args.report:
        res = json.load(open(results_path))
        print(f"{'family':10s} " + " ".join(f"{m:>10s}" for m in sorted(res)))
        fams = sorted({k.split('|')[1] for m in res.values() for k in m})
        for fam in fams:
            cells = []
            for m in sorted(res):
                rows = [v for k, v in res[m].items() if k.split('|')[1] == fam]
                cells.append(f"{sum(r['ok'] for r in rows)}/{len(rows):d}".rjust(10))
            print(f"{fam:10s} " + " ".join(cells))
        print(f"{'TOTAL':10s} " + " ".join(
            f"{sum(v['ok'] for v in res[m].values())}/{len(res[m])}".rjust(10)
            for m in sorted(res)))
        print(f"{'avg sec':10s} " + " ".join(
            f"{sum(v['seconds'] for v in res[m].values())/max(1,len(res[m])):>10.1f}"
            for m in sorted(res)))
        print(f"{'peak MiB':10s} " + " ".join(
            f"{max((v.get('peak_kib',0) for v in res[m].values()), default=0)/1024:>10.0f}"
            for m in sorted(res)))
        return

    if not args.method:
        ap.error("--method or --report required")

    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    images = sorted({r["image"] for r in qa})
    res = json.load(open(results_path)) if os.path.exists(results_path) else {}
    res.setdefault(args.method, {})

    server_proc, port, ocr_cache = None, 18089, {}
    if args.method == "ocr":
        qwen = os.path.join(VLM_DIR, "qwen2.5-0.5b-instruct-q8_0.gguf")
        server_proc = subprocess.Popen(
            [os.path.join(LLAMA, "llama-server"), "-m", qwen,
             "--port", str(port), "-c", "4096"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(60):
            if server_up(port):
                break
            time.sleep(1)
        else:
            sys.exit("llama-server did not come up")

    model = args.model or os.path.join(VLM_DIR, {
        "smol256": "SmolVLM-256M-Instruct-Q8_0.gguf",
        "smol500": "SmolVLM-500M-Instruct-Q8_0.gguf",
        "fork": "fork.gguf",
        "ocr": ""}[args.method])
    mmproj = args.mmproj or os.path.join(VLM_DIR, {
        "smol256": "mmproj-SmolVLM-256M-Instruct-Q8_0.gguf",
        "smol500": "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
        "fork": "mmproj-fork.gguf",
        "ocr": ""}[args.method])
    scale = args.scale
    if scale is None:
        scale = 0.5 if args.method == "smol256" else 1.0

    try:
        for img in images:
            rows = [r for r in qa if r["image"] == img]
            for r in pick_questions(rows):
                qid = f"{img}|{r['family']}|{r['question']}"
                if qid in res[args.method]:
                    continue  # resume support
                path = os.path.join(args.dir, img)
                if args.method == "ocr":
                    got, dt, peak = run_ocr_hybrid(path, r["question"], port, ocr_cache)
                else:
                    got, dt, peak = run_mtmd(model, mmproj, path, r["question"], scale)
                ok = norm(r["answer"]) in norm(got) and norm(r["answer"]) != ""
                res[args.method][qid] = {"ok": ok, "expected": r["answer"],
                                         "got": got, "seconds": round(dt, 1),
                                         "peak_kib": peak}
                print(f"{'OK ' if ok else 'MISS'} [{args.method}] {img} {r['family']:9s} "
                      f"want={r['answer']!r} got={got!r} ({dt:.1f}s)", flush=True)
                json.dump(res, open(results_path, "w"), indent=1)
    finally:
        if server_proc:
            server_proc.terminate()

    rows = list(res[args.method].values())
    print(f"== {args.method}: {sum(r['ok'] for r in rows)}/{len(rows)} correct ==")


if __name__ == "__main__":
    main()
