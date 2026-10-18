#!/usr/bin/env bash
# Smoke tests for bin/frame. No screenshots, recordings, clipboard,
# notifications or shortcut changes: capture/bind commands run with
# FRAME_DRY_RUN=1 against a temp config and temp XDG dirs. The few real
# runs (record stop, daemon TERM, GNOME read failure) use fake tools put
# first on PATH; fake notify-send / wl-copy log any call so we can check
# that nothing real was attempted.
# Run from anywhere: bash tests/smoke.sh
set -uo pipefail

FRAME="$(cd "$(dirname "$0")/.." && pwd)/bin/frame"
T="$(mktemp -d)"
trap 'pkill -KILL -f "$T/fake/gpu-screen-recorder" 2>/dev/null; rm -rf "$T"' EXIT
mkdir -p "$T/run" "$T/fake" "$T/fakesleep"
export FRAME_CONFIG="$T/config" XDG_RUNTIME_DIR="$T/run" XDG_DATA_HOME="$T/data" \
       XDG_CONFIG_HOME="$T/cfg" XDG_STATE_HOME="$T/state" XDG_CACHE_HOME="$T/cache" \
       TMPDIR="$T" FRAME_DRY_RUN=1 XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE \
       FRAME_GUI="$T/fake/frame-gui"
unset HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK
export PATH="$T/fake:$PATH" FRAME T
fail=0 n=0

# ---- fake tools
cat >"$T/fake/gpu-screen-recorder" <<'EOF'
#!/usr/bin/env python3
# Fake recorder: writes -o FILE on INT/TERM. FAKE_GSR=stubborn ignores both.
import os, signal, sys, time
a = sys.argv[1:]
if a and a[0] == "--list-monitors":
    # FAKE_MONITORS: what to print, ";" between lines.
    m = os.environ.get("FAKE_MONITORS", "")
    if m:
        print(m.replace(";", "\n"))
    sys.exit(0)
if a and a[0] == "--info":
    sys.exit(0)
out = a[a.index("-o") + 1] if "-o" in a else ""
def done(*_):
    if out and not os.path.isdir(out):
        open(out, "w").write("video")
    sys.exit(0)
h = signal.SIG_IGN if os.environ.get("FAKE_GSR") == "stubborn" else done
signal.signal(signal.SIGINT, h)
signal.signal(signal.SIGTERM, h)
while True:
    time.sleep(0.05)
EOF
cat >"$T/fake/gsettings" <<'EOF'
#!/bin/sh
# FAKE_GSETTINGS: list (default) | fail | garbage. Logs every call.
echo "$*" >>"$T/gsettings.log"
[ "$1" = get ] || exit 0
case "${FAKE_GSETTINGS:-list}" in
    fail) echo "No such schema" >&2; exit 1 ;;
    garbage) echo "No schemas installed" ;;
    *) echo "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/', '/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/frame-all/']" ;;
esac
EOF
cat >"$T/fake/hyprctl" <<'EOF'
#!/bin/sh
[ "$1" = binds ] && echo '[{"modmask":72,"key":"A","dispatcher":"exec","arg":"firefox"}]'
exit 0
EOF
cat >"$T/fake/swaymsg" <<'EOF'
#!/bin/sh
[ "$2" = get_config ] && printf '%s\n' '{"config":"set $mod Mod4\nbindsym --release $mod+Mod1+v exec foo\n"}'
exit 0
EOF
printf '#!/bin/sh\nexit 0\n' >"$T/fake/gnome-screenshot"
cat >"$T/fake/frame-gui" <<'EOF'
#!/bin/sh
# Fake KWin grab: logs its args, writes "png" to the last one.
# FAKE_GUI=fail answers like an unauthorized frame-gui.
echo "$*" >>"$T/gui.log"
[ "$FAKE_GUI" = fail ] && { echo "frame-gui: not authorized: is frame.desktop installed" >&2; exit 1; }
for o; do :; done; echo png >"$o"
EOF
# Fake KWin for video_source auto, only on PATH in the checks that want it.
# FAKE_KWIN: the activeOutputName reply; "fail" exits 1.
mkdir -p "$T/fakekwin"
cat >"$T/fakekwin/gdbus" <<'EOF'
#!/bin/sh
echo "$*" >>"$T/gdbus.log"
case "$*" in *org.kde.KWin.activeOutputName*) ;; *) exit 1 ;; esac
[ "$FAKE_KWIN" = fail ] && exit 1
printf '%s\n' "${FAKE_KWIN:-('DP-2',)}"
EOF
chmod +x "$T/fakekwin/gdbus"
for t in notify-send wl-copy; do
    printf '#!/bin/sh\n[ "$1" = --help ] && { echo "  -A, --action"; exit 0; }\necho "$0 $*" >>"$T/real-calls.log"\n' >"$T/fake/$t"
done
printf '#!/bin/sh\necho $$ >>"$T/sleep.pids"\nexec /usr/bin/sleep "$@"\n' >"$T/fakesleep/sleep"
chmod +x "$T/fake/"* "$T/fakesleep/sleep"

check() {
    local name="$1"; shift
    n=$((n + 1))
    if bash -c "$*" >"$T/out" 2>&1; then echo "ok   $name"
    else echo "FAIL $name"; sed 's/^/     /' "$T/out"; fail=1; fi
}
conf() { printf '%s\n' "save_dir = $T/docs" "$@" >"$FRAME_CONFIG"; }
lib() { echo "source '$FRAME'; $*"; }   # call a function from the script
# real (non-dry) runs: never clipboard or notifications
realconf() { conf 'clipboard = false' 'notify = false' 'video_source = DP-9' 'video_codec = h264' 'audio = none' "$@"; }

check "syntax"            "bash -n '$FRAME'"
check "help"              "'$FRAME' --help | grep -q '^Usage: frame'"
check "version 0.3.0"     "[ \"\$('$FRAME' --version)\" = 'frame 0.3.0' ]"
check "unknown command=2" "'$FRAME' nope; [ \$? -eq 2 ]"
check "config writes default" "'$FRAME' config | grep -qx '$FRAME_CONFIG' && grep -q '^bind.record = Meta+Alt+V' '$FRAME_CONFIG' && grep -q '^save_dir = ~/Documents' '$FRAME_CONFIG' && grep -qF '\$VARS are not'  '$FRAME_CONFIG'"
check "default config parses" "'$FRAME' --detect | grep -qx 'images=$HOME/Documents/frame/images'"

