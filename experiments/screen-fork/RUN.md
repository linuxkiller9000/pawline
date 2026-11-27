# Running the SmolVLM screen fork (one page)

Goal: a screen-tuned SmolVLM that answers VISUAL questions about your
niri desktop better than stock SmolVLM and the OCR hybrid. Training
needs a free GPU (Colab T4 / Kaggle P100); everything else runs on your
machine. Nothing here costs money.

## 1. Train (Colab, ~1-2 h on a free T4)

1. Open https://colab.research.google.com and upload
   `experiments/screen-fork/finetune_colab.ipynb` from this repo.
2. Runtime > Change runtime type > T4 GPU.
3. Run all cells top to bottom. It will:
   - pull ScreenQA (CC BY 4.0) and take 6000 rows,
   - fetch `synth_desktop.py` from this repo and render 700 synthetic
     desktop scenes (~6300 QA rows, exact labels, seeds 0+),
   - mix them 1:1 and LoRA-tune SmolVLM-256M-Instruct,
   - merge and save `./smolvlm-screen-merged`.
4. Download the merged folder (Files panel, or
   `from google.colab import files; files.download(...)` after zipping).

The notebook renders 450 scenes at 1920x1080 and 250 at 1366x768 (the
N5010's native panel), mixed 1:1 with ScreenQA.

## 2. Convert to GGUF (your machine, CPU, ~5 min)

    git clone https://github.com/ggml-org/llama.cpp; cd llama.cpp
    pip install -r requirements.txt
    python convert_hf_to_gguf.py /path/to/smolvlm-screen-merged --outfile fork.gguf --outtype q8_0
    # reuse the stock projector: mmproj-SmolVLM-256M-Instruct-Q8_0.gguf

## 3. Evaluate (your machine)

The battery compares stock 256M, stock 500M, the OCR hybrid and the fork
on the SAME held-out synthetic scenes (seeds 10000+, never trained on):

    cd experiments/screen-fork
    python3 synth_desktop.py --out data/eval --count 8 --eval   # held-out labels, seeds 10000+
    python3 eval_battery.py --dir data/eval --method ocr        # needs tesseract
    python3 eval_battery.py --dir data/eval --method smol256
    python3 eval_battery.py --dir data/eval --method smol500
    python3 eval_battery.py --dir data/eval --method fork --model /path/to/fork.gguf \
        --mmproj /path/to/mmproj-SmolVLM-256M-Instruct-Q8_0.gguf
    python3 eval_battery.py --dir data/eval --report            # table per question family

The eval set uses held-out content, colors and question phrasings the
training run never saw, so a memorizing fork scores no better than a
generalizing one deserves. The battery reports accuracy per question
family plus average seconds and peak RAM per method - on the Dell N5010
(i3 M370, 6 GB) those two rows decide whether the fork is usable at all.

Set `PAWLINE_VLM_DIR` if your llama.cpp build and GGUFs are not in
/downloads/vlm. On the Dell, point it at the real llama.cpp build.

## 4. Adopt only if it wins

The fork ships nowhere by default. It replaces the screen-vision model
only if it beats BOTH stock SmolVLM and the OCR hybrid on the battery
(see RESEARCH.md and docs/EXPERIMENTS.md 4d/4e/4f). If it wins, put
`fork.gguf` + the mmproj in the Pawline models dir and select it with
the screen-model setting ("use screen model fork").

## Licenses (keep in any model card)

SmolVLM: Apache-2.0. ScreenQA annotations: CC BY 4.0 (attribute Google);
Rico screenshots behind it: Rico research terms
(interactionmining.org/archive/rico/copyright.txt). Synthetic desktop
set: CC BY 4.0 "Pawline synthetic desktop dataset". WebSight if added:
CC-BY-4.0 + disclose its use when releasing a model.

## Colab gotchas (measured on a free T4, 2026-11-27)

Three fixes the notebook needed that the sandbox did not surface:

- `apt-get install -y fonts-dejavu-core` before `synth_desktop.py`. Colab ships
  no DejaVu and the generator renders every scene with it (first cell of the
  data section does this).
- transformers 5.x removed `AutoModelForVision2Seq`. Use
  `AutoModelForImageTextToText` - same class, new name.
- The free T4 has no bf16. Load the base in fp16 (`dtype=torch.float16`) and
  set `fp16=True` in `SFTConfig`. bf16 errors or falls back to fp32 and OOMs.

## Light Colab config (after the free session stalled)

The first free-T4 run stalled at step 3 of 375 and disconnected. The notebook
now runs a light config: 448px images (`_small()` in the data cell), ~3000 rows
(1500 ScreenQA + 1500 synthetic), `dataloader_num_workers=2`, checkpoints every
100 steps with `save_total_limit=2`. Two crashes fixed along the way:

- `datasets.Image()` cast on both sides before `concatenate_datasets`; a
  mismatched image feature type breaks the concat.
- the fp16 GradScaler crash: trainable LoRA params are cast to fp32 after the
  trainer is built, while the frozen base stays fp16.

## Evaluating a fork GGUF (one command)

`./fork_eval.sh /path/fork.gguf /path/mmproj-fork.gguf` runs the three
measurements that decide the gate: eval2 (86 state/text/refusal questions),
eval3's two visual families, and those families again at the training scale
(the light config trained on 448px images, so SCALE=0.23 matches training while
the default 0.5 matches how the app feeds stock models). It prints the
reference points next to the results.

Before trusting any of it, run leakage_check.py against whatever training set
the run actually used:

  python3 leakage_check.py --train <train dir 1> --train <train dir 2> --eval /tmp/synth/eval3
