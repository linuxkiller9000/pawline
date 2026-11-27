#!/usr/bin/env python3
"""No-training "fork" v1: a screen-question router (no weights change at all).

The premise measured in EXPERIMENTS 4f: a 256M/500M SmolVLM asked a desktop
question in one shot is wrong most of the time (13/86 and worse on text), and
prompting does not fix refusal. But most desktop questions are not really
visual - they are questions about window STATE, which the compositor already
reports exactly, and about window TEXT, which OCR reads exactly. So this
router answers from:

  metadata   window list from niri IPC (app_id, title, rect, focused)
             -> count, focus, spatial, app, presence   (exact, no inference)
  pixels     the focused window's border ring, sampled and matched to the
             accent palette -> color, color_neg        (deterministic)
  OCR        tesseract TSV, words bucketed per window -> clock, text
  refusal    no matching evidence -> "not visible on the screen"

Nothing here is trained. The model is not called at all: that is the point -
it measures the ceiling of the non-VLM path so the VLM has something honest to
beat.

PROVENANCE OF METADATA: in production niri IPC supplies the window list. In
this harness it comes from scene_meta.py, which re-derives the generator's
scene graph for the same seeds and refuses to write a sidecar when the
regenerated PNG differs from the file on disk. So these numbers are "router
with perfect compositor metadata on synthetic scenes", not "router on a live
desktop". Questions are routed from their TEXT only - the family label in
qa.jsonl is used for the confusion table afterwards, never for answering.

Usage:
  python3 scene_meta.py --dir /tmp/synth/eval2 --eval --count 8
  python3 fork_router.py --dir /tmp/synth/eval2

License: MIT. Free-only: tesseract + Pillow.
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

ACCENTS = [
    ("blue", (66, 133, 244)), ("red", (234, 67, 53)), ("green", (52, 168, 83)),
    ("yellow", (251, 188, 5)), ("purple", (149, 117, 205)),
    ("orange", (255, 112, 67)), ("teal", (0, 150, 136)), ("pink", (236, 64, 122)),
]
APP_WORDS = {
    "terminal": "terminal", "firefox": "browser", "browser": "browser",
    "neovim": "editor", "editor": "editor", "nautilus": "files",
    "files": "files", "signal": "chat", "chat": "chat",
    "thunderbird": "mail", "mail": "mail", "inbox": "mail", "email": "mail",
}


def ocr_lines(png, scale=2):
    """tesseract TSV -> lines with boxes in original-image pixels."""
    im = Image.open(png).convert("L")
    w, h = im.size
    im = im.resize((w * scale, h * scale), Image.LANCZOS)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    try:
        p = subprocess.run(["tesseract", tmp.name, "stdout", "--psm", "3", "tsv"],
                           capture_output=True, text=True, check=True)
    finally:
        os.unlink(tmp.name)
    groups = collections.OrderedDict()
    for row in p.stdout.splitlines()[1:]:
        c = row.split("\t")
        if len(c) < 12:
            continue
        try:
            left, top, ww, hh = (int(c[6]) / scale, int(c[7]) / scale,
                                 int(c[8]) / scale, int(c[9]) / scale)
        except ValueError:
            continue
        key = (c[1], c[2], c[3], c[4])
        g = groups.setdefault(key, {"words": [], "x0": left, "y0": top,
                                    "x1": left + ww, "y1": top + hh})
        if c[11].strip():
            g["words"].append(c[11])
        g["x0"], g["y0"] = min(g["x0"], left), min(g["y0"], top)
        g["x1"], g["y1"] = max(g["x1"], left + ww), max(g["y1"], top + hh)
    lines = [{"text": " ".join(g["words"]).strip(), "x0": g["x0"], "y0": g["y0"],
              "x1": g["x1"], "y1": g["y1"]}
             for g in groups.values() if g["words"]]
    lines.sort(key=lambda l: (round(l["y0"]), l["x0"]))
    return lines


def in_rect(line, rect, pad=0):
    x, y, w, h = rect
    cy = (line["y0"] + line["y1"]) / 2
    cx = (line["x0"] + line["x1"]) / 2
    return (x - pad) <= cx <= (x + w + pad) and (y - pad) <= cy <= (y + h + pad)


def border_color(png, rect):
    """Sample the focused window's 3px border ring and name the nearest accent."""
    im = Image.open(png).convert("RGB")
    x, y, w, h = rect
    pts = [(x + w // 2, y - 2), (x + w // 2, y + h + 1), (x - 2, y + h // 2),
           (x + w + 1, y + h // 2), (x + 4, y - 2), (x + w - 4, y + h + 1)]
    counts = collections.Counter()
    for px in pts:
        if 0 <= px[0] < im.width and 0 <= px[1] < im.height:
            counts[im.getpixel(px)] += 1
    if not counts:
        return None, None
    rgb = counts.most_common(1)[0][0]
    best = min(ACCENTS, key=lambda a: sum((a[1][i] - rgb[i]) ** 2 for i in range(3)))
    return best[0], rgb


def route(q):
    ql = q.lower()
    if re.search(r"how many windows|count the open windows|how many app windows", ql):
        return "count"
    if re.search(r"which window is (focused|currently active)|title of the focused window"
                 r"|name the active window", ql):
        return "focus"
    if re.search(r"is the focused window'?s? border|border .* (yellow|blue|red|green|purple|orange|teal|pink)", ql):
        return "color_neg"
    if re.search(r"colou?r", ql):
        return "color"
    if re.search(r"what time|clock|status bar", ql):
        return "clock"
    if re.search(r"leftmost|far left", ql):
        return "spatial"
    if re.search(r"rightmost|far right", ql):
        return "app"
    if re.search(r"is there an? .* window", ql):
        return "presence"
    if re.search(r"command|url|address bar|first line|chat message|last message"
                 r"|most recent|email subject|read the", ql):
        return "text"
    return "absent"


def target_app(q):
    ql = q.lower()
    for word, app in APP_WORDS.items():
        if re.search(r"\b" + re.escape(word) + r"\b", ql):
            return app
    return None


def crop_ocr(png, rect, scale, psm):
    im = Image.open(png).convert("L").crop(rect)
    im = im.resize((im.width * scale, im.height * scale), Image.LANCZOS)
    tmp = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    im.save(tmp.name)
    try:
        p = subprocess.run(["tesseract", tmp.name, "stdout", "--psm", psm],
                           capture_output=True, text=True, check=True)
    finally:
        os.unlink(tmp.name)
    return p.stdout


def window_text_lines(png, w, scale=3):
    """OCR one window's body at 3x, psm 6. On a small crop the line grouping
    is clean; on the full 1920px frame tesseract splits one chat line into two
    'lines' and drops the editor's line numbers."""
    out = crop_ocr(png, (w["x"], w["y"] + 30, w["x"] + w["w"], w["y"] + w["h"]),
                   scale, "6")
    return [t.strip() for t in out.splitlines() if t.strip()]


def top_bar_clock(png):
    """The 16px bold clock in the bar is invisible to a 2x full-frame pass;
    a 4x strip with psm 7 reads it exactly."""
    im = Image.open(png)
    out = crop_ocr(png, (0, 0, im.width, 32), 4, "7")
    m = re.search(r"\b(\d{1,2}:\d{2})\b", out)
    return m.group(1) if m else None


def answer_text(q, meta, png):
    app = target_app(q)
    wins = [w for w in meta["windows"] if app is None or w["app"] == app]
    if not wins:
        return "not visible on the screen"
    w = wins[0]
    body = window_text_lines(png, w)
    if not body:
        return "not visible on the screen"
    if w["app"] == "terminal":
        cmds = [t for t in body if re.match(r"^[\$s§]\s+\S", t)]
        if cmds:
            return re.sub(r"^[\$s§]\s+", "", cmds[-1])
        return body[-1]
    if w["app"] == "browser":
        for t in body:
            if re.search(r"https?|www\.|\.(com|org|net|io|dev|rs|co)\b", t):
                return t
        return body[0]
    if w["app"] == "editor":
        # first code line: strip the drawn line number when OCR keeps it
        return re.sub(r"^\s*\d{1,3}\s+", "", body[0])
    if w["app"] == "chat":
        msgs = [t for t in body if ":" in t]
        if msgs:
            return msgs[-1].split(":", 1)[1].strip()
        return body[-1]
    if w["app"] == "mail":
        subs = [t for t in body if "@" not in t and len(t) > 3]
        if subs:
            return subs[0]
        return body[0]
    return body[0]


def has_value_evidence(q, lines):
    """Is there actual evidence for the absent-family subject on screen?
    The top bar draws the labels 'net vol bat' with no values, so the label
    alone is not evidence - look for a value."""
    text = " ".join(l["text"] for l in lines).lower()
    ql = q.lower()
    if "battery" in ql:
        return bool(re.search(r"\d{1,3}\s*%", text))
    if "weather" in ql:
        return bool(re.search(r"\d+\s*°|\b(sunny|rain|cloud)\w*", text))
    if "wifi" in ql or "wi-fi" in ql:
        return bool(re.search(r"(wifi|ssid)\s*[:=]?\s*\w{3,}", text))
    if "song" in ql or "music" in ql:
        return bool(re.search(r"\b(playing|track|artist)\b", text))
    if "notification" in ql:
        return bool(re.search(r"\d+\s*(unread|notifications?)", text))
    return False


def answer(q, meta, lines, png):
    r = route(q)
    wins = meta["windows"]
    if r == "count":
        return r, str(len(wins))
    if r == "focus":
        fw = next((w for w in wins if w["focused"]), None)
        return r, fw["title"] if fw else "not visible on the screen"
    if r == "spatial":
        # same stable ordering the scene uses: stacked windows share an x, so
        # tie-break on list order, not on which one min()/max() hits first.
        return r, sorted(wins, key=lambda w: w["x"])[0]["title"]
    if r == "app":
        return r, sorted(wins, key=lambda w: w["x"])[-1]["app_name"]
    if r == "color":
        fw = next((w for w in wins if w["focused"]), None)
        name, _ = border_color(png, (fw["x"], fw["y"], fw["w"], fw["h"]))
        return r, name or "not visible on the screen"
    if r == "color_neg":
        fw = next((w for w in wins if w["focused"]), None)
        name, _ = border_color(png, (fw["x"], fw["y"], fw["w"], fw["h"]))
        asked = next((a[0] for a in ACCENTS if a[0] in q.lower()), None)
        return r, ("yes" if asked and name == asked else "no")
    if r == "clock":
        clk = top_bar_clock(png)
        if clk:
            return r, clk
        for l in lines:
            m = re.search(r"\b(\d{1,2}:\d{2})\b", l["text"])
            if m and l["y1"] < 40:
                return r, m.group(1)
        for l in lines:
            m = re.search(r"\b(\d{1,2}:\d{2})\b", l["text"])
            if m:
                return r, m.group(1)
        return r, "not visible on the screen"
    if r == "presence":
        asked = target_app(q)
        present = {w["app"] for w in wins}
        if asked is None:
            return r, "not visible on the screen"
        return r, ("yes" if asked in present else "no")
    if r == "text":
        return r, answer_text(q, meta, png)
    return r, ("not visible on the screen" if not has_value_evidence(q, lines)
               else answer_text(q, meta, png))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True, help="eval dir (PNG + qa.jsonl + <seed>.json)")
    ap.add_argument("--results", default=None)
    args = ap.parse_args()
    results_path = args.results or os.path.join(args.dir, "results.json")

    qa = [json.loads(l) for l in open(os.path.join(args.dir, "qa.jsonl"))]
    images = sorted({r["image"] for r in qa})
    res = json.load(open(results_path)) if os.path.exists(results_path) else {}
    res.setdefault("router", {})
    confusion = collections.Counter()

    for img in images:
        png = os.path.join(args.dir, img)
        meta_path = os.path.join(args.dir, img.replace(".png", ".json"))
        if not os.path.exists(meta_path):
            raise SystemExit(f"missing scene sidecar {meta_path} - run scene_meta.py first")
        meta = json.load(open(meta_path))
        t_ocr = time.time()
        lines = ocr_lines(png)
        ocr_dt = time.time() - t_ocr
        rows = [r for r in qa if r["image"] == img]
        for r in eb.pick_questions(rows):
            qid = f"{img}|{r['family']}|{r['question']}"
            if qid in res["router"]:
                continue
            t0 = time.time()
            rt, got = answer(r["question"], meta, lines, png)
            dt = time.time() - t0
            confusion[(r["family"], rt)] += 1
            ok = eb.norm(r["answer"]) in eb.norm(got) and eb.norm(r["answer"]) != ""
            peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            res["router"][qid] = {"ok": ok, "expected": r["answer"], "got": got,
                                  "seconds": round(dt + (ocr_dt if rt != "cache" else 0), 1),
                                  "peak_kib": peak, "route": rt}
            print(f"{'OK ' if ok else 'MISS'} [router/{rt}] {img} {r['family']:9s} "
                  f"want={r['answer']!r} got={got!r} ({dt:.2f}s)", flush=True)
            json.dump(res, open(results_path, "w"), indent=1)

    rows = list(res["router"].values())
    fams = collections.defaultdict(lambda: [0, 0])
    for k, v in res["router"].items():
        f = k.split("|")[1]
        fams[f][1] += 1
        fams[f][0] += v["ok"]
    print(f"== router: {sum(v['ok'] for v in rows)}/{len(rows)} ==")
    for f in eb.FAMILY_ORDER:
        if f in fams:
            print(f"   {f:9s} {fams[f][0]}/{fams[f][1]}")
    print("-- route vs family (diagnostic only) --")
    for (fam, rt), n in sorted(confusion.items()):
        print(f"   {fam:9s} -> {rt:9s} {n}")


if __name__ == "__main__":
    main()
