#!/usr/bin/env bash
BIN=$1; N=$2; X=/tmp/wl/sw/x
export LD_LIBRARY_PATH=$X/usr/lib/x86_64-linux-gnu:/tmp/wl/root/usr/lib/x86_64-linux-gnu:/tmp/deb/root/usr/lib/x86_64-linux-gnu
export XDG_RUNTIME_DIR=/tmp/xrs; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR; rm -f $XDG_RUNTIME_DIR/*
pkill -x sway; sleep 0.3
printf 'output HEADLESS-1 resolution 1024x768\nxwayland disable\n' > /tmp/sway.conf
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman $X/usr/bin/sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
sleep 3; export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep '^wayland-[0-9]$' | head -1)
env -u DISPLAY PAWLINE_ROOT=/tmp/ah2 PAWLINE_DATA=/tmp/ad2 $BIN --open >/tmp/swapp.log 2>&1 &
sleep 4; $X/usr/bin/grim /tmp/swh0_$N.png
/tmp/vptr/target/release/vptr wheel 800 250 -6; sleep 1
$X/usr/bin/grim /tmp/swh1_$N.png
pkill -x pawline; pkill -x sway
