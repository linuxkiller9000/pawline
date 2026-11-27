#!/usr/bin/env python3
"""Make a 1366x768 copy of an eval set, for the Dell's native resolution.

The scenes are drawn at 1920x1080 in the sandbox; the N5010 is 1366x768. Rather
than re-generate scenes, this resamples each image and scales the sidecar's
window rectangles with it, so the same questions can be asked at his resolution.
Caveat: text is resampled, not re-rendered, so it is slightly softer than a real
1366x768 desktop.

    python3 rescale_set.py --src /tmp/synth/eval3 --dst /tmp/synth/eval4 --width 1366
"""
import argparse, json, os, shutil
from PIL import Image

ap = argparse.ArgumentParser()
ap.add_argument("--src", required=True)
ap.add_argument("--dst", required=True)
ap.add_argument("--width", type=int, default=1366)
a = ap.parse_args()
os.makedirs(a.dst, exist_ok=True)
shutil.copy(os.path.join(a.src, "qa.jsonl"), os.path.join(a.dst, "qa.jsonl"))
imgs = sorted(f for f in os.listdir(a.src) if f.endswith(".png"))
h = 0
for f in imgs:
    im = Image.open(os.path.join(a.src, f)).convert("RGB")
    sx = a.width / im.width
    h = round(im.height * sx)
    im.resize((a.width, h), Image.LANCZOS).save(os.path.join(a.dst, f))
    side = json.load(open(os.path.join(a.src, f.replace(".png", ".json"))))
    side["width"], side["height"] = a.width, h
    for w in side.get("windows", []):
        for k in ("x", "y", "w", "h"):
            w[k] = round(w[k] * sx)
    json.dump(side, open(os.path.join(a.dst, f.replace(".png", ".json")), "w"))
print(f"{len(imgs)} images at {a.width}x{h} in {a.dst}")
