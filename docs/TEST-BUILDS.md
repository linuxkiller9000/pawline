# Publishing a Linux test build

Written down after doing it once the hard way (v0.3.1-test.1, 2026-11-28), so the
next one takes two minutes.

There is no token on the build box and no need for one. The chain is:

1. **Tag the commit** (annotated, on the branch you want built):

   ```
   git tag -a v0.3.1-test.1 -m "one line about what is in it" <sha>
   git push git@github.com:linuxkiller9000/pawline.git refs/tags/v0.3.1-test.1
   ```

2. **Create the release as a pre-release** at
   `https://github.com/linuxkiller9000/pawline/releases/new`, selecting the tag.
   Title, and a body with: what changed, how to run it, and the honest limits.
   The "Pre-release" radio must be set. Do not mark it latest.

3. **Run the CI build** at
   `https://github.com/linuxkiller9000/pawline/actions/workflows/build-native.yml`:
   "Run workflow", choose the branch that holds the code, set `tag` to the tag
   from step 1, and tick `publish`. The job builds on ubuntu-22.04 with a static
   libxkbcommon and static libc++, fails if the binary needs anything beyond
   glibc, and with `publish` attaches the binary and `SHA256SUMS.txt` to the
   release.

4. **Verify the published file**, do not trust the green tick alone:

   ```
   curl -sL -o /tmp/sums https://github.com/linuxkiller9000/pawline/releases/download/<tag>/SHA256SUMS.txt
   curl -sL -o /tmp/bin  https://github.com/linuxkiller9000/pawline/releases/download/<tag>/pawline-<tag>-linux-x86_64
   sha256sum /tmp/bin && cat /tmp/sums && file /tmp/bin
   ```

   The two hashes must match and `file` must say ELF 64-bit x86-64.

Building locally is not part of shipping: on a 1-vCPU box the release build of the
app crate alone takes many minutes, and CI does it faster and with the linkage
check. A local build is only for measuring.

Limits that stay: test builds are pre-releases, never a stable release; nothing
is force-pushed; the work branch is the source of truth.
