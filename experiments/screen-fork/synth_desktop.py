#!/usr/bin/env python3
"""Synthetic niri-style desktop screenshot + VQA dataset generator.

Why this exists: there is no public dataset of Linux DESKTOP screenshots
(ScreenQA is Android, WebSight is web pages, Rico is Android). Fine-tuning
SmolVLM for Pawline screen questions needs desktop data, so we render our
own. Because every pixel comes from a scene graph we control, the QA
labels are exact - no human annotation, no label noise.

Renders a fake niri (scrollable-tiling Wayland compositor) desktop:
top bar with workspaces + clock + tray, windows tiled in columns with
gaps, one focused window with a colored border. Window content is drawn
from small controlled text corpora (terminal, browser, editor, files,
chat), so questions like "what is the last command in the terminal" have
a known-exact answer.

Output per scene: <seed>.png + one line in qa.jsonl:
  {"image": "<seed>.png", "question": ..., "answer": ..., "family": ...}
Families: count, title, app, focus, color, clock, text, spatial, presence.

Seeds: training scenes from --seed-base upward; the eval battery uses
seed base 10000 by default so train/eval never overlap.

Deterministic: same seed -> same PNG bytes and same QA rows.

License: MIT (code). Generated images/QA are dedicated CC BY 4.0,
"Pawline synthetic desktop dataset" - attribution appreciated so the
provenance of the labels stays traceable. Free-only: uses Pillow only.

Usage:
  synth_desktop.py --out data/train --count 200 --seed-base 0
  synth_desktop.py --out data/eval  --count 8   --seed-base 10000
"""
import argparse
import json
import os
import random

from PIL import Image, ImageDraw, ImageFont

W, H = 1920, 1080
BAR_H = 28
GAP = 8
BORDER = 3

FONT_DIR = "/usr/share/fonts/truetype/dejavu"


def font(size, bold=False, mono=False):
    name = "DejaVuSansMono-Bold.ttf" if (bold and mono) else \
           "DejaVuSansMono.ttf" if mono else \
           "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    return ImageFont.truetype(os.path.join(FONT_DIR, name), size)


# Named colors (answers to color questions come from these names).
ACCENTS = [
    ("blue", (66, 133, 244)),
    ("red", (234, 67, 53)),
    ("green", (52, 168, 83)),
    ("yellow", (251, 188, 5)),
    ("purple", (149, 117, 205)),
    ("orange", (255, 112, 67)),
    ("teal", (0, 150, 136)),
    ("pink", (236, 64, 122)),
]
BGS = [(24, 24, 32), (18, 28, 38), (30, 24, 20), (16, 32, 24), (28, 20, 36)]
BAR_BG = (12, 12, 18)
WIN_BG = (250, 250, 250)
WIN_BG_DARK = (32, 34, 40)
UNFOCUSED_BORDER = (110, 110, 120)
TITLE_FG = (240, 240, 245)

