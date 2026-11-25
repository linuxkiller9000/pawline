. /tmp/ui3-lib.sh
H=/tmp/VoiceUI; S=/tmp/shots/vui; mkdir -p /tmp/shots
export XDG_RUNTIME_DIR=/tmp/xdg
pkill -f release/pawline; pkill sway; pkill swaybg; pkill -f vkd; pkill pulseaudio; sleep 1
rm -f /tmp/xdg/sway-ipc* /tmp/vk.fifo /tmp/pipesrc; mkfifo /tmp/vk.fifo; rm -rf $H; mkdir -p $H
setsid $BW --setenv WLR_BACKENDS headless --setenv WLR_LIBINPUT_NO_DEVICES 1 --setenv WLR_RENDERER pixman sway -c /tmp/scan/swaycfg/config > /tmp/nsway.log 2>&1 &
sleep 5
setsid $BW python3 /tmp/vkd3.py > /tmp/vkd.log 2>&1 &
XDG_RUNTIME_DIR=/tmp/xdgpa setsid pulseaudio --daemonize=no -n --exit-idle-time=-1 -L module-native-protocol-unix -L "module-pipe-source file=/tmp/pipesrc source_name=pipe format=float32le rate=16000 channels=1" >/tmp/pa.log 2>&1 &
sleep 2
export PULSE_SERVER=unix:/tmp/xdgpa/pulse/native PULSE_SOURCE=pipe
setsid $BW --setenv PATH /usr/bin --setenv HOME $H --setenv PAWLINE_ROOT $H --setenv PAWLINE_START_EXPANDED 1 --setenv PAWLINE_VOICE_DIR /tmp/vdir --setenv PULSE_SERVER $PULSE_SERVER --setenv PULSE_SOURCE pipe --setenv PULSE_COOKIE /home/sandbox/.config/pulse/cookie ~/pawline/target/release/pawline > /tmp/nat.log 2>&1 &
sleep 3
for p in $(pgrep -f "release/pawline"); do echo "PID $p sha=$(sha256sum /proc/$p/exe | cut -c1-16)"; done
$BW python3 /tmp/vptr.py 1000 200 1280 720 >/dev/null 2>&1; sleep 1
$BW grim $S-0.png
outs(){ XDG_RUNTIME_DIR=/tmp/xdgpa pactl list short source-outputs | wc -l; }
echo 66 > /tmp/vk.fifo; sleep 5
dict(){ echo d67 > /tmp/vk.fifo; sleep 1; (cat /tmp/es/mk.f32 > /tmp/pipesrc &); sleep 4; echo u67 > /tmp/vk.fifo; sleep 5; }
echo "== dictate #1"; dict; $BW grim $S-p1.png
echo 28 > /tmp/vk.fifo; sleep 3; $BW grim $S-p2.png; echo "files after Enter (before any Do it): [$(ls $H | tr '\n' ' ')]"
echo 28 > /tmp/vk.fifo; sleep 2; $BW grim $S-p3.png; echo "files after Cancel(Enter on default): [$(ls $H | tr '\n' ' ')]"
echo "== dictate #2"; dict; echo 28 > /tmp/vk.fifo; sleep 3; echo 15 > /tmp/vk.fifo; sleep 1; $BW grim $S-p4.png; echo "files before Do it (Do it focused only): [$(ls $H | tr '\n' ' ')]"
echo 28 > /tmp/vk.fifo; sleep 3; $BW grim $S-p5.png; echo "files after Do it: [$(ls $H | tr '\n' ' ')]"
