# laya-route-bench

Routing-quality harness for the Laya intent router. It loads the real converted
ONNX model (via pawline-native's in-app converter output) and the real
tokenizer, then runs a 22-message battery (18 positive intents, 4 adversarial
none-cases) through a faithful port of pawline-native/src/laya.rs `route_inner`.

Not part of the workspace: heavy deps (tract, tokenizers) stay out of shipping
crates. Build and run standalone:

    cargo build --release
    ./target/release/laya-route-bench /path/to/laya_fp32.onnx /path/to/tokenizer.json

Sandbox results (2026-11-27, 2 GB RAM VM): 21/22 pass; the one miss is the
adversarial "my folder of memories from school" which fired make_folder at
p=0.950 (above the 0.80 threshold). Known limitation: the harness exercises
only the common fits-in-window path; the over-budget per-phrasing truncation
branch is not covered.
