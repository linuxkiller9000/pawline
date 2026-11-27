#!/bin/sh
# laya-setup.sh - one-time installer for the EXPERIMENTAL Laya intent router model.
#
# Downloads vrajnotviraj/laya-intent-router-150m-onnx (Apache-2.0) from Hugging Face and
# converts its int8 weights to plain fp32 so Pawline's pure-Rust runtime (tract) can read
# them. Everything stays on your machine; the only network use is this download
# (~440 MB once, from huggingface.co). Needs: python3, ~2 GB free disk, ~2 GB RAM.
#
# Usage: sh tools/laya-setup.sh
# Result: <pawline data>/laya/{laya_fp32.onnx, laya_fp32.onnx.data, tokenizer.json}
# Then inside Pawline: laya on
set -eu

REPO="vrajnotviraj/laya-intent-router-150m-onnx"
DATA="${PAWLINE_DATA:-${XDG_DATA_HOME:-$HOME/.local/share}/pawline}"
OUT="$DATA/laya"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "Laya setup (experimental). Model: $REPO (Apache-2.0)."
echo "Installing into: $OUT"
mkdir -p "$OUT"

python3 -m venv "$WORK/venv"
"$WORK/venv/bin/pip" install -q --disable-pip-version-check onnx numpy huggingface_hub

HF_WORK="$WORK" "$WORK/venv/bin/python" - "$REPO" "$OUT" <<'PYEOF'
import os, sys
import numpy as np
import onnx
from huggingface_hub import hf_hub_download

repo, out = sys.argv[1], sys.argv[2]
work = os.environ["HF_WORK"]

src = hf_hub_download(repo, "laya.onnx", local_dir=work)
tok = hf_hub_download(repo, "tokenizer.json")

m = onnx.load(src, load_external_data=True)
g = m.graph
inits = {i.name: i for i in g.initializer}
drop, add = set(), []
converted = 0
for node in g.node:
    if node.op_type != "MatMulNBits":
        continue
    bits = next((a.i for a in node.attribute if a.name == "bits"), None)
    block = next((a.i for a in node.attribute if a.name == "block_size"), None)
    assert bits == 8 and block == 32, f"unsupported quantization bits={bits} block={block}"
    assert len(node.input) == 3, f"unexpected MatMulNBits inputs: {list(node.input)}"
    B = onnx.numpy_helper.to_array(inits[node.input[1]])      # uint8 packed weights [N, K]
    S = onnx.numpy_helper.to_array(inits[node.input[2]])      # fp scales [N, K/32]
    n = B.shape[0]
    k = S.shape[1] * 32
    W = (B.reshape(n, S.shape[1], 32).astype(np.float32) - 128.0) * S[..., None]
    W = W.reshape(n, k).T.copy()                              # [K, N] for a plain MatMul
    t = onnx.numpy_helper.from_array(W, name=node.input[1] + "_fp32")
    add.append(t)
    drop.update([node.input[1], node.input[2]])
    node.op_type = "MatMul"
    node.domain = ""
    del node.attribute[:]
    while len(node.input) > 1:
        node.input.pop()
    node.input.append(t.name)
    converted += 1
assert converted > 0, "no MatMulNBits nodes found - model layout changed?"
keep = [i for i in g.initializer if i.name not in drop]
del g.initializer[:]
g.initializer.extend(keep + add)

dst = os.path.join(out, "laya_fp32.onnx")
onnx.save_model(m, dst, save_as_external_data=True, all_tensors_to_one_file=True,
                location="laya_fp32.onnx.data", size_threshold=1024)
onnx.checker.check_model(dst)

import shutil
shutil.copy(tok, os.path.join(out, "tokenizer.json"))
print(f"converted {converted} quantized layers; wrote {dst} (+ .data, tokenizer.json)")
PYEOF

echo "Done. Start Pawline and say: laya on"
