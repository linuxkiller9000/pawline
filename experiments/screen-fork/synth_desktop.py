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
from controlled text corpora (terminal, browser, editor, files, chat,
mail), so questions like "what is the last command in the terminal" have
a known-exact answer.

Output per scene: <seed>.png + one line in qa.jsonl:
  {"image": "<seed>.png", "question": ..., "answer": ..., "family": ...}
Families: count, focus, color, color_neg, clock, spatial, app, text,
presence, absent. The absent family asks about UI that does NOT exist
(battery percentage, weather widget, wifi name) with the fixed answer
"not visible on the screen" - this is the refusal training that teaches
the fork to say it can't see something instead of guessing.

ANTI-LEAKAGE (the eval must test generalization, not memorization):
- Seeds: training scenes from --seed-base upward; --eval defaults to
  seed base 10000 and never overlaps.
- Held-out corpus items: the last ~25% of every text corpus (commands,
  URLs, code lines, chat messages, mail subjects, page titles) is used
  ONLY in --eval scenes. Training scenes never contain them.
- Held-out colors: two accent colors (teal, pink) are eval-only; the
  focused-border color question then tests the color CONCEPT, not a
  memorized mapping.
- Held-out phrasings: per question family some templates are eval-only.

Resolutions: 1920x1080 and 1366x768 (the Dell N5010's native panel);
train on both so the fork is not resolution-specialized.

Deterministic: same seed (+same flags) -> same PNG bytes and same QA rows.

License: MIT (code). Generated images/QA are dedicated CC BY 4.0,
"Pawline synthetic desktop dataset" - attribution appreciated so the
provenance of the labels stays traceable. Free-only: uses Pillow only.

Usage:
  synth_desktop.py --out data/train --count 400 --seed-base 0 --res 1920x1080
  synth_desktop.py --out data/train2 --count 300 --seed-base 2000 --res 1366x768
  synth_desktop.py --out data/eval --count 8 --eval --res 1920x1080
"""
import argparse
import json
import os
import random

from PIL import Image, ImageDraw, ImageFont

BAR_H = 28
GAP = 8
BORDER = 3

FONT_DIR = "/usr/share/fonts/truetype/dejavu"


def font(size, bold=False, mono=False):
    name = "DejaVuSansMono-Bold.ttf" if (bold and mono) else \
           "DejaVuSansMono.ttf" if mono else \
           "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"
    return ImageFont.truetype(os.path.join(FONT_DIR, name), size)


# Split helper: deterministic shuffle, then last `holdout` fraction is
# eval-only. Train draws from pool(..., False), eval from the held-out
# slice, so eval content is content the fork never trained on.
def split_corpus(items, holdout=0.25, seed=99):
    items = list(items)
    random.Random(seed).shuffle(items)
    n_hold = max(1, int(len(items) * holdout))
    return items[n_hold:], items[:n_hold]   # (train_pool, eval_pool)


ACCENTS_ALL = [
    ("blue", (66, 133, 244)),
    ("red", (234, 67, 53)),
    ("green", (52, 168, 83)),
    ("yellow", (251, 188, 5)),
    ("purple", (149, 117, 205)),
    ("orange", (255, 112, 67)),
    ("teal", (0, 150, 136)),      # eval-only
    ("pink", (236, 64, 122)),     # eval-only
]
ACCENTS_TRAIN = ACCENTS_ALL[:6]
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
    "journalctl -b -p err", "make -j8", "pacman -Qs tesseract",
    "free -h", "cat /proc/cpuinfo", "git log --oneline -5", "lsblk",
]
TERMINAL_OUTS = [
    "total 128", "nothing to commit, working tree clean", "Compiling pawline-core v0.1.0",
    "Packages (12) linux-6.17  firefox-133", "Tasks: 142 total, 1 running",
    "64 bytes from 1.1.1.1: time=12.3 ms", "Filesystem  Size  Used Avail",
    "Finished release profile in 42s", "No errors found", "drwxr-xr-x 5 user user",
    "Mem: 5.7Gi total, 1.2Gi used", "MODEL NAME: Intel i3 M370",
    "5 files changed, 45 insertions", "sda 232G, sdb 465G",
]
URLS = [
    "https://github.com/linuxkiller9000/pawline", "https://en.wikipedia.org/wiki/Wayland",
    "https://huggingface.co/HuggingFaceTB/SmolVLM-256M-Instruct", "https://doc.rust-lang.org/book/",
    "https://news.ycombinator.com", "https://github.com/YaLTeR/niri",
    "https://crates.io/crates/serde", "https://archlinux.org/packages/",
]
PAGE_TITLES = [
    "Wayland (protocol) - Wikipedia", "The Rust Programming Language",
    "SmolVLM-256M-Instruct - Hugging Face", "niri: A scrollable-tiling Wayland compositor",
    "Hacker News", "pawline: local-first assistant", "serde - crates.io",
    "Arch Linux package search",
]
PAGE_LINES = [
    "Wayland is a communication protocol that specifies", "Welcome! This book will teach you about",
    "SmolVLM is a compact multimodal model for", "niri arranges windows in columns on an",
    "1. Show HN: A tiling compositor", "A local-first assistant for Linux",
    "the display server and its clients.", "the Rust programming language.",
    "Serde is a framework for serializing", "Search the official package repositories",
]
CODE_LINES = [
    "fn main() {", "    let state = AppState::new();", "    loop {",
    "        state.poll_events();", "    }", "}", "use pawline_core::router;",
    "struct Window {", "    title: String,", "    focused: bool,", "}",
    "impl Router {", "    fn route(&self, msg: &str) {", "}", "#[cfg(test)]",
    "mod tests {", "    assert_eq!(2 + 2, 4);", "const MAX: usize = 512;",
]
FILES_LISTS = [
    ["Documents", "Downloads", "Pictures", "Projects", "notes.txt"],
    ["report.pdf", "budget.ods", "screenshot.png", "archive.tar.zst", "todo.md"],
    ["pawline", "niri", "dotfiles", "music", "backups"],
    ["thesis.docx", "slides.odp", "data.csv", "run.sh", "readme.md"],
]
CHAT_MSGS = [
    ("mama", "call me when you wake up"), ("kareem", "match at 8 tonight?"),
    ("sara", "did you push the branch?"), ("ahmed", "the build works!"),
    ("nour", "send me the notes please"), ("omar", "niri update broke my config"),
    ("laila", "dinner is at 7"), ("hassan", "check your email"),
]
MAIL_SUBJECTS = [
    "Your receipt from the bookstore", "Team standup moved to 10:30",
    "Weekend trip photos", "Invoice #4021 attached",
    "Reminder: dentist on Tuesday", "New comment on your issue",
    "Lab results are ready", "Meeting notes from today",
]
MAIL_SENDERS = ["bookstore@shop.example", "asmaa@work.example", "karim@photos.example",
                "billing@isp.example", "clinic@health.example", "github@notifications.example"]
APP_NAMES = {
    "terminal": "Terminal", "browser": "Firefox", "editor": "Neovim",
    "files": "Nautilus", "chat": "Signal", "mail": "Thunderbird",
}

# Question phrasings: (train templates, eval-only extra templates).
Q_TEMPLATES = {
    "count": (["How many windows are open on the desktop?", "How many windows are visible?"],
              ["Count the open windows.", "How many app windows can you see?"]),
    "focus": (["Which window is focused?", "What is the title of the focused window?"],
              ["Which window is currently active?", "Name the active window."]),
    "color": (["What color is the border of the focused window?"],
              ["What colour is the active window border?", "Tell me the focused border color."]),
    "clock": (["What time is shown in the top bar?", "What does the clock in the panel say?"],
              ["What time is it on the screen?", "Read the clock in the status bar."]),
    "spatial": (["What is the title of the leftmost window?"],
                ["Which window is on the far left?", "Name the leftmost window's title."]),
    "app": (["Which application is in the rightmost window?"],
            ["What app is open in the far right window?", "Name the program in the rightmost window."]),
    "indicator": (["What color is the network indicator in the top bar?",
                   "What colour is the status dot in the panel?"],
                  ["Which colour is the little dot next to the tray icons?",
                   "Tell me the color of the status light in the top bar."]),
    "progress": (["What color is the progress bar in the {app} window?",
                  "Which colour is the progress bar in the {app} window?"],
                 ["What color is the fill of the {app} progress bar?",
                  "Name the colour of the bar filling up in the {app} window."]),
}


def pick_template(family, rng, eval_mode):
    train, eval_only = Q_TEMPLATES[family]
    pool = train + (eval_only if eval_mode else [])
    return rng.choice(pool)


def draw_text_fit(draw, xy, text, fnt, fill, max_w):
    while text and draw.textlength(text, font=fnt) > max_w:
        text = text[:-1]
    if text:
        draw.text(xy, text, font=fnt, fill=fill)


def make_scene(rng, eval_mode, W, H):
    accents = ACCENTS_ALL if eval_mode else ACCENTS_TRAIN
    focus_accent = rng.choice(accents)
    # Purely visual probes (2026-11-27): a status dot in the top bar and a
    # progress bar in one window. Neither is answerable from window metadata
    # and neither is text, so together they measure what a vision model is
    # actually FOR - the rest of the battery is state and text questions.
    indicator = rng.choice(accents)
    progress_color = rng.choice([a for a in accents if a[0] != indicator[0]])
    progress_pct = rng.choice([15, 25, 40, 55, 70, 85])
    bg = rng.choice(BGS)
    clock = f"{rng.randint(0, 23):02d}:{rng.randint(0, 59):02d}"
    n_workspaces = rng.randint(2, 6)
    active_ws = rng.randint(1, n_workspaces)

    def pool(items, seed):
        tr, ev = split_corpus(items, seed=seed)
        return ev if eval_mode else tr

    cmds_pool = pool(TERMINAL_CMDS, 11)
    outs_pool = pool(TERMINAL_OUTS, 12)
    urls_pool = pool(URLS, 13)
    titles_pool = pool(PAGE_TITLES, 14)
    plines_pool = pool(PAGE_LINES, 15)
    code_pool = pool(CODE_LINES, 16)
    files_pool = pool(FILES_LISTS, 17)
    chat_pool = pool(CHAT_MSGS, 18)
    subj_pool = pool(MAIL_SUBJECTS, 19)
    send_pool = pool(MAIL_SENDERS, 20)

    n_cols = rng.choice([1, 2, 2, 3, 3, 4])
    avail_w = W - GAP * (n_cols + 1)
    col_w = avail_w // n_cols
    avail_h = H - BAR_H - GAP * 2
    apps = ["terminal", "browser", "editor", "files", "chat", "mail"]
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
                "app": app, "app_name": APP_NAMES[app], "title": "",
                "x": x, "y": y, "w": col_w, "h": h,
                "focused": False, "dark": app in ("terminal", "editor"),
                "content": {},
            })
    focused_idx = rng.randrange(len(windows))
    windows[focused_idx]["focused"] = True

    for w in windows:
        app = w["app"]
        if app == "terminal":
            cmds = rng.sample(cmds_pool, min(3, len(cmds_pool)))
            outs = rng.sample(outs_pool, min(2, len(outs_pool)))
            w["content"] = {"cmds": cmds, "outs": outs, "last_cmd": cmds[-1]}
            w["title"] = "user@pawline: ~"
        elif app == "browser":
            i = rng.randrange(len(urls_pool))
            t = rng.choice(titles_pool)
            lines = rng.sample(plines_pool, min(4, len(plines_pool)))
            w["content"] = {"url": urls_pool[i], "page_title": t, "lines": lines}
            w["title"] = f"{t} - Firefox"
        elif app == "editor":
            lines = rng.sample(code_pool, min(8, len(code_pool)))
            fname = rng.choice(["main.rs", "router.rs", "laya.rs", "bridge.rs"])
            w["content"] = {"lines": lines, "file": fname, "first_line": lines[0]}
            w["title"] = f"{fname} - Neovim"
        elif app == "files":
            items = rng.choice(files_pool)
            w["content"] = {"items": items, "first_item": items[0], "count": len(items)}
            w["title"] = "Home - Nautilus"
        elif app == "chat":
            msgs = rng.sample(chat_pool, min(3, len(chat_pool)))
            w["content"] = {"msgs": msgs, "last_sender": msgs[-1][0], "last_msg": msgs[-1][1]}
            w["title"] = "Signal"
        elif app == "mail":
            subs = rng.sample(subj_pool, min(4, len(subj_pool)))
            sends = rng.sample(send_pool, min(4, len(send_pool)))
            w["content"] = {"subs": subs, "senders": sends, "first_sub": subs[0]}
            w["title"] = "Inbox - Thunderbird"
    progress_win = rng.randrange(len(windows))
    return {
        "windows": windows, "clock": clock, "bg": bg,
        "indicator": indicator, "indicator_name": indicator[0],
        "progress_color": progress_color, "progress_name": progress_color[0],
        "progress_pct": progress_pct, "progress_win": progress_win,
        "focus_accent": focus_accent, "focus_accent_name": focus_accent[0],
        "n_workspaces": n_workspaces, "active_ws": active_ws,
    }


def render(scene, W, H):
    img = Image.new("RGB", (W, H), scene["bg"])
    d = ImageDraw.Draw(img)
    f_bar = font(16, bold=True)
    f_title = font(14, bold=True)
    f_body = font(15)
    f_mono = font(15, mono=True)

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
    d.ellipse([W - 172, 9, W - 162, 19], fill=scene["indicator"][1])

    for w in scene["windows"]:
        x0, y0, ww, hh = w["x"], w["y"], w["w"], w["h"]
        bcol = scene["focus_accent"][1] if w["focused"] else UNFOCUSED_BORDER
        d.rectangle([x0 - BORDER, y0 - BORDER, x0 + ww + BORDER, y0 + hh + BORDER], fill=bcol)
        body_bg = WIN_BG_DARK if w["dark"] else WIN_BG
        d.rectangle([x0, y0, x0 + ww, y0 + 30], fill=(44, 46, 54))
        d.text((x0 + 10, y0 + 6), w["title"], font=f_title, fill=TITLE_FG)
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
        elif w["app"] == "mail":
            for subj, snd in zip(c["subs"], c["senders"]):
                draw_text_fit(d, (tx, ty), subj, f_body, (20, 20, 120), ww - 24)
                draw_text_fit(d, (tx, ty + 18), snd, font(12), (100, 100, 110), ww - 24)
                ty += 46
        if w is scene["windows"][scene["progress_win"]]:
            # drawn last so nothing overlaps it; the label is text, the fill is
            # colour only, which is the point of the question
            by = y0 + hh - 28
            pw = ww - 24
            d.rectangle([tx, by, tx + pw, by + 12], fill=(70, 70, 80))
            d.rectangle([tx, by, tx + max(2, int(pw * scene["progress_pct"] / 100)), by + 12],
                        fill=scene["progress_color"][1])
    return img


ABSENT_QUESTIONS = [
    ("What battery percentage is shown in the top bar?", "not visible on the screen"),
    ("What does the weather widget say?", "not visible on the screen"),
    ("What is the name of the wifi network shown?", "not visible on the screen"),
    ("What song is playing in the music player?", "not visible on the screen"),
    ("How many unread notifications are shown?", "not visible on the screen"),
]


def gen_qa(scene, rng, eval_mode):
    wins = scene["windows"]
    qa = []

    def add(family, q, a):
        qa.append({"family": family, "question": q, "answer": str(a)})

    add("count", pick_template("count", rng, eval_mode), len(wins))
    fw = next(w for w in wins if w["focused"])
    add("focus", pick_template("focus", rng, eval_mode), fw["title"])
    add("color", pick_template("color", rng, eval_mode), scene["focus_accent_name"])

    # Negative color probe: hallucination check on the same pixels.
    wrong = rng.choice([a[0] for a in ACCENTS_ALL if a[0] != scene["focus_accent_name"]])
    add("color_neg", f"Is the focused window's border {wrong}?", "no")

    add("clock", pick_template("clock", rng, eval_mode), scene["clock"])

    order = sorted(wins, key=lambda w: w["x"])
    add("spatial", pick_template("spatial", rng, eval_mode), order[0]["title"])
    add("app", pick_template("app", rng, eval_mode), order[-1]["app_name"])

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
        elif app == "mail":
            add("text", rng.choice([
                "What is the first email subject in the inbox?",
                "Read the top email subject in the mail window.",
            ]), c["first_sub"])

    present = {w["app"] for w in wins}
    absent_apps = [a for a in APP_NAMES if a not in present]
    pick_yes = rng.choice(sorted(present))
    add("presence", f"Is there a {APP_NAMES[pick_yes]} window open?", "yes")
    if absent_apps:
        pick_no = rng.choice(absent_apps)
        add("presence", f"Is there a {APP_NAMES[pick_no]} window open?", "no")

    # Visual-only probes: no metadata, no OCR text - colour in pixels.
    add("indicator", pick_template("indicator", rng, eval_mode), scene["indicator_name"])
    pwin = wins[scene["progress_win"]]
    add("progress", pick_template("progress", rng, eval_mode).format(app=pwin["app_name"]),
        scene["progress_name"])

    # Refusal training: ask about UI that does not exist anywhere.
    q, a = rng.choice(ABSENT_QUESTIONS)
    add("absent", q, a)
    return qa


def parse_res(s):
    w, h = s.lower().split("x")
    return int(w), int(h)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--count", type=int, default=200)
    ap.add_argument("--seed-base", type=int, default=None)
    ap.add_argument("--res", default="1920x1080", choices=["1920x1080", "1366x768"])
    ap.add_argument("--eval", action="store_true",
                    help="use held-out corpora/colors/phrasings; default seed base 10000")
    args = ap.parse_args()
    W, H = parse_res(args.res)
    seed_base = args.seed_base if args.seed_base is not None else (10000 if args.eval else 0)
    os.makedirs(args.out, exist_ok=True)
    qa_path = os.path.join(args.out, "qa.jsonl")
    with open(qa_path, "w") as f:
        for i in range(args.count):
            seed = seed_base + i
            rng = random.Random(seed)
            scene = make_scene(rng, args.eval, W, H)
            img = render(scene, W, H)
            name = f"{seed:05d}.png"
            img.save(os.path.join(args.out, name))
            for row in gen_qa(scene, random.Random(seed + 500000), args.eval):
                row["image"] = name
                f.write(json.dumps(row) + "\n")
    print(f"wrote {args.count} scenes ({W}x{H}, eval={args.eval}) to {args.out}")


if __name__ == "__main__":
    main()