# ---- config parsing and validation
conf 'image_format = jpg   # comment' 'replay_seconds = 45' "\$(touch $T/pwned)" 'bind.window ='
check "config never executed" "'$FRAME' --detect >/dev/null 2>&1; [ ! -e '$T/pwned' ]"
check "config values"     "'$FRAME' --detect 2>/dev/null | grep -qx 'replay_seconds=45' && '$FRAME' --detect 2>/dev/null | grep -qx 'videos=$T/docs/frame/videos'"
printf '%s\n' "save_dir = \"$T/my#docs\"   # real comment" "video_source = 'DP-1'#x" '  # indented comment' "video_codec = 'av1'" >"$FRAME_CONFIG"
check "# only after space, quotes stripped" "out=\$('$FRAME' --detect 2>&1) && grep -qx 'images=$T/my#docs/frame/images' <<<\"\$out\" && grep -qx \"video_source='DP-1'#x\" <<<\"\$out\" && grep -qx 'video_codec=av1' <<<\"\$out\" && ! grep -q ignored <<<\"\$out\""
printf '%s\n' "save_dir = $T/d" 'image_format=jpg#no-space' >"$FRAME_CONFIG"
check "jpg#x is a value, not jpg" "'$FRAME' --detect; [ \$? -eq 2 ]"
for bad in 'image_format = gif' 'save_dir = docs' 'save_dir = ~kevin/x' 'save_dir = $HOME/x' \
           'replay_seconds = 0' 'replay_seconds = 99999' 'video_fps = fast' 'video_quality = max' \
           'video_codec = h265' $'video_format = webm\nvideo_codec = hevc' 'notify = maybe'; do
    printf '%s\n' "save_dir = $T/docs" "$bad" >"$FRAME_CONFIG"
    [[ "$bad" == save_dir* ]] && printf '%s\n' "$bad" >"$FRAME_CONFIG"
    check "rejects: ${bad//$'\n'/; }" "'$FRAME' --detect; [ \$? -eq 2 ]"
