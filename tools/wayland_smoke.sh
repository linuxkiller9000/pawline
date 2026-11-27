#!/usr/bin/env bash
# Wayland startup/draw smoke test against a headless Weston 9 (sandbox recipe, NOT a real desktop).
# Needs: weston 9 debs extracted to /tmp/wl/root with module paths patched to /tmp/lw9 and /tmp/lw9w
# (see docs/WAYLAND-SMOKE.md). Usage: tools/wayland_smoke.sh /path/to/pawline-binary
set -u
BIN=${1:?binary}
R=/tmp/wl/root
export LD_LIBRARY_PATH=$R/usr/lib/x86_64-linux-gnu:$R/usr/lib/x86_64-linux-gnu/weston:/tmp/deb/root/usr/lib/x86_64-linux-gnu
export XDG_RUNTIME_DIR=/tmp/xrw; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR; rm -f $XDG_RUNTIME_DIR/wl-test*
pkill -x weston; sleep 0.3
$R/usr/bin/weston --backend=headless-backend.so --shell=kiosk-shell.so --use-pixman --socket=wl-test --width=800 --height=600 --no-config >/tmp/weston.log 2>&1 &
sleep 2
mkdir -p /tmp/ah3 /tmp/ad3
env -u DISPLAY WAYLAND_DISPLAY=wl-test PAWLINE_PERF=1 PAWLINE_ROOT=/tmp/ah3 PAWLINE_DATA=/tmp/ad3 "$BIN" --open >/tmp/wlapp.log 2>&1 &
sleep 5
n=$(grep -c "PERF draw_ms" /tmp/wlapp.log)
alive=$(pgrep -x pawline | wc -l)
pkill -x pawline; pkill -x weston
echo "wayland smoke: redraws=$n process_alive_after_5s=$alive"
[ "$n" -ge 1 ] && [ "$alive" -ge 1 ]
