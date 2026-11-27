#!/bin/sh
# Pawline Needle bench for the Dell - one command, honest labels.
# Usage:   sh tools/dell_bench.sh
# Output:  dell-bench-results.txt in the repo root - send that file back.
# Runs one config at a time and caps generation at 64 tokens, so it fits
# the Dell's 6 GB and stays interruptible. Expect a few minutes total.
set -e
cd "$(dirname "$0")/.."
OUT=dell-bench-results.txt

{
echo "Pawline needle bench - $(date -u '+%Y-%m-%d %H:%M UTC')"
echo "== machine =="
uname -a
grep -m1 "model name" /proc/cpuinfo
printf 'SIMD flags: '
grep -m1 flags /proc/cpuinfo | tr ' ' '\n' | grep -E '^(sse4_2|avx|avx2|fma|f16c)$' | tr '\n' ' '
echo
free -h | sed -n 1,2p
} > "$OUT"

echo "== build ==" | tee -a "$OUT"
cargo build --release -p pawline-needle --example dump_tools \
    -p pawline-needle-native --example bench --example simd_check 2>&1 | tail -2 | tee -a "$OUT"
./target/release/examples/dump_tools | tee -a "$OUT"

{
echo "== dispatch =="
./target/release/examples/simd_check
echo "== bench (64 new tokens cap, one config at a time) =="
} >> "$OUT" 2>&1

for cfg in "d20 f32" "d20 int8" "d12"; do
    echo "-- $cfg --" >> "$OUT"
    BENCH_MAX_NEW=64 ./target/release/examples/bench "$cfg" >> "$OUT" 2>&1
done

echo "Done. Results in $OUT - send that file back."