TERMINAL_CMDS = [
    "ls -la ~/Downloads", "git status", "cargo build --release",
    "sudo pacman -Syu", "htop", "cd ~/projects/pawline", "nvim main.rs",
    "systemctl --user status pipewire", "ping 1.1.1.1", "df -h",
    "grep -r TODO src/", "python3 synth_desktop.py", "ssh dell@192.168.1.5",
    "journalctl -b -p err", "make -j8",
]
TERMINAL_OUTS = [
    "total 128", "nothing to commit, working tree clean", "Compiling pawline-core v0.1.0",
    "Packages (12) linux-6.17  firefox-133", "Tasks: 142 total, 1 running",
    "64 bytes from 1.1.1.1: time=12.3 ms", "Filesystem  Size  Used Avail",
    "Finished release profile in 42s", "No errors found", "drwxr-xr-x 5 user user",
]
URLS = [
    "https://github.com/linuxkiller9000/pawline", "https://en.wikipedia.org/wiki/Wayland",
    "https://huggingface.co/HuggingFaceTB/SmolVLM-500M-Instruct", "https://doc.rust-lang.org/book/",
    "https://news.ycombinator.com", "https://github.com/YaLTeR/niri",
]
PAGE_TITLES = [
    "Wayland (protocol) - Wikipedia", "The Rust Programming Language",
    "SmolVLM-500M-Instruct - Hugging Face", "niri: A scrollable-tiling Wayland compositor",
    "Hacker News", "pawline: local-first assistant",
]
PAGE_LINES = [
    "Wayland is a communication protocol that specifies", "Welcome! This book will teach you about",
    "SmolVLM is a compact multimodal model for", "niri arranges windows in columns on an",
    "1. Show HN: A tiling compositor", "A local-first assistant for Linux",
    "the display server and its clients.", "the Rust programming language.",
]
CODE_LINES = [
    "fn main() {", "    let state = AppState::new();", "    loop {",
    "        state.poll_events();", "    }", "}", "use pawline_core::router;",
    "struct Window {", "    title: String,", "    focused: bool,", "}",
    "impl Router {", "    fn route(&self, msg: &str) {", "}", "#[cfg(test)]",
]
FILES_LISTS = [
    ["Documents", "Downloads", "Pictures", "Projects", "notes.txt"],
    ["report.pdf", "budget.ods", "screenshot.png", "archive.tar.zst", "todo.md"],
    ["pawline", "niri", "dotfiles", "music", "backups"],
]
CHAT_MSGS = [
    ("mama", "call me when you wake up"), ("kareem", "match at 8 tonight?"),
    ("sara", "did you push the branch?"), ("ahmed", "the build works!"),
    ("nour", "send me the notes please"), ("omar", "niri update broke my config"),
]
APP_NAMES = {
    "terminal": "Terminal", "browser": "Firefox", "editor": "Neovim",
    "files": "Nautilus", "chat": "Signal",
}


def draw_text_fit(draw, xy, text, fnt, fill, max_w):
    """Draw text truncated with ellipsis to fit max_w."""
    while text and draw.textlength(text, font=fnt) > max_w:
        text = text[:-1]
    if text:
        draw.text(xy, text, font=fnt, fill=fill)


