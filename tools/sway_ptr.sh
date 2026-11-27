#!/usr/bin/env bash
BIN=$1; N=$2
X=/tmp/wl/sw/x
export LD_LIBRARY_PATH=$X/usr/lib/x86_64-linux-gnu:/tmp/wl/root/usr/lib/x86_64-linux-gnu:/tmp/deb/root/usr/lib/x86_64-linux-gnu
export XDG_RUNTIME_DIR=/tmp/xrs; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR; rm -f $XDG_RUNTIME_DIR/*
pkill -x sway; sleep 0.3
printf 'output HEADLESS-1 resolution 1024x768\nxwayland disable\n' > /tmp/sway.conf
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman $X/usr/bin/sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sleep 3
export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep '^wayland-[0-9]$' | head -1)
export SWAYSOCK=$(ls $XDG_RUNTIME_DIR/sway-ipc.* | head -1)
rm -rf /tmp/ad5 /tmp/ah5; mkdir -p /tmp/ad5 /tmp/ah5
env -u DISPLAY PAWLINE_PERF=1 PAWLINE_ROOT=/tmp/ah5 PAWLINE_DATA=/tmp/ad5 $BIN --open >/tmp/swapp.log 2>&1 &
sleep 4
S=$X/usr/bin/swaymsg
# 1. click the hint bubble (copies the reply)
$X/usr/bin/wtype -s 200 "x" -k BackSpace; sleep 0.5; /tmp/vptr/target/release/vptr click 700 370; sleep 1
echo "clipboard after bubble click: [$($X/usr/bin/wl-paste 2>&1 | head -c 60)]"
# 2. click the gear (settings)
/tmp/vptr/target/release/vptr click 947 41; sleep 1.2
$X/usr/bin/grim /tmp/swp_$N.png
echo "redraws=$(grep -c PERF /tmp/swapp.log)"
pkill -x pawline; pkill -x sway
