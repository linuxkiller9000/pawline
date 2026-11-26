#!/bin/bash
# Fail-fast validate, then commit, push (new branch only) and bundle. Usage: safe_ship.sh BRANCH_SUFFIX BUNDLE_NUM "commit message"
# Refuses to commit or push if tests exit non-zero, any "test result" shows failures, or the Windows build fails.
# SHIP_TEST_CMD / SHIP_WIN_CMD override the commands (used to prove the refusal path).
set -u
cd "$(dirname "$0")/.." || exit 90
[ $# -eq 3 ] || { echo "REFUSED: need BRANCH_SUFFIX BUNDLE_NUM MESSAGE"; exit 91; }
suffix=$1; num=$2; msg=$3
case "$suffix" in main|master|""|*[!A-Za-z0-9._-]*) echo "REFUSED: bad branch suffix"; exit 92;; esac
export PATH=$HOME/.cargo/bin:$PATH PKG_CONFIG_PATH=/tmp/deb/root/usr/lib/x86_64-linux-gnu/pkgconfig LIBRARY_PATH=/tmp/deb/root/usr/lib/x86_64-linux-gnu CACTUS_CPP_STATIC_DIR=/tmp/stdir
log=/tmp/ship-test.log
TESTCMD=${SHIP_TEST_CMD:-cargo test --workspace --no-fail-fast}
WINCMD=${SHIP_WIN_CMD:-env PATH=/tmp/mg/bin:/tmp/mg/root/usr/bin:$PATH cargo build -p pawline-native --target x86_64-pc-windows-gnu --target-dir /tmp/wintarget}
$TESTCMD > "$log" 2>&1; rc=$?
if [ $rc -ne 0 ]; then echo "REFUSED: tests exit=$rc (no commit, no push)"; exit 93; fi
failed=$(grep "^test result" "$log" | awk '{f+=$6} END{print f+0}')
passed=$(grep "^test result" "$log" | awk '{p+=$4} END{print p+0}')
if [ "$failed" -ne 0 ] || grep -q "^test .* FAILED" "$log"; then echo "REFUSED: $failed failed tests"; exit 94; fi
if [ "${SHIP_TEST_CMD:-}" = "" ] && [ "$passed" -eq 0 ]; then echo "REFUSED: zero tests ran"; exit 95; fi
$WINCMD > /tmp/ship-win.log 2>&1; wrc=$?
if [ $wrc -ne 0 ]; then echo "REFUSED: windows build exit=$wrc (no commit, no push)"; exit 96; fi
echo "VALIDATED: tests exit=0 $passed passed $failed failed; windows build exit=0 (build only)"
git add -A
git commit -qm "$msg" || { echo "REFUSED: commit failed or nothing to commit"; exit 97; }
git push -q git@github.com:linuxkiller9000/pawline.git "HEAD:refs/heads/successor/work-20261126-$suffix" || { echo "PUSH FAILED"; exit 98; }
git bundle create /tmp/successor-consolidated$num.bundle 5c6c5eb..HEAD >/dev/null 2>&1 || { echo "BUNDLE FAILED"; exit 99; }
echo "SHIPPED HEAD=$(git rev-parse HEAD) branch=successor/work-20261126-$suffix bundle=/tmp/successor-consolidated$num.bundle"
