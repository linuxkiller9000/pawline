#!/usr/bin/env python3
"""Dump the scene graph for synthetic eval scenes (window rects, titles, apps).

Why: the eval battery only knows PNG + qa.jsonl, but a real Pawline session on
niri also knows what the compositor reports over IPC - which windows exist,
their app_id, title, geometry, and which one is focused. The no-training
router experiments need exactly those fields, and synth_desktop.py renders from
that graph without writing it, so this re-derives the graph for the same seeds
(the generator is deterministic) and writes <seed>.json next to the PNG.

It refuses to write a sidecar when the regenerated PNG does not match the file
on disk, so a stale or mismatched scene graph can never be used silently.

Usage:
  python3 scene_meta.py --dir /tmp/synth/eval2 --eval --count 8 --seed-base 10000

License: MIT.
"""
import argparse
import hashlib
import json
import os
import random

import synth_desktop as sd


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True, help="dir with <seed>.png + qa.jsonl")
    ap.add_argument("--seed-base", type=int, default=10000)
    ap.add_argument("--count", type=int, default=8)
    ap.add_argument("--res", default="1920x1080", choices=["1920x1080", "1366x768"])
    ap.add_argument("--eval", dest="eval", action="store_true", default=True,
                    help="scene was generated with --eval (held-out corpora; default)")
    ap.add_argument("--no-eval", dest="eval", action="store_false",
                    help="scene was generated without --eval (train corpora)")
    args = ap.parse_args()
    W, H = sd.parse_res(args.res)

    ok = 0
    for i in range(args.count):
        seed = args.seed_base + i
        scene = sd.make_scene(random.Random(seed), args.eval, W, H)
        img = sd.render(scene, W, H)
        name = f"{seed:05d}.png"
        path = os.path.join(args.dir, name)
        tmp = os.path.join("/tmp", f"scenecheck_{seed}.png")
        img.save(tmp)
        if not os.path.exists(path):
            os.unlink(tmp)
            print(f"skip {name}: no PNG on disk")
            continue
        if sha256(tmp) != sha256(path):
            os.unlink(tmp)
            print(f"MISMATCH {name}: regenerated render differs from the PNG on "
                  f"disk - not writing a sidecar (wrong --res/--eval/seed-base?)")
            continue
        os.unlink(tmp)
        meta = {
            "image": name, "width": W, "height": H,
            "clock": scene["clock"],
            "focus_accent_name": scene["focus_accent_name"],
            "focus_accent_rgb": list(scene["focus_accent"][1]),
            "indicator_name": scene["indicator_name"],
            "progress_name": scene["progress_name"],
            "progress_pct": scene["progress_pct"],
            "progress_win": scene["progress_win"],
            "n_workspaces": scene["n_workspaces"], "active_ws": scene["active_ws"],
            "windows": [{"app": w["app"], "app_name": w["app_name"],
                         "title": w["title"], "x": w["x"], "y": w["y"],
                         "w": w["w"], "h": w["h"], "focused": w["focused"],
                         "dark": w["dark"], "content": w["content"]}
                        for w in scene["windows"]],
        }
        with open(os.path.join(args.dir, f"{seed:05d}.json"), "w") as f:
            json.dump(meta, f, indent=1)
        ok += 1
    print(f"wrote {ok}/{args.count} scene sidecars to {args.dir}")


if __name__ == "__main__":
    main()
