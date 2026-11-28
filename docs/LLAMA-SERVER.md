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

## How the user asks for it

Typing any of `get a llama-server`, `install llama-server`, `llama server install`
or `set up llama-server` (and the close variants) runs detection first:

1. **Offline mode on** -> a plain refusal, nothing downloaded, no card.
2. **A llama-server already there** (Pawline's own install, then `PATH`) -> the
   path is named and nothing is downloaded. An existing server is never replaced.
3. **No build pinned for this platform** -> said plainly, nothing downloaded.
4. **Otherwise** -> the confirmation card, which names the source, the exact
   size, where it goes, that no root is needed, that the download is checked
   against a fixed sha256 and that the binary has to start before Pawline calls
   it installed. Focus starts on **Cancel**; only `Do it` (or Left/Right then
   Enter) starts the download. Typing never approves it, Esc cancels.

After a yes: `install_live` streams progress lines (`Downloading 12 of 23 MB...`,
`Unpacking...`, `Checking that llama-server starts...`), checks size and sha256,
extracts with the system `tar`, runs `--version` once, and only then reports the
path. Esc during the download stops it at the next block and the partial file is
removed, so a cancelled or failed run leaves no half-install. Asking again after
an install answers from the installed copy, with no second download.

## Measured end to end (sandbox, this branch)

- Card shown for `install llama-server` with a throwaway `HOME`; approved with the
  keyboard; message: "llama-server is installed at
  `<home>/.local/share/pawline/llama/b12811/llama-b12811/llama-server` (22 MB
  downloaded, checksum matched, and it started when asked)".
- 58 MB on disk after extraction; 14 `libggml-cpu-*.so` backends including
  `libggml-cpu-sse42.so`, which is what makes a no-AVX CPU work.
- `llama-server --version` -> `version: 1.1.0-dev (build 12811, commit 00d5c142c)`,
  `built with GNU 11.4.0 for Linux x86_64`.
- Wall clock for download + verify + unpack + start check in this sandbox: 1.9 s
  (fast link). On a limited data plan the 22.4 MB download is the real cost, which
  is exactly why the size is on the card before anything starts.

## Tests

- `pawline-core` (7 pure tests): pinned asset, card wording, short download
  refused, wrong checksum refused, no extraction on a bad download, a verified
  download lands where expected, detection order.
- `pawline-native`: `asking_for_a_llama_server_offers_a_card_and_never_downloads_by_itself`
  covers the offline refusal, the existing-copy answer, the card, and that Cancel
  clears it. `an_approved_llama_card_installs_for_real` is `#[ignore]`d because it
  downloads 22.4 MB; run it by hand with a throwaway home:
  `HOME=/tmp/pw-home cargo test -p pawline-native --bin pawline -- --ignored --exact bridge::memory_tests::an_approved_llama_card_installs_for_real`
