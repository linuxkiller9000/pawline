# pawline-llama

llama.cpp inside Pawline: same GGUF files, no `llama-server`, no port, no second
terminal. llama.cpp is MIT (Copyright (c) 2023-2026 The ggml authors); its
LICENSE travels with the source tree that gets built.

## Getting the source

The source is not committed (200 MB unpacked). Fetch the pinned tag and check
the checksum before building:

    curl -fsSLO https://github.com/ggml-org/llama.cpp/archive/refs/tags/b12811.tar.gz
    echo "d7fc9bdd36e72c99f64e253fc581bc3deea73f27fded8fe1b2faf7e7fdef1252  b12811.tar.gz" | sha256sum -c -
    tar xzf b12811.tar.gz            # -> llama.cpp-b12811/
    export PAWLINE_LLAMA_SRC=$PWD/llama.cpp-b12811

or put it at `vendor/llama.cpp` (the default the build script looks for).

## Building and running

    cargo build -p pawline-llama --features vendored
    PAWLINE_THREADS=1 cargo run -p pawline-llama --features vendored \
        --example answer -- /path/model.gguf "Name a fruit."

Without `--features vendored` the crate compiles a stub: `available()` is false
and nothing links C++, so the rest of the workspace builds with no C++ toolchain.

The cmake build is minutes long on one core. Reuse a tree you already built:

    export PAWLINE_LLAMA_BUILD_DIR=/path/to/llama-build

## What ships

`-DGGML_CPU_ALL_VARIANTS=ON`, so the binary carries one CPU kernel per
instruction set and the machine picks at runtime (the i3-370M needs `sse42`;
a native-only build would die there with an illegal instruction).

## Limits, measured

See `docs/INDEPENDENT-RUNTIME.md` and `docs/EXPERIMENTS.md`. Short version: a
first question is the same speed as llama-server; a repeat is currently slower
because the shim clears the KV cache between answers, and prefix reuse is the
next step. The vision path is still on llama-server.