done
conf 'image_format = webp   # c' 'video_fps = 30'
check "check: good config" "out=\$('$FRAME' check 2>&1) && [ \"\$out\" = ok ]"
conf 'video_fps = fast'
check "check: bad config exits 2" "out=\$('$FRAME' check 2>&1); [ \$? -eq 2 ] && grep -qx 'frame: video_fps must be 1-240' <<<\"\$out\" && ! grep -qx ok <<<\"\$out\""
t_u3000() {   # U+3000 before # is no space for bash either (load_config runs under LC_ALL=C)
    printf 'save_dir = %s\naudio = x\343\200\200#c\nbind.all = Meta+\342\204\252\n' "$T/docs" >"$T/u3000"
    local out; out=$(FRAME_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$FRAME"; load_config; printf %s "${CFG[audio]}"' 2>&1)
    [ $? -eq 2 ] && grep -q "bind.all: 'Meta+.*' is not a shortcut" <<<"$out" || { echo "$out"; return 1; }   # Kelvin sign is not k
    sed -i '$d' "$T/u3000"
    out=$(FRAME_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$FRAME"; load_config; printf %s "${CFG[audio]}"') || return 1
    [ "$out" = "$(printf 'x\343\200\200#c')" ] || { echo "got [$out]"; return 1; }
    [ "${LC_ALL:-unset}" = "$(FRAME_CONFIG="$T/u3000" bash -c 'source "$FRAME"; load_config >/dev/null 2>&1; echo "${LC_ALL:-unset}"')" ]   # local, not leaked
}
export -f t_u3000
check "config: U+3000 isn't space, Kelvin isn't k" t_u3000
check "check: missing config = defaults" "[ \"\$(FRAME_CONFIG='$T/none' '$FRAME' check)\" = ok ] && [ ! -e '$T/none' ]"
t_migrate() {   # one-time copy of the Clipped config, default path only
    local c="$T/mig" out
    mkdir -p "$c/clipped"; printf '# old\nvideo_fps = 30\n' >"$c/clipped/config"
    out=$(XDG_CONFIG_HOME="$c" FRAME_CONFIG='' "$FRAME" check 2>&1) || { echo "$out"; return 1; }
    [ "$out" = "$(printf 'frame: note: copied your Clipped config %s to %s\nok' "$c/clipped/config" "$c/frame/config")" ] || { echo "$out"; return 1; }
    cmp -s "$c/clipped/config" "$c/frame/config" || return 1
    echo 'video_fps = 45' >"$c/frame/config"
    out=$(XDG_CONFIG_HOME="$c" FRAME_CONFIG='' "$FRAME" check 2>&1) && [ "$out" = ok ] || { echo "$out"; return 1; }
    grep -qx 'video_fps = 45' "$c/frame/config" || return 1
    out=$(XDG_CONFIG_HOME="$T/mig2" FRAME_CONFIG="$T/mig2.conf" bash -c 'mkdir -p "$XDG_CONFIG_HOME/clipped"; echo x=1 >"$XDG_CONFIG_HOME/clipped/config"; "$FRAME" check' 2>&1)
    [ "$out" = ok ] && [ ! -e "$T/mig2.conf" ] && [ ! -e "$T/mig2/frame" ]
}
export -f t_migrate
check "migrates the Clipped config once" t_migrate
printf '%s\n' 'save_dir = ~/x' >"$FRAME_CONFIG"
check "save_dir ~/ expands" "'$FRAME' --detect | grep -qx 'images=$HOME/x/frame/images'"

# ---- shortcuts
conf 'bind.region = x, exec, touch '"$T"'/PWNED'
check "injection rejected (detect)" "'$FRAME' --detect; [ \$? -eq 2 ]"
check "injection rejected (hyprland bind)" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$FRAME' bind 2>&1); [ \$? -eq 2 ] && ! grep -q hyprctl <<<\"\$out\" && [ ! -e '$T/PWNED' ]"
for bad in 'Meta+Foo+R' 'Meta+Alt+' 'Meta+R+T' 'Meta+Alt' 'Meta+,' 'Meta+F36' 'Meta+é'; do
    check "shortcut rejected: $bad" "$(lib "! parse_shortcut '$bad'")"
done
check "shortcut ok: ctrl + shift + print" "$(lib "parse_shortcut ' ctrl + shift + print '")"
conf
check "keyint Meta+Alt+F12" "[ \"\$($(lib qt_keyint Meta+Alt+F12))\" = 419430459 ]"
check "keyint Meta+Alt+R" "[ \"\$($(lib qt_keyint Meta+Alt+R))\" = $((0x18000052)) ]"
check "keyint Ctrl+Shift+Print" "[ \"\$($(lib qt_keyint Ctrl+Shift+Print))\" = $((0x06000000 | 0x01000009)) ]"
check "xkb names" "[ \"\$($(lib "bind_combo sway Meta+PgUp; bind_combo hypr Alt+backspace; bind_combo gnome Ctrl+Ins"))\" = \$'Mod4+Prior\nALT, BackSpace\n<Ctrl>Insert' ]"

check "replay auto 30|60" "'$FRAME' --detect | grep -Eqx 'replay_seconds=(30|60)'"
check "kde wayland -> kwin" "'$FRAME' --detect | grep -qx 'capture=kwin'"
check "kde, no frame-gui -> not kwin" "FRAME_GUI='$T/none' '$FRAME' --detect | grep -Eqx 'capture=(spectacle|none)'"
check "kde x11 -> not kwin" "out=\$(XDG_SESSION_TYPE=x11 '$FRAME' --detect) && grep -q '^capture=' <<<\"\$out\" && ! grep -qx 'capture=kwin' <<<\"\$out\""
t_gui_next() {   # without FRAME_GUI: the frame-gui beside bin/frame wins
    mkdir -p "$T/nb" && cp "$T/fake/frame-gui" "$T/nb/" || return 1
    [ "$(env -u FRAME_GUI bash -c "source '$FRAME'; FRAME_BIN='$T/nb/frame'; gui_bin")" = "$T/nb/frame-gui" ]
}
export -f t_gui_next
check "frame-gui next to frame" t_gui_next
check "codec mp4 prefers av1" "[ \"\$($(lib "pick_codec mp4 \$'h264\nhevc\nav1'"))\" = av1 ]"
check "codec mp4 h264 only"   "[ \"\$($(lib "pick_codec mp4 h264"))\" = h264 ]"
check "codec webm vp9"        "[ \"\$($(lib "pick_codec webm \$'h264\nvp9'"))\" = vp9 ]"
check "uri encodes UTF-8 bytes" "[ \"\$($(lib "file_uri '/h/é Д'"))\" = 'file:///h/%C3%A9%20%D0%94' ]"

# ---- screenshots + toast
mkdir -p "$T/fakeregion"; printf '#!/bin/sh\nexit 1\n' >"$T/fakeregion/slurp"; chmod +x "$T/fakeregion/slurp"  # dry run never calls it
check "region png (kde)"  "out=\$(PATH=\"$T/fakeregion:\$PATH\" '$FRAME' region 2>&1) && grep -q '^+ slurp -b #00000055' <<<\"\$out\" && grep -q '^+ $T/fake/frame-gui --grab area X Y W H $T/frame\.[A-Za-z0-9]*/shot.png\$' <<<\"\$out\" && ! grep -qE 'spectacle|crop=' <<<\"\$out\" && grep -q 'wl-copy --type image/png' <<<\"\$out\" && grep -Eq '/docs/frame/images/[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{2}-[0-9]{2}-[0-9]{2}_region.png\$' <<<\"\$out\" && [ -d '$T/docs/frame/images' ] && ! ls -d '$T'/frame.* 2>/dev/null"
check "toast kde: x-kde-urls + actions" "out=\$('$FRAME' all) && grep -q '^+ notify-send -a frame -i camera-photo -h string:x-kde-urls:file://$T/docs/frame/images/.*_all.png -A open=Open -A .folder=Show in folder. .Screenshot saved. ' <<<\"\$out\" && grep -qx 'Copied to clipboard.' <<<\"\$out\""
check "toast gnome: image-path" "XDG_CURRENT_DESKTOP=GNOME '$FRAME' window | grep -q 'notify-send -a frame -h string:image-path:$T/docs/frame/images/.*_window.png -A open=Open'"
conf 'image_format = webp'
check "window webp"       "out=\$('$FRAME' window) && grep -q 'frame-gui --grab window ' <<<\"\$out\" && grep -q 'libwebp' <<<\"\$out\" && grep -q 'image/webp' <<<\"\$out\""
conf 'notify = false' 'clipboard = false'
check "notify/clipboard off" "out=\$('$FRAME' monitor) && ! grep -qE 'notify-send|wl-copy' <<<\"\$out\" && ! grep -q 'Copied' <<<\"\$out\""
conf
check "kwin monitor/all"  "'$FRAME' monitor | grep -q '^+ $T/fake/frame-gui --grab monitor ' && '$FRAME' all | grep -q '^+ $T/fake/frame-gui --grab all '"
mkdir -p "$T/fakespec"; printf '#!/bin/sh\nexit 1\n' >"$T/fakespec/spectacle"; chmod +x "$T/fakespec/spectacle"
t_spectacle() {   # no frame-gui: Spectacle (dry run) is the fallback
    export PATH="$T/fakespec:$PATH" FRAME_GUI="$T/none"
    "$FRAME" --detect | grep -qx capture=spectacle && "$FRAME" monitor | grep -q 'spectacle -b -n -m' \
        && "$FRAME" all | grep -q 'spectacle -b -n -f' && "$FRAME" window | grep -q 'spectacle -b -n -a' \
        && "$FRAME" region | grep -q 'spectacle -b -n -r'
}
export -f t_spectacle
check "spectacle fallback flags" t_spectacle
# Real mode, fake frame-gui/slurp: the png is saved; a failed grab is
# reported (not "cancelled"); a cancelled selection never calls frame-gui.
t_kwin_real() {
    local out rc; rm -f "$T/gui.log"; export FRAME_DRY_RUN=0 PATH="$T/fakekwin:$PATH"
    mkdir -p "$T/fakekwin"
    printf '#!/bin/sh\necho "${FAKE_SLURP:-10 20 30 40}"; [ -z "$FAKE_SLURP" ]\n' >"$T/fakekwin/slurp"
    chmod +x "$T/fakekwin/slurp"
    out=$("$FRAME" region 2>&1) || { echo "$out"; return 1; }
    [[ "$out" == "$T"/docs/frame/images/*_region.png && "$(cat "$out")" == png ]] || { echo "$out"; return 1; }
    grep -Eqx -- "--grab area 10 20 30 40 $T/frame\.[A-Za-z0-9]+/shot\.png" "$T/gui.log" || { cat "$T/gui.log"; return 1; }
    out=$(FAKE_GUI=fail "$FRAME" all 2>&1); rc=$?
    [ $rc = 1 ] && [ "$out" = "frame: screenshot failed: not authorized: is frame.desktop installed" ] || { echo "rc=$rc $out"; return 1; }
    rm -f "$T/gui.log"; out=$(FAKE_SLURP=" " "$FRAME" region 2>&1); rc=$?
    [ $rc = 1 ] && [ "$out" = "frame: cancelled" ] && [ ! -e "$T/gui.log" ] || { echo "rc=$rc $out"; return 1; }
    [ "$(ls "$T/docs/frame/images" | wc -l)" = 1 ] && ! ls -d "$T"/frame.* 2>/dev/null
}
export -f t_kwin_real
realconf
check "kwin real mode (fake gui): save, fail, cancel" t_kwin_real
conf
if command -v grim >/dev/null; then
    check "hyprland grim"  "XDG_CURRENT_DESKTOP=Hyprland '$FRAME' window | grep -q '^+ grim -g ' && XDG_CURRENT_DESKTOP=sway '$FRAME' monitor | grep -q '^+ grim -o '"
fi

# ---- video
conf 'video_source = DP-9' 'video_codec = hevc' 'video_format = mkv' 'audio = none'
check "record cmd"        "out=\$('$FRAME' record) && grep -q '^+ gpu-screen-recorder -w DP-9 -c mkv -k hevc -f 60 -q very_high -sc ' <<<\"\$out\" && ! grep -q -- ' -a ' <<<\"\$out\" && grep -Eq '_record.mkv\$' <<<\"\$out\""
# auto source: the monitor KWin calls active, if gpu-screen-recorder lists it
K="PATH='$T/fakekwin:$PATH' FAKE_MONITORS='DP-1|2560x1440;DP-2|2560x1440'"
conf 'video_source = auto' 'video_codec = hevc' 'audio = none'
check "auto source = KWin active output" "out=\$($K '$FRAME' --detect) && grep -qx video_source=DP-2 <<<\"\$out\" && grep -q -- '--timeout 2 -d org.kde.KWin -o /KWin -m org.kde.KWin.activeOutputName' '$T/gdbus.log'"
check "auto record follows the active monitor" "$K FAKE_KWIN=\"('DP-1',)\" '$FRAME' record | grep -q '^+ gpu-screen-recorder -w DP-1 ' && $K '$FRAME' record | grep -q '^+ gpu-screen-recorder -w DP-2 '"
check "auto: unlisted/garbage/failed KWin = first monitor" "for k in \"('HDMI-9',)\" \"('DP-2'); rm -rf ~\" DP-2 fail; do out=\$($K FAKE_KWIN=\"\$k\" '$FRAME' --detect) && grep -qx video_source=DP-1 <<<\"\$out\" || exit 1; done"
check "auto: no monitors listed = screen" "PATH='$T/fakekwin:$PATH' '$FRAME' --detect | grep -qx video_source=screen"
check "auto: KWin not asked off KDE" "rm -f '$T/gdbus.log'; out=\$(XDG_CURRENT_DESKTOP=GNOME $K '$FRAME' --detect) && grep -qx video_source=DP-1 <<<\"\$out\" && [ ! -e '$T/gdbus.log' ]"
printf 'video_source = auto\nimage_format = bmp\n' >"$FRAME_CONFIG"
check "monitors: list, active marked, any config" "out=\$($K '$FRAME' monitors) && [ \"\$out\" = \"\$(printf 'DP-1|2560x1440\nDP-2|2560x1440|active')\" ] && [ -z \"\$(PATH='$T/fakekwin:$PATH' '$FRAME' monitors)\" ]"
check "monitors: no gpu-screen-recorder = error" "out=\$($(lib 'have() { [ "$1" != gpu-screen-recorder ]; }; main monitors') 2>&1); [ \$? -eq 1 ] && grep -q 'gpu-screen-recorder is not installed' <<<\"\$out\""
conf 'video_source = DP-9' 'video_codec = hevc' 'video_format = mkv' 'audio = none'
check "ffmpeg webm = vp9 + opus" "out=\$($(lib 'CFG[video_format]=webm; ffmpeg_args /v.webm | paste -sd" "')) && grep -q -- '-c:v libvpx-vp9 -crf 32 -b:v 0 ' <<<\"\$out\" && grep -q -- '-c:a libopus' <<<\"\$out\" && ! grep -q x264 <<<\"\$out\""
check "ffmpeg mp4 = x264 + aac" "out=\$($(lib 'ffmpeg_args /v.mp4 | paste -sd" "')) && grep -q -- '-c:v libx264 ' <<<\"\$out\" && grep -q -- '-c:a aac' <<<\"\$out\" && grep -q ' /v.mp4\$' <<<\"\$out\""
check "toast video: thumb + titles" "out=\$('$FRAME' _saved /x/Replay_1.mp4 replay 2>&1) && grep -q 'ffmpeg -loglevel error -y -ss 1 -i $T/docs/frame/videos/.*_replay.mp4 -frames:v 1 -vf scale=512:-2 $T/cache/frame/thumbs/' <<<\"\$out\" && grep -q 'x-kde-urls:file://.*_replay.mp4 -h string:image-path:$T/cache/frame/thumbs/.*_replay.png .*Clip saved (' <<<\"\$out\" && '$FRAME' _saved /x/r.webm | grep -q \"'Recording saved'\""

t_record_lock() {
    mkdir -p "$T/run/frame"
    flock "$T/run/frame/record.lock" sleep 2 & sleep 0.3
    out=$("$FRAME" record 2>&1); rc=$?
    wait
    [ $rc -eq 1 ] && grep -q 'already starting or stopping' <<<"$out" && ! grep -q gpu-screen-recorder <<<"$out"
}
t_stale_pid() {
    mkdir -p "$T/run/frame"
    /usr/bin/sleep 30 & victim=$!
    printf '%s 1\n/x.mp4\ngsr\n' "$victim" >"$T/run/frame/record.pid"
    out=$("$FRAME" record 2>&1)
    alive=0; kill -0 "$victim" 2>/dev/null && alive=1
    kill "$victim"; rm -f "$T/run/frame/record.pid"
    [ $alive = 1 ] && ! grep -q 'kill' <<<"$out" && grep -q '^+ gpu-screen-recorder' <<<"$out"
}
t_record_real() {   # fake recorder, real start/stop logic
    out=$(FRAME_DRY_RUN=0 "$FRAME" record) || return 1
    read -r pid start <"$T/run/frame/record.pid"
    [ -n "$start" ] && kill -0 "$pid" || return 1
    f=$(FRAME_DRY_RUN=0 "$FRAME" record) || return 1
    [ "$(cat "$f")" = video ] && [ ! -e "$T/run/frame/record.pid" ] && ! kill -0 "$pid" 2>/dev/null
}
t_record_stubborn() {
    FRAME_DRY_RUN=0 FAKE_GSR=stubborn "$FRAME" record >/dev/null || return 1
    read -r pid _ <"$T/run/frame/record.pid"
    out=$(FRAME_DRY_RUN=0 FRAME_STOP_WAIT=1 "$FRAME" record 2>&1); rc=$?
    kept=0; [ -e "$T/run/frame/record.pid" ] && kept=1
    kill -KILL "$pid"; rm -f "$T/run/frame/record.pid"
    [ $rc = 1 ] && [ $kept = 1 ] && grep -q 'sending TERM' <<<"$out" && grep -q 'still running' <<<"$out"
}
t_daemon_term() {   # $1 = replay true|false
    : >"$T/sleep.pids"
    FRAME_DRY_RUN=0 XDG_CURRENT_DESKTOP=none PATH="$T/fakesleep:$PATH" "$FRAME" daemon >"$T/daemon.out" 2>&1 &
    d=$!
    for _ in $(seq 30); do [ -s "$T/run/frame/daemon.pid" ] && { [ "$1" = false ] || [ -s "$T/run/frame/replay.pid" ]; } && break; sleep 0.1; done
    [ "$1" = true ] && read -r g _ <"$T/run/frame/replay.pid"
    sleep 0.3; kill -TERM "$d"
    for _ in $(seq 30); do kill -0 "$d" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$d" 2>/dev/null && { kill -KILL "$d"; echo "daemon still running"; return 1; }
    for s in $(cat "$T/sleep.pids"); do kill -0 "$s" 2>/dev/null && { echo "sleep $s left behind"; return 1; }; done
    [ "$1" = false ] || { ! kill -0 "$g" 2>/dev/null || { echo "recorder left"; return 1; }; }
    [ ! -e "$T/run/frame/daemon.pid" ] && grep -q 'daemon stopped' "$T/daemon.out"
}
export -f t_record_lock t_stale_pid t_record_real t_record_stubborn t_daemon_term
check "record lock blocks 2nd press" t_record_lock
check "stale pid file not signalled" t_stale_pid
realconf
check "record start/stop (fake gsr)" t_record_real
check "stuck recorder: TERM, keep pid, exit 1" t_record_stubborn
realconf 'replay = true' 'replay_seconds = 20'
check "daemon TERM stops recorder" "t_daemon_term true"
realconf 'replay = false'
check "daemon TERM kills its sleep" "t_daemon_term false"
# daemon.pid and the TERM trap are set before binding: a slow bind (fake
# hyprctl, 1 s per `binds`) already sees the pid file, and a TERM sent
# then stops the daemon once the bind returns.
mkdir -p "$T/slowhypr"
printf '#!/bin/sh
[ -s "$T/run/frame/daemon.pid" ] && echo pid >>"$T/hypr.log"
[ "$1" = binds ] && { /usr/bin/sleep 1; echo "[]"; }
exit 0
' >"$T/slowhypr/hyprctl"
chmod +x "$T/slowhypr/hyprctl"
t_daemon_term_in_bind() {
    : >"$T/sleep.pids"; rm -f "$T/hypr.log"
    FRAME_DRY_RUN=0 XDG_CURRENT_DESKTOP=Hyprland PATH="$T/slowhypr:$T/fakesleep:$PATH" "$FRAME" daemon >"$T/daemon.out" 2>&1 &
    d=$!
    for _ in $(seq 30); do [ -s "$T/hypr.log" ] && break; sleep 0.1; done
    [ -s "$T/hypr.log" ] || { kill -KILL "$d"; echo "no daemon.pid during bind"; cat "$T/daemon.out"; return 1; }
    kill -TERM "$d"
    for _ in $(seq 100); do kill -0 "$d" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$d" 2>/dev/null && { kill -KILL "$d"; echo "TERM during bind lost"; return 1; }
    [ ! -s "$T/sleep.pids" ] || { echo "idle loop started after TERM"; return 1; }
    [ ! -e "$T/run/frame/daemon.pid" ] && grep -q 'daemon stopped' "$T/daemon.out"
}
t_daemon_lock_busy() {   # another daemon holds daemon.lock: exit 3
    mkdir -p "$T/run/frame"
    out=$(flock -n "$T/run/frame/daemon.lock" "$FRAME" daemon 2>&1); rc=$?
    [ $rc = 3 ] && grep -q 'daemon already running' <<<"$out" || { echo "rc=$rc $out"; return 1; }
}
export -f t_daemon_term_in_bind t_daemon_lock_busy
check "daemon: pid + TERM trap before bind" t_daemon_term_in_bind
check "daemon: lock busy exits 3" t_daemon_lock_busy
rm -f "$T/state/frame/binds"
check "rundir: refuses planted /tmp dir" "mkdir -p '$T/tp' '$T/elsewhere' && ln -s '$T/elsewhere' '$T/tp/frame-$UID' && out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp' '$FRAME' record 2>&1); [ \$? -eq 1 ] && grep -q 'not a directory owned by you' <<<\"\$out\""
check "rundir: fallback is 700" "out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp2' bash -c 'mkdir -p \$TMPDIR; '\"'$FRAME'\"' record') && [ \"\$(stat -c %a '$T/tp2/frame-$UID')\" = 700 ]"

# ---- binds
conf 'replay_seconds = 20' 'bind.window ='
mkdir -p "$T/state/frame"; echo 'kde window Meta+Alt+W' >"$T/state/frame/binds"
check "daemon: kde binds + replay" "out=\$('$FRAME' daemon) && grep -q 'setShortcut .\[\"frame-region.desktop\",\"_launch\",\"frame region\",\"frame region\"\]. .\[402653266\]. 6' <<<\"\$out\" && grep -q 'rm -f .*frame-window.desktop' <<<\"\$out\" && grep -q 'save $T/state/frame/binds: kde region Meta+Alt+R monitor' <<<\"\$out\" && grep -q -- '-r 20 -replay-storage ram -o $T/docs/frame/videos' <<<\"\$out\" && [ ! -e '$T/data/applications/frame-region.desktop' ]"
printf '%s\n' 'kde window Meta+Alt+W' 'kde all Meta+Alt+A' 'hyprland region Meta+Shift+R' >"$T/state/frame/binds"
check "unbind kde from state" "out=\$('$FRAME' unbind) && [ \$(grep -c 'KGlobalAccel.unregister' <<<\"\$out\") = 2 ] && grep -q 'frame-all.desktop' <<<\"\$out\" && ! grep -q 'frame-region' <<<\"\$out\""
check "unbind hyprland from state" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$FRAME' unbind) && grep -qF \"hyprctl keyword unbind 'SUPER SHIFT, r'\" <<<\"\$out\" && ! grep -q 'ALT, r' <<<\"\$out\""
conf
check "hyprland bind"     "XDG_CURRENT_DESKTOP=Hyprland '$FRAME' bind | grep -qF \"hyprctl keyword bind 'SUPER ALT, r, exec, $FRAME region'\""
check "hyprland skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$FRAME' bind 2>&1) && grep -q 'Meta+Alt+A is already bound in Hyprland, skipped (all)' <<<\"\$out\" && ! grep -q 'ALT, a, exec' <<<\"\$out\" && grep -q 'save .*: hyprland region .* replay Meta+Alt+C record' <<<\"\$out\" && ! grep -q ' all Meta' <<<\"\$out\""
check "sway bind (one string)" "XDG_CURRENT_DESKTOP=sway '$FRAME' bind 2>/dev/null | grep -qF \"swaymsg 'bindsym Mod4+Mod1+r exec $FRAME region'\""
check "sway skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=sway '$FRAME' bind 2>&1) && grep -q 'Meta+Alt+V is already bound in sway, skipped (record)' <<<\"\$out\" && ! grep -q 'Mod1+v exec' <<<\"\$out\""
check "gnome bind"        "out=\$(XDG_CURRENT_DESKTOP=GNOME '$FRAME' bind) && grep -qF \"binding ''\\\\''<Super><Alt>m'\\\\'''\" <<<\"\$out\" && grep -qF \"custom-keybindings '['\\\\''/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/'\\\\''\" <<<\"\$out\""
t_gnome_fail() {   # $1 = fail | garbage; real run against fake gsettings
    for c in bind unbind; do
        : >"$T/gsettings.log"
        out=$(FRAME_DRY_RUN=0 FAKE_GSETTINGS=$1 XDG_CURRENT_DESKTOP=GNOME "$FRAME" "$c" 2>&1) && return 1
        grep -q 'cannot read GNOME custom shortcuts' <<<"$out" || return 1
        grep -qv '^get ' "$T/gsettings.log" && { echo "gsettings written:"; cat "$T/gsettings.log"; return 1; }
    done
    return 0
}
export -f t_gnome_fail
check "gnome: get fails -> no set" "t_gnome_fail fail"
check "gnome: garbage -> no set" "t_gnome_fail garbage"
check "service on (dry)"  "'$FRAME' service on | grep -q 'enable --now frame.service' && [ ! -e '$T/cfg/systemd/user/frame.service' ]"
# Real service on/off against a fake systemctl: the unit doesn't restart
# a daemon that lost the lock (exit 3).
mkdir -p "$T/fakesd"
printf '#!/bin/sh
echo "$*" >>"$T/systemctl.log"
exit 0
' >"$T/fakesd/systemctl"
chmod +x "$T/fakesd/systemctl"
t_service_unit() {
    local u="$T/cfg/systemd/user/frame.service"
    rm -f "$T/systemctl.log"
    FRAME_DRY_RUN=0 PATH="$T/fakesd:$PATH" "$FRAME" service on || return 1
    grep -qx 'RestartPreventExitStatus=3' "$u" && grep -qx 'Restart=on-failure' "$u" || { cat "$u"; return 1; }
    grep -qx -- '--user enable --now frame.service' "$T/systemctl.log" || return 1
    FRAME_DRY_RUN=0 PATH="$T/fakesd:$PATH" "$FRAME" service off && [ ! -e "$u" ]
}
t_service_tray() {   # a live tray.pid (the tray runs its own daemon): refused
    local s out rc
    mkdir -p "$T/run/frame"
    sleep 30 & s=$!
    bash -c "source '$FRAME'; write_pid '$T/run/frame/tray.pid' $s"
    out=$(FRAME_DRY_RUN=0 PATH="$T/fakesd:$PATH" "$FRAME" service on 2>&1); rc=$?
    kill "$s"; rm -f "$T/run/frame/tray.pid"
    [ $rc = 1 ] && grep -q "Frame tray (frame-gui, pid $s) already runs the daemon" <<<"$out" \
        && [ ! -e "$T/cfg/systemd/user/frame.service" ] || { echo "rc=$rc $out"; return 1; }
}
export -f t_service_unit t_service_tray
check "service on: unit has RestartPreventExitStatus=3" t_service_unit
check "service on: refused while the tray runs" t_service_tray

# ---- a frame path with a space (and other shell/desktop-entry specials)
mkdir -p "$T/my dir"; cp "$FRAME" "$T/my dir/frame"
S="$T/my dir/frame"; export S
check "spaces: kde Exec=" "'$S' bind | grep -qF 'Exec=\"$S\" region'"
check "spaces: systemd ExecStart=" "'$S' service on | grep -qF 'ExecStart=\"$S\" daemon'"
check "spaces: hyprland exec" "XDG_CURRENT_DESKTOP=Hyprland '$S' bind | grep -qF \"exec, '\\''$S'\\'' region\""
check "spaces: sway one string" "XDG_CURRENT_DESKTOP=sway '$S' bind 2>/dev/null | grep -qF \"+ swaymsg 'bindsym Mod4+Mod1+r exec '\\''$S'\\'' region'\""
t_gnome_spaces() {   # command must be the GVariant string of: '<path>' region
    out=$(XDG_CURRENT_DESKTOP=GNOME "$S" bind) || return 1
    want="+ gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/frame-region/ command $(source "$FRAME"; quote "$(gv_str "$(quote "$S") region")")"
    grep -qxF "$want" <<<"$out" && [ "$(source "$FRAME"; gv_str "'$S' region")" = "'\\'$S\\' region'" ]
}
export -f t_gnome_spaces
check "spaces: gnome command" t_gnome_spaces
check "spaces: hook script" "$(lib "RUNDIR='$T/hk'; FRAME_BIN='$S'; write_hook") && sh -n '$T/hk/hook' && grep -qF \"exec '$S' _saved\" '$T/hk/hook'"
t_quoting() {
    source "$FRAME"
    local p='/a b/$x%"q\'
    [ "$(desktop_arg "$p")" = '"/a b/\\$x%%\\"q\\\\"' ] || { echo "desktop: $(desktop_arg "$p")"; return 1; }
    [ "$(systemd_arg "$p")" = '"/a b/$x%%\"q\\"' ] || { echo "systemd: $(systemd_arg "$p")"; return 1; }
    [ "$(gv_str "it's \\")" = "'it\\'s \\\\'" ] || { echo "gvariant: $(gv_str "it's \\")"; return 1; }
}
export -f t_quoting
check "quoting helpers" t_quoting
if command -v desktop-file-validate >/dev/null; then
    check "spaces: desktop file validates" "$(lib "printf '[Desktop Entry]\nType=Application\nName=frame\nExec=%s region\n' \"\$(desktop_arg '$S')\" >'$T/v.desktop'") && desktop-file-validate '$T/v.desktop'"
fi

check "saved hook names replay" "'$FRAME' _saved /x/Replay_1.mp4 replay | grep -Eq '^\+ mv /x/Replay_1.mp4 $T/docs/frame/videos/.*_replay.mp4\$'"
check "video clipboard uri" "'$FRAME' _saved '/x/a b.mp4' regular | grep -qF 'wl-copy --type text/uri-list file:///x/a%20b.mp4'"
t_bind_lock() {   # a second bind/unbind waits for bind.lock, then gives up
    local out
    mkdir -p "$T/run/frame"
    exec 7>"$T/run/frame/bind.lock"; flock 7
    out=$(FRAME_BIND_WAIT=1 "$FRAME" bind 2>&1 7>&-); [ $? -eq 1 ] || { echo "$out"; return 1; }
    grep -q 'another frame bind/unbind is still running' <<<"$out" || { echo "$out"; return 1; }
    out=$(FRAME_BIND_WAIT=1 "$FRAME" unbind 2>&1 7>&-); [ $? -eq 1 ] || return 1
    exec 7>&-
    "$FRAME" bind >/dev/null
}
export -f t_bind_lock
check "bind.lock: second bind waits, fails" t_bind_lock
check "bind.lock released after bind" "$(lib "with_bind_lock true && flock -n '$T/run/frame/bind.lock' true")"
check "status runs"       "'$FRAME' status | grep -q '^daemon: stopped'"
# ---- GUI (gui/, Rust): only where cargo / a release build exist
ROOT="$(dirname "$(dirname "$FRAME")")"; export ROOT
if command -v cargo >/dev/null; then
    check "gui: cargo test" "cargo test --quiet --manifest-path '$ROOT/gui/Cargo.toml'"
fi
t_install() {   # into a temp DESTDIR; uninstall removes exactly the manifest
    local d="$T/dest" n out
    rm -f "$XDG_CONFIG_HOME/autostart/frame-gui.desktop"
    DESTDIR="$d" PREFIX=/usr/local "$ROOT/install.sh" --no-build >/dev/null || return 1
    n=$(find "$d" -type f | wc -l); [ "$n" = 13 ] || { echo "installed $n files"; return 1; }
    [ ! -e "$XDG_CONFIG_HOME/autostart/frame-gui.desktop" ] || { echo "DESTDIR install wrote autostart"; return 1; }
    [ "$(wc -l <"$d/usr/local/share/frame/installed")" = 12 ] || return 1
    grep -qx 'Exec=/usr/local/bin/frame-gui --open' "$d/usr/local/share/applications/frame.desktop" || return 1
    grep -qx 'Categories=Utility;' "$d/usr/local/share/applications/frame.desktop" || return 1
    grep -qx 'X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2' "$d/usr/local/share/applications/frame.desktop" || return 1
    if command -v desktop-file-validate >/dev/null; then
        out=$(desktop-file-validate "$d/usr/local/share/applications/frame.desktop") && [ -z "$out" ] || { echo "$out"; return 1; }
    fi
    echo keep >"$d/usr/outside"; echo ../outside >>"$d/usr/local/share/frame/installed"
    out=$(DESTDIR="$d" PREFIX=/usr/local "$ROOT/install.sh" --uninstall 2>&1) || return 1
    grep -q 'skipped odd manifest line: ../outside' <<<"$out" || { echo "$out"; return 1; }
    [ "$(find "$d" -type f)" = "$d/usr/outside" ]
}
# No DESTDIR, temp PREFIX (with a space): uninstall stops our tray (a
# copy of sleep named frame-gui), runs frame service off + unbind (bin/frame
# swapped for a logger) and removes the autostart entry only if it is ours.
t_install_live() {
    local p="$T/live $1" auto="$XDG_CONFIG_HOME/autostart/frame-gui.desktop" out pid i
    rm -f "$T/uninst.log" "$auto"
    PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null || return 1
    printf '#!/bin/sh\necho "$*" >>"%s/uninst.log"\n' "$T" >"$p/bin/frame"
    cp /usr/bin/sleep "$p/bin/frame-gui"
    ( "$p/bin/frame-gui" 60 </dev/null >/dev/null 2>&1 & echo $! >"$T/fgui.pid" )
    pid=$(cat "$T/fgui.pid")
    mkdir -p "${auto%/*}"
    if [ "$1" = ours ]; then printf '[Desktop Entry]\nExec="%s"\n' "$p/bin/frame-gui" >"$auto"
    else printf '[Desktop Entry]\nExec=/opt/other/frame-gui\n' >"$auto"; fi
    out=$(PREFIX="$p" "$ROOT/install.sh" --uninstall 2>&1) || { echo "$out"; return 1; }
    for i in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; echo "tray not stopped"; return 1; }
    grep -q "stopping frame-gui (pid $pid)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/uninst.log")" = "$(printf 'service off\nunbind')" ] || return 1
    if [ "$1" = ours ]; then [ ! -e "$auto" ] || return 1
    else [ -e "$auto" ] && grep -q "note: kept $auto" <<<"$out" || return 1; fi
    out=$(find "$p" -type f ! -name mimeinfo.cache ! -name icon-theme.cache); [ -z "$out" ] || { echo "$out"; return 1; }
}
t_install_foreign() {   # a bin/frame we didn't install is not replaced or removed
    local p="$T/fpfx" out
    mkdir -p "$p/bin"; printf '#!/bin/sh\n# not ours\n' >"$p/bin/frame"
    out=$(PREFIX="$p" "$ROOT/install.sh" --no-build 2>&1); [ $? -ne 0 ] || return 1
    grep -q "$p/bin/frame exists and was not installed by this script" <<<"$out" || { echo "$out"; return 1; }
    grep -q 'not ours' "$p/bin/frame" && [ ! -e "$p/bin/frame-gui" ] || return 1
    out=$(PREFIX="$p" "$ROOT/install.sh" --uninstall 2>&1); [ $? -ne 0 ] && grep -q 'nothing removed' <<<"$out" || return 1
    grep -q 'not ours' "$p/bin/frame" || return 1
    DESTDIR="$T/fdest" PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null || return 1   # DESTDIR is checked, not PREFIX
    PREFIX="$p" "$ROOT/install.sh" --no-build --force >/dev/null && ! grep -q 'not ours' "$p/bin/frame" \
        && grep -qx bin/frame "$p/share/frame/installed"
}
# A first real install turns on start-at-login; a reinstall keeps the
# user's choice; --no-autostart skips it. Temp PREFIX, temp XDG_CONFIG_HOME.
t_install_autostart() {
    local p="$T/apfx" auto="$XDG_CONFIG_HOME/autostart/frame-gui.desktop" out
    rm -f "$auto"
    out=$(PREFIX="$p" "$ROOT/install.sh" --no-build 2>&1) || { echo "$out"; return 1; }
    grep -q '^Frame starts when you log in' <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$auto")" = "$(printf '[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: shortcuts and replay buffer\nExec=%s\nIcon=frame\nTerminal=false\nX-GNOME-Autostart-enabled=true' "$p/bin/frame-gui")" ] || { cat "$auto"; return 1; }
    if command -v desktop-file-validate >/dev/null; then desktop-file-validate "$auto" || return 1; fi
    rm -f "$auto"   # the user turned it off; a reinstall leaves that alone
    PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null && [ ! -e "$auto" ] || { echo "reinstall re-added autostart"; return 1; }
    printf '#!/bin/sh\n' >"$p/bin/frame"   # uninstall must not run the real frame service off/unbind
    PREFIX="$p" "$ROOT/install.sh" --uninstall >/dev/null || return 1
    PREFIX="$T/apfx2" "$ROOT/install.sh" --no-build --no-autostart >/dev/null && [ ! -e "$auto" ] || return 1
    printf '#!/bin/sh\n' >"$T/apfx2/bin/frame"
    PREFIX="$T/apfx2" "$ROOT/install.sh" --uninstall >/dev/null
    out=$(PREFIX="$p" "$ROOT/install.sh" --bogus 2>&1); [ $? -ne 0 ] && grep -q 'unknown option: --bogus' <<<"$out"
}
# A real install over an old Clipped install (same temp PREFIX): stops its
# clipped-gui (a copy of sleep), runs its service off + unbind (bin/clipped
# is a logger), removes the files its manifest lists, the manifest and its
# autostart entry; keeps unlisted files, ~/.config/clipped and captures.
t_install_clipped() {
    local p="$T/oldc" auto="$XDG_CONFIG_HOME/autostart/clipped-gui.desktop" out pid f
    rm -f "$T/clipped.log"
    mkdir -p "$p/bin" "$p/share/clipped" "$p/share/applications" "${auto%/*}" "$XDG_CONFIG_HOME/clipped" "$T/docs/clipped/images"
    printf '#!/bin/sh
echo "$*" >>"%s/clipped.log"
' "$T" >"$p/bin/clipped"; chmod +x "$p/bin/clipped"
    cp /usr/bin/sleep "$p/bin/clipped-gui"
    echo '[Desktop Entry]' >"$p/share/applications/clipped.desktop"
    echo mine >"$p/share/applications/other.desktop"
    printf '%s
' bin/clipped bin/clipped-gui share/applications/clipped.desktop >"$p/share/clipped/installed"
    printf '[Desktop Entry]
Exec=%s
' "$p/bin/clipped-gui" >"$auto"
    echo 'image_format = jpg' >"$XDG_CONFIG_HOME/clipped/config"
    echo shot >"$T/docs/clipped/images/a.png"
    ( "$p/bin/clipped-gui" 60 </dev/null >/dev/null 2>&1 & echo $! >"$T/cgui.pid" )
    pid=$(cat "$T/cgui.pid")
    # DESTDIR installs (packaging) leave the old install alone.
    DESTDIR="$T/cdest" PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null || return 1
    kill -0 "$pid" 2>/dev/null && [ -e "$p/bin/clipped" ] && [ ! -e "$T/clipped.log" ] || { echo "DESTDIR touched Clipped"; return 1; }
    out=$(PREFIX="$p" "$ROOT/install.sh" --no-build --no-autostart 2>&1) || { echo "$out"; return 1; }
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; echo "clipped-gui not stopped"; return 1; }
    grep -q "stopping clipped-gui (pid $pid)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/clipped.log")" = "$(printf 'service off
unbind')" ] || { cat "$T/clipped.log"; return 1; }
    for f in bin/clipped bin/clipped-gui share/applications/clipped.desktop share/clipped/installed share/clipped; do
        [ ! -e "$p/$f" ] || { echo "left $f"; return 1; }
    done
    [ ! -e "$auto" ] && [ -e "$p/share/applications/other.desktop" ] || return 1
    [ -e "$XDG_CONFIG_HOME/clipped/config" ] && [ -e "$T/docs/clipped/images/a.png" ] || { echo "config or captures removed"; return 1; }
    [ -x "$p/bin/frame" ] && [ -x "$p/bin/frame-gui" ] && [ -f "$p/share/frame/installed" ] || return 1
    printf '#!/bin/sh
' >"$p/bin/frame"   # uninstall must not run the real frame service off/unbind
    PREFIX="$p" "$ROOT/install.sh" --uninstall >/dev/null
}
export -f t_install t_install_live t_install_foreign t_install_autostart t_install_clipped
if [ -x "$ROOT/gui/target/release/frame-gui" ]; then
    check "install.sh into DESTDIR + uninstall" t_install
    check "uninstall (live): tray, service, unbind, our autostart" "t_install_live ours"
    check "uninstall (live): keeps someone else's autostart" "t_install_live other"
    check "install refuses a foreign bin/frame" t_install_foreign
    check "install: autostart on first real install only" t_install_autostart
    check "install removes an old Clipped install" t_install_clipped
fi

check "no real notify/clipboard calls" "[ ! -e '$T/real-calls.log' ] || { cat '$T/real-calls.log'; false; }"

echo "$n checks, $([ $fail = 0 ] && echo all passed || echo FAILURES)"
exit "$fail"
