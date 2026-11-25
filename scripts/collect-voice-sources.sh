#!/bin/sh
# Collect the corresponding source for the voice libraries into ./voice-sources/ (release-side assets).
# Draft for CI. Every download is checked against a pinned sha256 or a pinned git commit.
# Usage: scripts/collect-voice-sources.sh [gcc|moonshine|ort|libcxx|all]   (default: all)
set -eu
OUT=${OUT:-voice-sources}
mkdir -p "$OUT"
what=${1:-all}

fetch() { # url sha256 file
  [ -f "$OUT/$3" ] || curl -fsSL -o "$OUT/$3" "$1"
  echo "$2  $OUT/$3" | sha256sum -c - 
}

gcc() {
  B=http://archive.ubuntu.com/ubuntu/pool/main/g/gcc-12
  fetch $B/gcc-12_12.3.0.orig.tar.gz 62b0fc89b6d41f9df2470d0fb4995f6ff5885f910518a2c6a44a6888ea5a9ea1 gcc-12_12.3.0.orig.tar.gz
  fetch "$B/gcc-12_12.3.0-1ubuntu1~22.04.3.debian.tar.xz" 44074c3d5e7d97365f1f7b45291ade2ee40ed6300176530d912e7cc0ceba77ab gcc-12_12.3.0-1ubuntu1~22.04.3.debian.tar.xz
  fetch "$B/gcc-12_12.3.0-1ubuntu1~22.04.3.dsc" ec040a1fa94186bd53535f7156e086e67863bb55836ae7fd7fef64f1ab66b04d gcc-12_12.3.0-1ubuntu1~22.04.3.dsc
}

moonshine() {
  # tag v0.1.6 -> commit d8d8fa29d05ca67441a7719e70099f9f48917a93 (tag object a0ea0d09...)
  d=$(mktemp -d)
  git -c advice.detachedHead=false clone -q --branch v0.1.6 https://github.com/moonshine-ai/moonshine.git "$d/m"
  [ "$(git -C "$d/m" rev-parse HEAD)" = d8d8fa29d05ca67441a7719e70099f9f48917a93 ] || { echo "moonshine commit mismatch"; exit 1; }
  git -C "$d/m" archive --format=tar.gz --prefix=moonshine-v0.1.6/ -o "$PWD/$OUT/moonshine-v0.1.6-d8d8fa2.tar.gz" HEAD
  rm -rf "$d"
}

ort() {
  # Microsoft ONNX Runtime v1.23.2 (the version scripts/ort-build-common.sh in Moonshine v0.1.6 builds).
  # Not reachable from the author's sandbox, so the commit pin below must be filled in on first CI run
  # and then checked in. Submodules are included because the build fetches them.
  d=$(mktemp -d)
  git clone -q --recursive --depth 1 --branch v1.23.2 https://github.com/microsoft/onnxruntime.git "$d/o"
  echo "onnxruntime v1.23.2 commit: $(git -C "$d/o" rev-parse HEAD)"
  [ -z "${ORT_COMMIT:-}" ] || [ "$(git -C "$d/o" rev-parse HEAD)" = "$ORT_COMMIT" ] || { echo "ort commit mismatch"; exit 1; }
  tar --exclude=.git -czf "$OUT/onnxruntime-v1.23.2-with-submodules.tar.gz" -C "$d" o
  rm -rf "$d"
}

libcxx() {
  # Ubuntu llvm-toolchain-14 1:14.0.0-1ubuntu1 (source of the static libc++/libc++abi/libunwind).
  # Needs the Launchpad source files; filenames/hashes still to be pinned (see licenses/THIRD-PARTY-NATIVE.txt).
  echo "libcxx: PENDING, pin Launchpad source files for llvm-toolchain-14 1:14.0.0-1ubuntu1" >&2
  return 1
}

case $what in
  gcc) gcc ;; moonshine) moonshine ;; ort) ort ;; libcxx) libcxx ;;
  all) gcc; moonshine; ort; libcxx ;;
  *) echo "unknown: $what" >&2; exit 2 ;;
esac
( cd "$OUT" && sha256sum * > SHA256SUMS )
