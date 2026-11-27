#!/usr/bin/env bash
# Evaluate a fork GGUF on the strict battery and print the ship/no-ship numbers.
#
#   ./fork_eval.sh /path/fork.gguf /path/mmproj-fork.gguf
#
# Runs, in order:
#   1. eval2  - 86 state/text/refusal questions (the set every other number in
#               EXPERIMENTS 4f/4g was measured on)
#   2. eval3  - the two genuinely visual families, 16 questions
#   3. the same two at the training scale, since the light config trained on
#      448px images and vision tokens change with the input size
#
# Nothing here decides anything on its own. The gate is in EXPERIMENTS 4g: the
# fork ships only if it beats the router's 84/86 on state/text AND moves the
# visual families, where every no-training path sits near the 2/16 chance line.
set -euo pipefail
FORK=${1:?usage: fork_eval.sh FORK.gguf MMPROJ.gguf [eval2dir] [eval3dir]}
MMPROJ=${2:?usage: fork_eval.sh FORK.gguf MMPROJ.gguf [eval2dir] [eval3dir]}
E2=${3:-/tmp/synth/eval2}
E3=${4:-/tmp/synth/eval3}
SCALE=${SCALE:-0.5}
TRAIN_SCALE=${TRAIN_SCALE:-0.23}   # 448px / 1920px, the light-config training size
cd "$(dirname "$0")"

echo "== 1. eval2, 86 state/text/refusal questions, scale $SCALE =="
python3 eval_battery.py --dir "$E2" --method fork --model "$FORK" --mmproj "$MMPROJ" \
    --scale "$SCALE" --results "$E2/results_fork.json"
python3 eval_battery.py --dir "$E2" --report --results "$E2/results_fork.json"

echo
echo "== 2. eval3, visual families (indicator, progress), scale $SCALE =="
python3 eval_battery.py --dir "$E3" --method fork --model "$FORK" --mmproj "$MMPROJ" \
    --scale "$SCALE" --families indicator,progress --results "$E3/results_fork_visual.json"
python3 eval_battery.py --dir "$E3" --report --results "$E3/results_fork_visual.json"

echo
echo "== 3. same visual families at the training scale ($TRAIN_SCALE) =="
python3 eval_battery.py --dir "$E3" --method fork --model "$FORK" --mmproj "$MMPROJ" \
    --scale "$TRAIN_SCALE" --families indicator,progress --results "$E3/results_fork_visual_train_scale.json"
python3 eval_battery.py --dir "$E3" --report --results "$E3/results_fork_visual_train_scale.json"

echo
echo "Reference points (EXPERIMENTS 4g, same scenes, sandbox CPU):"
echo "  router, no model      84/86 state+text, 0/16 visual (refuses), 0.27 s/q, 81 MiB"
echo "  tuned stock 256M      36/86 state+text, 6/16 visual, 16.2 s/q, 539 MiB"
echo "  stock 256M            13/86 state+text, 5/16 visual, 9.7 s/q"
echo "  OCR hybrid            29/86 state+text, 5/16 visual, 1.4 s/q"
echo "  visual chance line    2/16"