def make_scene(rng):
    """Build the scene graph. Returns dict with everything the renderer
    and the QA generator need."""
    n_cols = rng.choice([1, 2, 2, 3, 3, 4])
    accent_names = [a[0] for a in ACCENTS]
    focus_accent = rng.choice(ACCENTS)
    bg = rng.choice(BGS)
    clock = f"{rng.randint(0, 23):02d}:{rng.randint(0, 59):02d}"
    n_workspaces = rng.randint(2, 6)
    active_ws = rng.randint(1, n_workspaces)

    # Column layout: each column gets 1 or 2 stacked windows.
    avail_w = W - GAP * (n_cols + 1)
    col_w = avail_w // n_cols
    avail_h = H - BAR_H - GAP * 2
    apps = ["terminal", "browser", "editor", "files", "chat"]
    rng.shuffle(apps)
    windows = []
    wi = 0
    for c in range(n_cols):
        stack = 1 if (n_cols >= 3 or rng.random() < 0.5) else rng.choice([1, 2])
        stack = min(stack, 2)
        for s in range(stack):
            app = apps[wi % len(apps)]
            wi += 1
            x = GAP + c * (col_w + GAP)
            y = BAR_H + GAP + s * ((avail_h - GAP) // stack + GAP)
            h = (avail_h - GAP * (stack - 1)) // stack
            windows.append({
                "app": app,
                "app_name": APP_NAMES[app],
                "title": "",
                "x": x, "y": y, "w": col_w, "h": h,
                "focused": False,
                "dark": app in ("terminal", "editor"),
                "content": {},
            })
    focused_idx = rng.randrange(len(windows))
    windows[focused_idx]["focused"] = True

    # Fill content + titles per app.
    for w in windows:
        app = w["app"]
        if app == "terminal":
            cmds = rng.sample(TERMINAL_CMDS, 3)
            outs = rng.sample(TERMINAL_OUTS, 2)
            w["content"] = {"cmds": cmds, "outs": outs, "last_cmd": cmds[-1]}
            w["title"] = f"user@pawline: ~"
        elif app == "browser":
            i = rng.randrange(len(URLS))
            lines = rng.sample(PAGE_LINES, 4)
            w["content"] = {"url": URLS[i], "page_title": PAGE_TITLES[i], "lines": lines}
            w["title"] = f"{PAGE_TITLES[i]} - Firefox"
        elif app == "editor":
            lines = rng.sample(CODE_LINES, min(8, len(CODE_LINES)))
            fname = rng.choice(["main.rs", "router.rs", "laya.rs", "bridge.rs"])
            w["content"] = {"lines": lines, "file": fname, "first_line": lines[0]}
            w["title"] = f"{fname} - Neovim"
        elif app == "files":
            items = rng.choice(FILES_LISTS)
            w["content"] = {"items": items, "first_item": items[0], "count": len(items)}
            w["title"] = "Home - Nautilus"
        elif app == "chat":
            msgs = rng.sample(CHAT_MSGS, 3)
            w["content"] = {"msgs": msgs, "last_sender": msgs[-1][0], "last_msg": msgs[-1][1]}
            w["title"] = "Signal"
    return {
        "windows": windows, "clock": clock, "bg": bg,
        "focus_accent": focus_accent, "focus_accent_name": focus_accent[0],
        "n_workspaces": n_workspaces, "active_ws": active_ws,
        "accent_names": accent_names,
    }


def render(scene):
    img = Image.new("RGB", (W, H), scene["bg"])
    d = ImageDraw.Draw(img)
    f_bar = font(16, bold=True)
    f_title = font(14, bold=True)
    f_body = font(15)
    f_mono = font(15, mono=True)

    # Top bar (waybar style).
    d.rectangle([0, 0, W, BAR_H], fill=BAR_BG)
    x = 12
    for i in range(1, scene["n_workspaces"] + 1):
        active = i == scene["active_ws"]
        col = scene["focus_accent"][1] if active else (90, 90, 100)
        d.ellipse([x, 9, x + 10, 19], fill=col)
        x += 20
    clk = scene["clock"]
    tw = d.textlength(clk, font=f_bar)
    d.text(((W - tw) / 2, 5), clk, font=f_bar, fill=(235, 235, 240))
    d.text((W - 150, 6), "net  vol  bat", font=font(13), fill=(150, 150, 160))

    # Windows.
    for w in scene["windows"]:
        x0, y0, ww, hh = w["x"], w["y"], w["w"], w["h"]
        bcol = scene["focus_accent"][1] if w["focused"] else UNFOCUSED_BORDER
        d.rectangle([x0 - BORDER, y0 - BORDER, x0 + ww + BORDER, y0 + hh + BORDER], fill=bcol)
        body_bg = WIN_BG_DARK if w["dark"] else WIN_BG
        d.rectangle([x0, y0, x0 + ww, y0 + 30], fill=(44, 46, 54))
        d.text((x0 + 10, y0 + 6), w["title"], font=f_title, fill=TITLE_FG)
        # window controls (right): minimize, maximize, close dots
        for i, c in enumerate([(251, 188, 5), (52, 168, 83), (234, 67, 53)]):
            cx = x0 + ww - 24 - i * 22
            d.ellipse([cx, y0 + 10, cx + 12, y0 + 22], fill=c)
        d.rectangle([x0, y0 + 30, x0 + ww, y0 + hh], fill=body_bg)
        tx = x0 + 12
        ty = y0 + 42
        fg = (230, 230, 235) if w["dark"] else (30, 30, 35)
        dim = (120, 190, 130) if w["dark"] else (60, 110, 70)
        c = w["content"]
        if w["app"] == "terminal":
            for j, cmd in enumerate(c["cmds"]):
                draw_text_fit(d, (tx, ty), f"$ {cmd}", f_mono, dim, ww - 24)
                ty += 24
                if j < len(c["outs"]):
                    draw_text_fit(d, (tx, ty), c["outs"][j], f_mono, fg, ww - 24)
                    ty += 24
        elif w["app"] == "browser":
            d.rectangle([tx, ty, x0 + ww - 12, ty + 26], fill=(226, 228, 233))
            draw_text_fit(d, (tx + 8, ty + 4), c["url"], f_mono, (40, 40, 50), ww - 40)
            ty += 40
            for ln in c["lines"]:
                draw_text_fit(d, (tx, ty), ln, f_body, fg, ww - 24)
                ty += 26
        elif w["app"] == "editor":
            for j, ln in enumerate(c["lines"]):
                draw_text_fit(d, (tx, ty), f"{j+1:2d} {ln}", f_mono, fg, ww - 24)
                ty += 24
        elif w["app"] == "files":
            for it in c["items"]:
                d.rectangle([tx, ty, tx + 18, ty + 18], outline=(90, 90, 150), width=2)
                draw_text_fit(d, (tx + 30, ty), it, f_body, fg, ww - 60)
                ty += 30
        elif w["app"] == "chat":
            for sender, msg in c["msgs"]:
                draw_text_fit(d, (tx, ty), f"{sender}:", f_body, (30, 90, 160), ww - 24)
                draw_text_fit(d, (tx + 90, ty), msg, f_body, fg, ww - 120)
                ty += 28
    return img


def gen_qa(scene, rng):
    wins = scene["windows"]
    qa = []

    def add(family, q, a):
        qa.append({"family": family, "question": q, "answer": str(a)})

    # count
    add("count", rng.choice([
        "How many windows are open on the desktop?",
        "How many windows are visible?",
        "Count the open windows.",
    ]), len(wins))

    # focus
    fw = next(w for w in wins if w["focused"])
    add("focus", rng.choice([
        "Which window is focused?",
        "What is the title of the focused window?",
        "Which window is currently active?",
    ]), fw["title"])
    add("color", rng.choice([
        "What color is the border of the focused window?",
        "What colour is the active window border?",
    ]), scene["focus_accent_name"])

    # clock
    add("clock", rng.choice([
        "What time is shown in the top bar?",
        "What does the clock in the panel say?",
        "What time is it on the screen?",
    ]), scene["clock"])

    # spatial: leftmost / rightmost
    order = sorted(wins, key=lambda w: w["x"])
    add("spatial", rng.choice([
        "What is the title of the leftmost window?",
        "Which window is on the far left?",
    ]), order[0]["title"])
    add("app", rng.choice([
        "Which application is in the rightmost window?",
        "What app is open in the far right window?",
    ]), order[-1]["app_name"])

    # text-reading questions (exact strings we drew)
    for w in wins:
        c, app = w["content"], w["app"]
        if app == "terminal":
            add("text", rng.choice([
                "What is the last command run in the terminal?",
                "What command was typed most recently in the terminal window?",
            ]), c["last_cmd"])
        elif app == "browser":
            add("text", rng.choice([
                "What URL is open in the browser?",
                "What is the address in the browser address bar?",
            ]), c["url"])
        elif app == "editor":
            add("text", rng.choice([
                "What is the first line of code shown in the editor?",
                "Read the first line in the editor window.",
            ]), c["first_line"])
        elif app == "chat":
            add("text", rng.choice([
                "What is the last message in the chat window?",
                "What does the most recent chat message say?",
            ]), c["last_msg"])

    # presence (yes/no) - pick one present app and one absent app
    present = {w["app"] for w in wins}
    absent = [a for a in APP_NAMES if a not in present]
    pick_yes = rng.choice(sorted(present))
    add("presence", f"Is there a {APP_NAMES[pick_yes]} window open?", "yes")
    if absent:
        pick_no = rng.choice(absent)
        add("presence", f"Is there a {APP_NAMES[pick_no]} window open?", "no")
    return qa


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--count", type=int, default=200)
    ap.add_argument("--seed-base", type=int, default=0)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    qa_path = os.path.join(args.out, "qa.jsonl")
    with open(qa_path, "w") as f:
        for i in range(args.count):
            seed = args.seed_base + i
            rng = random.Random(seed)
            scene = make_scene(rng)
            img = render(scene)
            name = f"{seed:05d}.png"
            img.save(os.path.join(args.out, name))
            for row in gen_qa(scene, random.Random(seed + 500000)):
                row["image"] = name
                f.write(json.dumps(row) + "\n")
    print(f"wrote {args.count} scenes to {args.out} (qa.jsonl)")


if __name__ == "__main__":
    main()
