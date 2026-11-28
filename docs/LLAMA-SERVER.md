# llama-server: find it, or install it for the user

Asked 2026-11-28: "make it automate llama server installation and usage". The
reason is measured: on a CPU without AVX the vision encoder inside Pawline costs
tens of seconds per new screen, while a `llama-server` build uses every core and
is what already felt fast on his machine. So the server stays the fast path, and
Pawline should stop asking the user to install it by hand.

## The order, and why

1. **A llama-server Pawline installed before** - `~/.local/share/pawline/llama/<tag>/llama-<tag>/llama-server`.
2. **One on PATH**, or one sitting next to the model file the user pointed at.
3. **Offer to install one** (a card, then a download the user agreed to).
4. **The engine inside the binary** as the no-install fallback, when this build
   has it (`--features engine`) and the model files are set.
5. Otherwise say plainly that nothing can look at the screen, and say why.

## The card

Nothing is downloaded because Pawline decided to. The card names:

- the source: the official llama.cpp release `b12811`, CPU build, from
  `github.com/ggml-org/llama.cpp`
- the exact asset and size: `llama-b12811-bin-ubuntu-x64.tar.gz`, 22.4 MB
  (23,491,893 bytes)
- the checksum it will be verified against:
  `d6129b610ad889c80545177a462f49b7960fbddecccdb82a176150a9f0cfede0`
- the destination: `~/.local/share/pawline/llama/b12811`, no root, nothing
  system-wide, removed by deleting one folder
- that it is the CPU build, so a processor without AVX still runs it

The size is on the card because his data plan is limited: 22 MB is a decision he
gets to make, not a surprise.

## Why the CPU build

The release ships one ggml backend per instruction set (`x64`, `sse42`,
`sandybridge`, `haswell`, ...) and picks at run time. That is what makes the same
tarball work on an i3-370M with no AVX. The CUDA, ROCm, Vulkan, SYCL and OpenVINO
builds are much larger and pointless on his machine.

## The steps, and what stops each one

1. fetch the tarball (the only network step; nothing is sent about the user)
2. refuse unless the byte count matches exactly, then check sha-256
3. extract into the destination folder with the system `tar`
4. refuse unless `llama-server` is really there
5. run `llama-server --version` once and refuse if it does not start
6. hand the path to the managed runner, which already starts, supervises and
   reaps it

A failure at any step leaves the previous state alone: nothing half-installed is
used, and the folder is not treated as present unless the binary is.

## What is in the tree today

- `pawline-core/src/llamainstall.rs`: detection order, the pinned candidate, the
  card text, the checksum and size checks, and `install()` with fetch, extract
  and binary-check injected. Seven tests, all pure: no network, no tarball, no
  real binary needed. They cover the pinned asset, the card wording (size,
  destination, checksum, "no root", "nothing is downloaded until you say yes"),
  a short download being refused, a wrong checksum being refused, extraction
  never running on a bad download, a verified download landing where expected,
  and the detection order.
- The screen path now looks for a Pawline-installed copy before PATH or the model
  folder, so an install made here is used without any further setup.

## What is not wired yet

The card itself, and the download button behind it: that needs the existing
confirmation-card plumbing in the bridge, so a "yes" approves exactly the asset,
size and checksum the card showed, and nothing else. Until that exists the module
is used for detection only, and a machine with no server still gets the honest
"nothing can look at the screen" message. That is the next piece of work, and it
gets the same rule as every other download: the user's yes first.
