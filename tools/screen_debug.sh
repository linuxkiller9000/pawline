#!/bin/sh
# Screen self-check for a machine Pawline cannot see.
# Runs exactly the capture commands Pawline runs, in the same order, and says
# which piece is missing or failing. Writes nothing outside a temp dir.
#
#   sh tools/screen_debug.sh              # capture checks only
#   sh tools/screen_debug.sh --draw       # also draw a region with slurp
#
# Result: screen-debug.txt in the current directory. Send that file back.
set -u
OUT=screen-debug.txt
TMP=$(mktemp -d)
DRAW=0
[ "${1:-}" = "--draw" ] && DRAW=1

say() { echo "$@" | tee -a "$OUT"; }
have() { command -v "$1" >/dev/null 2>&1; }

: > "$OUT"
say "Pawline screen self-check - $(date -u '+%Y-%m-%d %H:%M UTC')"
say "session: XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-<unset>} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-<unset>} DISPLAY=${DISPLAY:-<unset>}"
say "nproc: $(nproc 2>/dev/null || echo '?')"
say ""
say "== what is installed =="
for t in niri grim wayshot slurp tesseract llama-server; do
    if have "$t"; then say "  $t: $(command -v "$t")"; else say "  $t: MISSING"; fi
done

say ""
say "== whole screen, the way Pawline does it =="
if have grim; then
    if grim "$TMP/grim-full.png" 2>"$TMP/e"; then say "  grim full: OK ($(wc -c <"$TMP/grim-full.png") bytes)"; else say "  grim full: FAILED - $(cat "$TMP/e")"; fi
elif have wayshot; then
    if wayshot --file "$TMP/wayshot-full.png" 2>"$TMP/e"; then say "  wayshot full: OK"; else say "  wayshot full: FAILED - $(cat "$TMP/e")"; fi
else
    say "  grim/wayshot: not installed"
fi
if have niri; then
    rm -f "$TMP/niri-screen.png"
    niri msg action screenshot-screen --show-pointer false --path "$TMP/niri-screen.png" >>"$TMP/e" 2>&1
    i=0; while [ $i -lt 20 ] && [ ! -s "$TMP/niri-screen.png" ]; do sleep 0.25; i=$((i+1)); done
    if [ -s "$TMP/niri-screen.png" ]; then
        say "  niri screenshot-screen: OK ($(wc -c <"$TMP/niri-screen.png") bytes, after ~$i x 0.25 s)"
    else
        say "  niri screenshot-screen: NO FILE - $(cat "$TMP/e")"
    fi
fi

say ""
say "== one window =="
if have niri; then
    niri msg --json windows > "$TMP/wins.json" 2>"$TMP/e" || say "  niri msg windows: FAILED - $(cat "$TMP/e")"
    WID=$(tr ',{' '\n\n' < "$TMP/wins.json" | sed -n 's/.*"id": *\([0-9][0-9]*\).*/\1/p' | head -1)
    if [ -n "${WID:-}" ]; then
        rm -f "$TMP/niri-win.png"
        niri msg action screenshot-window --path "$TMP/niri-win.png" --show-pointer false --id "$WID" >>"$TMP/e" 2>&1
        i=0; while [ $i -lt 20 ] && [ ! -s "$TMP/niri-win.png" ]; do sleep 0.25; i=$((i+1)); done
        if [ -s "$TMP/niri-win.png" ]; then say "  niri screenshot-window (id $WID): OK"; else say "  niri screenshot-window (id $WID): NO FILE - $(cat "$TMP/e")"; fi
    else
        say "  niri: no window id found (is a window open?)"
    fi
else
    say "  niri: not installed, so window questions have nothing to ask"
fi

say ""
say "== drawn region =="
if [ "$DRAW" = "1" ]; then
    if have slurp && have grim; then
        G=$(slurp 2>"$TMP/e")
        if [ -n "${G:-}" ]; then
            say "  slurp gave: $G"
            if grim -g "$G" "$TMP/region.png" 2>>"$TMP/e"; then say "  grim -g: OK"; else say "  grim -g: FAILED - $(cat "$TMP/e")"; fi
        else
            say "  slurp: nothing drawn"
        fi
    else
        say "  slurp or grim: missing"
    fi
else
    say "  skipped (run with --draw to test it)"
fi

say ""
say "== vision server =="
for port in 8080 8081 11434; do
    if curl -fsS -m 2 "http://127.0.0.1:$port/props" 2>/dev/null | grep -q '"model_path"'; then
        say "  llama-server on :$port answers"
        curl -fsS -m 2 "http://127.0.0.1:$port/props" 2>/dev/null | grep -o '"model_path":"[^"]*"' | sed 's/^/    /' | tee -a "$OUT" >/dev/null
    fi
done
say "  (nothing listed above means no llama-server on 8080/8081/11434)"

rm -rf "$TMP"
say ""
say "Done. Send $OUT back."
