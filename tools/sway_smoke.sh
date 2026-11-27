#!/usr/bin/env bash
# usage: sway_smoke.sh BIN OUTPNG   (headless sway 1.7 + wlroots 0.15, pixman; typed text via wtype; grim screenshot)
BIN=$1; OUT=$2
X=/tmp/wl/sw/x
export LD_LIBRARY_PATH=$X/usr/lib/x86_64-linux-gnu:/tmp/wl/root/usr/lib/x86_64-linux-gnu:/tmp/deb/root/usr/lib/x86_64-linux-gnu
export XDG_RUNTIME_DIR=/tmp/xrs; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR; rm -f $XDG_RUNTIME_DIR/*
pkill -x sway; sleep 0.3
printf 'output HEADLESS-1 resolution 1024x768\nxwayland disable\n' > /tmp/sway.conf
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman $X/usr/bin/sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sleep 3; ls $XDG_RUNTIME_DIR | head -3
SOCK=$(ls $XDG_RUNTIME_DIR | grep '^wayland-[0-9]$' | head -1)
export WAYLAND_DISPLAY=$SOCK
rm -rf /tmp/ad5 /tmp/ah5; mkdir -p /tmp/ad5 /tmp/ah5
env -u DISPLAY PAWLINE_PERF=1 PAWLINE_ROOT=/tmp/ah5 PAWLINE_DATA=/tmp/ad5 $BIN --open >/tmp/swapp.log 2>&1 &
sleep 4
$X/usr/bin/grim /tmp/sw_before.png 2>&1 | head -2
$X/usr/bin/wtype -s 300 "make a folder called wltest" -s 600 -k Return 2>&1 | head -2; sleep 1.5
$X/usr/bin/grim /tmp/sw_card.png
$X/usr/bin/grim $OUT 2>&1 | head -2
echo "redraws=$(grep -c PERF /tmp/swapp.log)"; ls /tmp/ah5
pkill -x pawline; pkill -x sway
