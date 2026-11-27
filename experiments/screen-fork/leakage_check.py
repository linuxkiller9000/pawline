#!/usr/bin/env python3
"""Leakage check: can the eval set have been seen during training?

A training loss of 15 -> 0.23 in one epoch on 3000 rows is either a narrow
answer distribution or a leak. This settles which, without training anything:

  1. image identity   - any eval PNG byte-identical to a training PNG?
  2. question text    - any eval question verbatim in the training questions?
  3. answer overlap   - per family, how many eval answers were also training
                        answers (structural answers repeat by design; text
                        answers must not)
  4. corpus holdout   - the eval-only quarter of every text corpus must be
                        disjoint from the training quarter, and must not appear
                        in any training text answer
  5. colour holdout   - teal and pink are eval-only accents; they must not
                        appear in any training colour answer
  6. phrasing holdout - the eval-only question templates must not appear in the
                        training questions

Usage:
  python3 leakage_check.py --train /tmp/synth/train_check/t1080 --train2 /tmp/synth/train_check/t768 --eval /tmp/synth/eval3

License: MIT.
"""
import argparse
import collections
import hashlib
import json
import os
import sys

import synth_desktop as sd

CORPORA = [
    ("terminal cmds", sd.TERMINAL_CMDS, 11),
    ("terminal outs", sd.TERMINAL_OUTS, 12),
    ("urls", sd.URLS, 13),
    ("page titles", sd.PAGE_TITLES, 14),
    ("page lines", sd.PAGE_LINES, 15),
    ("code lines", sd.CODE_LINES, 16),
    # exact corpus objects the generator splits - flattening them first would
    # produce a different split and a false verdict
    ("file lists", sd.FILES_LISTS, 17),
    ("chat msgs", sd.CHAT_MSGS, 18),
    ("mail subjects", sd.MAIL_SUBJECTS, 19),
    ("mail senders", sd.MAIL_SENDERS, 20),
]


def flat(item):
    """Corpus items are strings, (sender, msg) pairs or lists of filenames."""
    if isinstance(item, str):
        return [item]
    if isinstance(item, (tuple, list)):
        return [x for x in item if isinstance(x, str)]
    return []


def norm(s):
    return "".join(c for c in s.lower() if c.isalnum())


def load(dirs):
    rows = []
    for d in dirs:
        p = os.path.join(d, "qa.jsonl")
        if not os.path.exists(p):
            sys.exit(f"missing {p}")
        for line in open(p):
            r = json.loads(line)
            r["_dir"] = d
            rows.append(r)
    return rows


def hashes(dirs):
    out = {}
    for d in dirs:
        for name in os.listdir(d):
            if not name.endswith(".png"):
                continue
            h = hashlib.sha256(open(os.path.join(d, name), "rb").read()).hexdigest()
            out.setdefault(h, []).append(os.path.join(d, name))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--train", action="append", required=True)
    ap.add_argument("--eval", action="append", required=True)
    args = ap.parse_args()
    train, ev = load(args.train), load(args.eval)
    bad = []

    # 1. image identity
    th, eh = hashes(args.train), hashes(args.eval)
    shared = set(th) & set(eh)
    print(f"1. image bytes      : {len(th)} train, {len(eh)} eval, "
          f"{len(shared)} identical  {'LEAK' if shared else 'ok'}")
    if shared:
        bad.append(f"{len(shared)} byte-identical images")

    # 2. question text
    tq = {norm(r["question"]) for r in train}
    eq = {norm(r["question"]) for r in ev}
    qshared = tq & eq
    # Question TEMPLATES are shared on purpose: only the eval-only phrasings are
    # held out (checked in 6). A shared template string with a different answer
    # is the intended generalisation test, not a leak.
    print(f"2. question text    : {len(qshared)} eval questions share a template "
          f"string with training (expected; see 6 for the held-out phrasings)")

    # 3. answer overlap per family (structural answers repeat by design)
    ta = collections.defaultdict(set)
    for r in train:
        ta[r["family"]].add(norm(r["answer"]))
    per_fam = collections.defaultdict(lambda: [0, 0])
    for r in ev:
        f = r["family"]
        per_fam[f][1] += 1
        if norm(r["answer"]) in ta[f]:
            per_fam[f][0] += 1
    print("3. answer overlap   : eval answers also present in training answers, per family")
    for f in sorted(per_fam):
        a, b = per_fam[f]
        note = ""
        if f == "text" and a:
            note = "  <- text answers must not overlap"
            bad.append(f"{a} text answers seen in training")
        print(f"     {f:10s} {a}/{b}{note}")

    # 4. corpus holdout
    print("4. corpus holdout   : eval-only quarter vs training quarter")
    for name, corpus, seed in CORPORA:
        tr, evq = sd.split_corpus(corpus, seed=seed)
        tr_s = {norm(x) for item in tr for x in flat(item)}
        ev_s = {norm(x) for item in evq for x in flat(item)}
        inter = tr_s & ev_s
        in_train = sum(1 for r in train if r["family"] == "text"
                       and any(x and x in norm(r["answer"]) for x in ev_s))
        flag = "LEAK" if (inter or in_train) else "ok"
        print(f"     {name:15s} train {len(tr):3d} / eval {len(evq):3d}, "
              f"shared {len(inter)}, eval items in training answers {in_train}  {flag}")
        if inter or in_train:
            bad.append(f"{name}: holdout overlap")

    # 5. colour holdout
    eval_colours = [a[0] for a in sd.ACCENTS_ALL if a not in sd.ACCENTS_TRAIN]
    leaked = [r for r in train if r["family"] in ("color", "indicator", "progress")
              and norm(r["answer"]) in {norm(c) for c in eval_colours}]
    print(f"5. colour holdout   : eval-only colours {eval_colours} in training "
          f"answers: {len(leaked)}  {'LEAK' if leaked else 'ok'}")
    if leaked:
        bad.append(f"{len(leaked)} eval-only colours in training")

    # 6. phrasing holdout
    leak_q = []
    for fam, (train_t, eval_only) in sd.Q_TEMPLATES.items():
        for tmpl in eval_only:
            key = norm(tmpl.split("{")[0])
            if any(key and key in norm(r["question"]) for r in train):
                leak_q.append((fam, tmpl))
    print(f"6. phrasing holdout : eval-only templates found in training questions: "
          f"{len(leak_q)}  {'LEAK' if leak_q else 'ok'}")
    if leak_q:
        bad.append(f"{len(leak_q)} eval-only phrasings in training")

    print()
    print("VERDICT:", "CLEAN - eval set is not in the training data" if not bad
          else "LEAK: " + "; ".join(bad))
    print("Note: count/focus/spatial/app/presence answers repeat across scenes by "
          "design (a handful of window titles and app names), so high overlap "
          "there is compositionality, not leakage. Text answers and the held-out "
          "corpora/colours/phrasings are what must be disjoint.")


if __name__ == "__main__":
    main()
