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

## 2. Convert to GGUF (your machine, CPU, ~5 min)

    git clone https://github.com/ggml-org/llama.cpp; cd llama.cpp
    pip install -r requirements.txt
    python convert_hf_to_gguf.py /path/to/smolvlm-screen-merged --outfile fork.gguf --outtype q8_0
    # reuse the stock projector: mmproj-SmolVLM-256M-Instruct-Q8_0.gguf

## 3. Evaluate (your machine)

The battery compares stock 256M, stock 500M, the OCR hybrid and the fork
on the SAME held-out synthetic scenes (seeds 10000+, never trained on):

    cd experiments/screen-fork
    python3 synth_desktop.py --out data/eval --count 8 --seed-base 10000
    python3 eval_battery.py --dir data/eval --method ocr        # needs tesseract
    python3 eval_battery.py --dir data/eval --method smol256
    python3 eval_battery.py --dir data/eval --method smol500
    python3 eval_battery.py --dir data/eval --method fork --model /path/to/fork.gguf \
        --mmproj /path/to/mmproj-SmolVLM-256M-Instruct-Q8_0.gguf
    python3 eval_battery.py --dir data/eval --report            # table per question family

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
