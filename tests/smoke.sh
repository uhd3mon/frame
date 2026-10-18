#!/usr/bin/env bash
# Smoke tests for bin/clip. No screenshots, recordings, clipboard,
# notifications or shortcut changes: capture/bind commands run with
# CLIP_DRY_RUN=1 against a temp config and temp XDG dirs. The few real
# runs (record stop, daemon TERM, GNOME read failure) use fake tools put
# first on PATH; fake notify-send / wl-copy log any call so we can check
# that nothing real was attempted.
# Run from anywhere: bash tests/smoke.sh
set -uo pipefail

CLIP="$(cd "$(dirname "$0")/.." && pwd)/bin/clip"
T="$(mktemp -d)"
trap 'pkill -KILL -f "$T/fake/gpu-screen-recorder" 2>/dev/null; rm -rf "$T"' EXIT
mkdir -p "$T/run" "$T/fake" "$T/fakesleep"
export CLIP_CONFIG="$T/config" XDG_RUNTIME_DIR="$T/run" XDG_DATA_HOME="$T/data" \
       XDG_CONFIG_HOME="$T/cfg" XDG_STATE_HOME="$T/state" XDG_CACHE_HOME="$T/cache" \
       TMPDIR="$T" CLIP_DRY_RUN=1 XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE
unset HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK
export PATH="$T/fake:$PATH" CLIP T
fail=0 n=0

# ---- fake tools
cat >"$T/fake/gpu-screen-recorder" <<'EOF'
#!/usr/bin/env python3
# Fake recorder: writes -o FILE on INT/TERM. FAKE_GSR=stubborn ignores both.
import os, signal, sys, time
a = sys.argv[1:]
if a and a[0] in ("--info", "--list-monitors"):
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
    *) echo "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/', '/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clip-all/']" ;;
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
conf() { printf '%s\n' "save_dir = $T/docs" "$@" >"$CLIP_CONFIG"; }
lib() { echo "source '$CLIP'; $*"; }   # call a function from the script
# real (non-dry) runs: never clipboard or notifications
realconf() { conf 'clipboard = false' 'notify = false' 'video_source = DP-9' 'video_codec = h264' 'audio = none' "$@"; }

check "syntax"            "bash -n '$CLIP'"
check "help"              "'$CLIP' --help | grep -q '^Usage: clip'"
check "version 0.2.0"     "[ \"\$('$CLIP' --version)\" = 'clip 0.2.0' ]"
check "unknown command=2" "'$CLIP' nope; [ \$? -eq 2 ]"
check "config writes default" "'$CLIP' config | grep -qx '$CLIP_CONFIG' && grep -q '^bind.record = Meta+Alt+V' '$CLIP_CONFIG' && grep -q '^save_dir = ~/Documents' '$CLIP_CONFIG' && grep -qF '\$VARS are not'  '$CLIP_CONFIG'"
check "default config parses" "'$CLIP' --detect | grep -qx 'images=$HOME/Documents/clip/images'"

# ---- config parsing and validation
conf 'image_format = jpg   # comment' 'replay_seconds = 45' "\$(touch $T/pwned)" 'bind.window ='
check "config never executed" "'$CLIP' --detect >/dev/null 2>&1; [ ! -e '$T/pwned' ]"
check "config values"     "'$CLIP' --detect 2>/dev/null | grep -qx 'replay_seconds=45' && '$CLIP' --detect 2>/dev/null | grep -qx 'videos=$T/docs/clip/videos'"
printf '%s\n' "save_dir = \"$T/my#docs\"   # real comment" "video_source = 'DP-1'#x" '  # indented comment' "video_codec = 'av1'" >"$CLIP_CONFIG"
check "# only after space, quotes stripped" "out=\$('$CLIP' --detect 2>&1) && grep -qx 'images=$T/my#docs/clip/images' <<<\"\$out\" && grep -qx \"video_source='DP-1'#x\" <<<\"\$out\" && grep -qx 'video_codec=av1' <<<\"\$out\" && ! grep -q ignored <<<\"\$out\""
printf '%s\n' "save_dir = $T/d" 'image_format=jpg#no-space' >"$CLIP_CONFIG"
check "jpg#x is a value, not jpg" "'$CLIP' --detect; [ \$? -eq 2 ]"
for bad in 'image_format = gif' 'save_dir = docs' 'save_dir = ~kevin/x' 'save_dir = $HOME/x' \
           'replay_seconds = 0' 'replay_seconds = 99999' 'video_fps = fast' 'video_quality = max' \
           'video_codec = h265' $'video_format = webm\nvideo_codec = hevc' 'notify = maybe'; do
    printf '%s\n' "save_dir = $T/docs" "$bad" >"$CLIP_CONFIG"
    [[ "$bad" == save_dir* ]] && printf '%s\n' "$bad" >"$CLIP_CONFIG"
    check "rejects: ${bad//$'\n'/; }" "'$CLIP' --detect; [ \$? -eq 2 ]"
done
conf 'image_format = webp   # c' 'video_fps = 30'
check "check: good config" "out=\$('$CLIP' check 2>&1) && [ \"\$out\" = ok ]"
conf 'video_fps = fast'
check "check: bad config exits 2" "out=\$('$CLIP' check 2>&1); [ \$? -eq 2 ] && grep -qx 'clip: video_fps must be 1-240' <<<\"\$out\" && ! grep -qx ok <<<\"\$out\""
t_u3000() {   # U+3000 before # is no space for bash either (load_config runs under LC_ALL=C)
    printf 'save_dir = %s\naudio = x\343\200\200#c\nbind.all = Meta+\342\204\252\n' "$T/docs" >"$T/u3000"
    local out; out=$(CLIP_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$CLIP"; load_config; printf %s "${CFG[audio]}"' 2>&1)
    [ $? -eq 2 ] && grep -q "bind.all: 'Meta+.*' is not a shortcut" <<<"$out" || { echo "$out"; return 1; }   # Kelvin sign is not k
    sed -i '$d' "$T/u3000"
    out=$(CLIP_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$CLIP"; load_config; printf %s "${CFG[audio]}"') || return 1
    [ "$out" = "$(printf 'x\343\200\200#c')" ] || { echo "got [$out]"; return 1; }
    [ "${LC_ALL:-unset}" = "$(CLIP_CONFIG="$T/u3000" bash -c 'source "$CLIP"; load_config >/dev/null 2>&1; echo "${LC_ALL:-unset}"')" ]   # local, not leaked
}
export -f t_u3000
check "config: U+3000 isn't space, Kelvin isn't k" t_u3000
check "check: missing config = defaults" "[ \"\$(CLIP_CONFIG='$T/none' '$CLIP' check)\" = ok ] && [ ! -e '$T/none' ]"
printf '%s\n' 'save_dir = ~/x' >"$CLIP_CONFIG"
check "save_dir ~/ expands" "'$CLIP' --detect | grep -qx 'images=$HOME/x/clip/images'"

# ---- shortcuts
conf 'bind.region = x, exec, touch '"$T"'/PWNED'
check "injection rejected (detect)" "'$CLIP' --detect; [ \$? -eq 2 ]"
check "injection rejected (hyprland bind)" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$CLIP' bind 2>&1); [ \$? -eq 2 ] && ! grep -q hyprctl <<<\"\$out\" && [ ! -e '$T/PWNED' ]"
for bad in 'Meta+Foo+R' 'Meta+Alt+' 'Meta+R+T' 'Meta+Alt' 'Meta+,' 'Meta+F36' 'Meta+é'; do
    check "shortcut rejected: $bad" "$(lib "! parse_shortcut '$bad'")"
done
check "shortcut ok: ctrl + shift + print" "$(lib "parse_shortcut ' ctrl + shift + print '")"
conf
check "keyint Meta+Alt+F12" "[ \"\$($(lib qt_keyint Meta+Alt+F12))\" = 419430459 ]"
check "keyint Meta+Alt+R" "[ \"\$($(lib qt_keyint Meta+Alt+R))\" = $((0x18000052)) ]"
check "keyint Ctrl+Shift+Print" "[ \"\$($(lib qt_keyint Ctrl+Shift+Print))\" = $((0x06000000 | 0x01000009)) ]"
check "xkb names" "[ \"\$($(lib "bind_combo sway Meta+PgUp; bind_combo hypr Alt+backspace; bind_combo gnome Ctrl+Ins"))\" = \$'Mod4+Prior\nALT, BackSpace\n<Ctrl>Insert' ]"

check "replay auto 30|60" "'$CLIP' --detect | grep -Eqx 'replay_seconds=(30|60)'"
check "kde -> spectacle"  "'$CLIP' --detect | grep -Eqx 'capture=(spectacle|none)'"
check "codec mp4 prefers av1" "[ \"\$($(lib "pick_codec mp4 \$'h264\nhevc\nav1'"))\" = av1 ]"
check "codec mp4 h264 only"   "[ \"\$($(lib "pick_codec mp4 h264"))\" = h264 ]"
check "codec webm vp9"        "[ \"\$($(lib "pick_codec webm \$'h264\nvp9'"))\" = vp9 ]"
check "uri encodes UTF-8 bytes" "[ \"\$($(lib "file_uri '/h/é Д'"))\" = 'file:///h/%C3%A9%20%D0%94' ]"

# ---- screenshots + toast
check "region png (kde)"  "out=\$('$CLIP' region) && grep -q '^+ spectacle -b -n -r -o $T/clip\.[A-Za-z0-9]*/shot.png' <<<\"\$out\" && grep -q 'wl-copy --type image/png' <<<\"\$out\" && grep -Eq '/docs/clip/images/[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{2}-[0-9]{2}-[0-9]{2}_region.png\$' <<<\"\$out\" && [ -d '$T/docs/clip/images' ] && ! ls -d '$T'/clip.* 2>/dev/null"
check "toast kde: x-kde-urls + actions" "out=\$('$CLIP' all) && grep -q '^+ notify-send -a clip -i camera-photo -h string:x-kde-urls:file://$T/docs/clip/images/.*_all.png -A open=Open -A .folder=Show in folder. .Screenshot saved. ' <<<\"\$out\" && grep -qx 'Copied to clipboard.' <<<\"\$out\""
check "toast gnome: image-path" "XDG_CURRENT_DESKTOP=GNOME '$CLIP' window | grep -q 'notify-send -a clip -h string:image-path:$T/docs/clip/images/.*_window.png -A open=Open'"
conf 'image_format = webp'
check "window webp"       "out=\$('$CLIP' window) && grep -q 'spectacle -b -n -a' <<<\"\$out\" && grep -q 'libwebp' <<<\"\$out\" && grep -q 'image/webp' <<<\"\$out\""
conf 'notify = false' 'clipboard = false'
check "notify/clipboard off" "out=\$('$CLIP' monitor) && ! grep -qE 'notify-send|wl-copy' <<<\"\$out\" && ! grep -q 'Copied' <<<\"\$out\""
conf
check "monitor/all flags" "'$CLIP' monitor | grep -q 'spectacle -b -n -m' && '$CLIP' all | grep -q 'spectacle -b -n -f'"
if command -v grim >/dev/null; then
    check "hyprland grim"  "XDG_CURRENT_DESKTOP=Hyprland '$CLIP' window | grep -q '^+ grim -g ' && XDG_CURRENT_DESKTOP=sway '$CLIP' monitor | grep -q '^+ grim -o '"
fi

# ---- video
conf 'video_source = DP-9' 'video_codec = hevc' 'video_format = mkv' 'audio = none'
check "record cmd"        "out=\$('$CLIP' record) && grep -q '^+ gpu-screen-recorder -w DP-9 -c mkv -k hevc -f 60 -q very_high -sc ' <<<\"\$out\" && ! grep -q -- ' -a ' <<<\"\$out\" && grep -Eq '_record.mkv\$' <<<\"\$out\""
check "ffmpeg webm = vp9 + opus" "out=\$($(lib 'CFG[video_format]=webm; ffmpeg_args /v.webm | paste -sd" "')) && grep -q -- '-c:v libvpx-vp9 -crf 32 -b:v 0 ' <<<\"\$out\" && grep -q -- '-c:a libopus' <<<\"\$out\" && ! grep -q x264 <<<\"\$out\""
check "ffmpeg mp4 = x264 + aac" "out=\$($(lib 'ffmpeg_args /v.mp4 | paste -sd" "')) && grep -q -- '-c:v libx264 ' <<<\"\$out\" && grep -q -- '-c:a aac' <<<\"\$out\" && grep -q ' /v.mp4\$' <<<\"\$out\""
check "toast video: thumb + titles" "out=\$('$CLIP' _saved /x/Replay_1.mp4 replay 2>&1) && grep -q 'ffmpeg -loglevel error -y -ss 1 -i $T/docs/clip/videos/.*_replay.mp4 -frames:v 1 -vf scale=512:-2 $T/cache/clip/thumbs/' <<<\"\$out\" && grep -q 'x-kde-urls:file://.*_replay.mp4 -h string:image-path:$T/cache/clip/thumbs/.*_replay.png .*Clip saved (' <<<\"\$out\" && '$CLIP' _saved /x/r.webm | grep -q \"'Recording saved'\""

t_record_lock() {
    mkdir -p "$T/run/clip"
    flock "$T/run/clip/record.lock" sleep 2 & sleep 0.3
    out=$("$CLIP" record 2>&1); rc=$?
    wait
    [ $rc -eq 1 ] && grep -q 'already starting or stopping' <<<"$out" && ! grep -q gpu-screen-recorder <<<"$out"
}
t_stale_pid() {
    mkdir -p "$T/run/clip"
    /usr/bin/sleep 30 & victim=$!
    printf '%s 1\n/x.mp4\ngsr\n' "$victim" >"$T/run/clip/record.pid"
    out=$("$CLIP" record 2>&1)
    alive=0; kill -0 "$victim" 2>/dev/null && alive=1
    kill "$victim"; rm -f "$T/run/clip/record.pid"
    [ $alive = 1 ] && ! grep -q 'kill' <<<"$out" && grep -q '^+ gpu-screen-recorder' <<<"$out"
}
t_record_real() {   # fake recorder, real start/stop logic
    out=$(CLIP_DRY_RUN=0 "$CLIP" record) || return 1
    read -r pid start <"$T/run/clip/record.pid"
    [ -n "$start" ] && kill -0 "$pid" || return 1
    f=$(CLIP_DRY_RUN=0 "$CLIP" record) || return 1
    [ "$(cat "$f")" = video ] && [ ! -e "$T/run/clip/record.pid" ] && ! kill -0 "$pid" 2>/dev/null
}
t_record_stubborn() {
    CLIP_DRY_RUN=0 FAKE_GSR=stubborn "$CLIP" record >/dev/null || return 1
    read -r pid _ <"$T/run/clip/record.pid"
    out=$(CLIP_DRY_RUN=0 CLIP_STOP_WAIT=1 "$CLIP" record 2>&1); rc=$?
    kept=0; [ -e "$T/run/clip/record.pid" ] && kept=1
    kill -KILL "$pid"; rm -f "$T/run/clip/record.pid"
    [ $rc = 1 ] && [ $kept = 1 ] && grep -q 'sending TERM' <<<"$out" && grep -q 'still running' <<<"$out"
}
t_daemon_term() {   # $1 = replay true|false
    : >"$T/sleep.pids"
    CLIP_DRY_RUN=0 XDG_CURRENT_DESKTOP=none PATH="$T/fakesleep:$PATH" "$CLIP" daemon >"$T/daemon.out" 2>&1 &
    d=$!
    for _ in $(seq 30); do [ -s "$T/run/clip/daemon.pid" ] && { [ "$1" = false ] || [ -s "$T/run/clip/replay.pid" ]; } && break; sleep 0.1; done
    [ "$1" = true ] && read -r g _ <"$T/run/clip/replay.pid"
    sleep 0.3; kill -TERM "$d"
    for _ in $(seq 30); do kill -0 "$d" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$d" 2>/dev/null && { kill -KILL "$d"; echo "daemon still running"; return 1; }
    for s in $(cat "$T/sleep.pids"); do kill -0 "$s" 2>/dev/null && { echo "sleep $s left behind"; return 1; }; done
    [ "$1" = false ] || { ! kill -0 "$g" 2>/dev/null || { echo "recorder left"; return 1; }; }
    [ ! -e "$T/run/clip/daemon.pid" ] && grep -q 'daemon stopped' "$T/daemon.out"
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
check "rundir: refuses planted /tmp dir" "mkdir -p '$T/tp' '$T/elsewhere' && ln -s '$T/elsewhere' '$T/tp/clip-$UID' && out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp' '$CLIP' record 2>&1); [ \$? -eq 1 ] && grep -q 'not a directory owned by you' <<<\"\$out\""
check "rundir: fallback is 700" "out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp2' bash -c 'mkdir -p \$TMPDIR; '\"'$CLIP'\"' record') && [ \"\$(stat -c %a '$T/tp2/clip-$UID')\" = 700 ]"

# ---- binds
conf 'replay_seconds = 20' 'bind.window ='
mkdir -p "$T/state/clip"; echo 'kde window Meta+Alt+W' >"$T/state/clip/binds"
check "daemon: kde binds + replay" "out=\$('$CLIP' daemon) && grep -q 'setShortcut .\[\"clip-region.desktop\",\"_launch\",\"clip region\",\"clip region\"\]. .\[402653266\]. 4' <<<\"\$out\" && grep -q 'rm -f .*clip-window.desktop' <<<\"\$out\" && grep -q 'save $T/state/clip/binds: kde region Meta+Alt+R monitor' <<<\"\$out\" && grep -q -- '-r 20 -replay-storage ram -o $T/docs/clip/videos' <<<\"\$out\" && [ ! -e '$T/data/applications/clip-region.desktop' ]"
printf '%s\n' 'kde window Meta+Alt+W' 'kde all Meta+Alt+A' 'hyprland region Meta+Shift+R' >"$T/state/clip/binds"
check "unbind kde from state" "out=\$('$CLIP' unbind) && [ \$(grep -c 'KGlobalAccel.unregister' <<<\"\$out\") = 2 ] && grep -q 'clip-all.desktop' <<<\"\$out\" && ! grep -q 'clip-region' <<<\"\$out\""
check "unbind hyprland from state" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$CLIP' unbind) && grep -qF \"hyprctl keyword unbind 'SUPER SHIFT, r'\" <<<\"\$out\" && ! grep -q 'ALT, r' <<<\"\$out\""
conf
check "hyprland bind"     "XDG_CURRENT_DESKTOP=Hyprland '$CLIP' bind | grep -qF \"hyprctl keyword bind 'SUPER ALT, r, exec, $CLIP region'\""
check "hyprland skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$CLIP' bind 2>&1) && grep -q 'Meta+Alt+A is already bound in Hyprland, skipped (all)' <<<\"\$out\" && ! grep -q 'ALT, a, exec' <<<\"\$out\" && grep -q 'save .*: hyprland region .* replay Meta+Alt+C record' <<<\"\$out\" && ! grep -q ' all Meta' <<<\"\$out\""
check "sway bind (one string)" "XDG_CURRENT_DESKTOP=sway '$CLIP' bind 2>/dev/null | grep -qF \"swaymsg 'bindsym Mod4+Mod1+r exec $CLIP region'\""
check "sway skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=sway '$CLIP' bind 2>&1) && grep -q 'Meta+Alt+V is already bound in sway, skipped (record)' <<<\"\$out\" && ! grep -q 'Mod1+v exec' <<<\"\$out\""
check "gnome bind"        "out=\$(XDG_CURRENT_DESKTOP=GNOME '$CLIP' bind) && grep -qF \"binding ''\\\\''<Super><Alt>m'\\\\'''\" <<<\"\$out\" && grep -qF \"custom-keybindings '['\\\\''/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/'\\\\''\" <<<\"\$out\""
t_gnome_fail() {   # $1 = fail | garbage; real run against fake gsettings
    for c in bind unbind; do
        : >"$T/gsettings.log"
        out=$(CLIP_DRY_RUN=0 FAKE_GSETTINGS=$1 XDG_CURRENT_DESKTOP=GNOME "$CLIP" "$c" 2>&1) && return 1
        grep -q 'cannot read GNOME custom shortcuts' <<<"$out" || return 1
        grep -qv '^get ' "$T/gsettings.log" && { echo "gsettings written:"; cat "$T/gsettings.log"; return 1; }
    done
    return 0
}
export -f t_gnome_fail
check "gnome: get fails -> no set" "t_gnome_fail fail"
check "gnome: garbage -> no set" "t_gnome_fail garbage"
check "service on (dry)"  "'$CLIP' service on | grep -q 'enable --now clip.service' && [ ! -e '$T/cfg/systemd/user/clip.service' ]"

# ---- a clip path with a space (and other shell/desktop-entry specials)
mkdir -p "$T/my dir"; cp "$CLIP" "$T/my dir/clip"
S="$T/my dir/clip"; export S
check "spaces: kde Exec=" "'$S' bind | grep -qF 'Exec=\"$S\" region'"
check "spaces: systemd ExecStart=" "'$S' service on | grep -qF 'ExecStart=\"$S\" daemon'"
check "spaces: hyprland exec" "XDG_CURRENT_DESKTOP=Hyprland '$S' bind | grep -qF \"exec, '\\''$S'\\'' region\""
check "spaces: sway one string" "XDG_CURRENT_DESKTOP=sway '$S' bind 2>/dev/null | grep -qF \"+ swaymsg 'bindsym Mod4+Mod1+r exec '\\''$S'\\'' region'\""
t_gnome_spaces() {   # command must be the GVariant string of: '<path>' region
    out=$(XDG_CURRENT_DESKTOP=GNOME "$S" bind) || return 1
    want="+ gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clip-region/ command $(source "$CLIP"; quote "$(gv_str "$(quote "$S") region")")"
    grep -qxF "$want" <<<"$out" && [ "$(source "$CLIP"; gv_str "'$S' region")" = "'\\'$S\\' region'" ]
}
export -f t_gnome_spaces
check "spaces: gnome command" t_gnome_spaces
check "spaces: hook script" "$(lib "RUNDIR='$T/hk'; CLIP_BIN='$S'; write_hook") && sh -n '$T/hk/hook' && grep -qF \"exec '$S' _saved\" '$T/hk/hook'"
t_quoting() {
    source "$CLIP"
    local p='/a b/$x%"q\'
    [ "$(desktop_arg "$p")" = '"/a b/\\$x%%\\"q\\\\"' ] || { echo "desktop: $(desktop_arg "$p")"; return 1; }
    [ "$(systemd_arg "$p")" = '"/a b/$x%%\"q\\"' ] || { echo "systemd: $(systemd_arg "$p")"; return 1; }
    [ "$(gv_str "it's \\")" = "'it\\'s \\\\'" ] || { echo "gvariant: $(gv_str "it's \\")"; return 1; }
}
export -f t_quoting
check "quoting helpers" t_quoting
if command -v desktop-file-validate >/dev/null; then
    check "spaces: desktop file validates" "$(lib "printf '[Desktop Entry]\nType=Application\nName=clip\nExec=%s region\n' \"\$(desktop_arg '$S')\" >'$T/v.desktop'") && desktop-file-validate '$T/v.desktop'"
fi

check "saved hook names replay" "'$CLIP' _saved /x/Replay_1.mp4 replay | grep -Eq '^\+ mv /x/Replay_1.mp4 $T/docs/clip/videos/.*_replay.mp4\$'"
check "video clipboard uri" "'$CLIP' _saved '/x/a b.mp4' regular | grep -qF 'wl-copy --type text/uri-list file:///x/a%20b.mp4'"
t_bind_lock() {   # a second bind/unbind waits for bind.lock, then gives up
    local out
    mkdir -p "$T/run/clip"
    exec 7>"$T/run/clip/bind.lock"; flock 7
    out=$(CLIP_BIND_WAIT=1 "$CLIP" bind 2>&1 7>&-); [ $? -eq 1 ] || { echo "$out"; return 1; }
    grep -q 'another clip bind/unbind is still running' <<<"$out" || { echo "$out"; return 1; }
    out=$(CLIP_BIND_WAIT=1 "$CLIP" unbind 2>&1 7>&-); [ $? -eq 1 ] || return 1
    exec 7>&-
    "$CLIP" bind >/dev/null
}
export -f t_bind_lock
check "bind.lock: second bind waits, fails" t_bind_lock
check "bind.lock released after bind" "$(lib "with_bind_lock true && flock -n '$T/run/clip/bind.lock' true")"
check "status runs"       "'$CLIP' status | grep -q '^daemon: stopped'"
# ---- GUI (gui/, Rust): only where cargo / a release build exist
ROOT="$(dirname "$(dirname "$CLIP")")"; export ROOT
if command -v cargo >/dev/null; then
    check "gui: cargo test" "cargo test --quiet --manifest-path '$ROOT/gui/Cargo.toml'"
fi
t_install() {   # into a temp DESTDIR; uninstall removes exactly the manifest
    local d="$T/dest" n out
    DESTDIR="$d" PREFIX=/usr/local "$ROOT/install.sh" --no-build >/dev/null || return 1
    n=$(find "$d" -type f | wc -l); [ "$n" = 13 ] || { echo "installed $n files"; return 1; }
    [ "$(wc -l <"$d/usr/local/share/clip/installed")" = 12 ] || return 1
    grep -qx 'Exec=/usr/local/bin/clip-gui --open' "$d/usr/local/share/applications/clip.desktop" || return 1
    grep -qx 'Categories=Utility;' "$d/usr/local/share/applications/clip.desktop" || return 1
    if command -v desktop-file-validate >/dev/null; then
        out=$(desktop-file-validate "$d/usr/local/share/applications/clip.desktop") && [ -z "$out" ] || { echo "$out"; return 1; }
    fi
    echo keep >"$d/usr/outside"; echo ../outside >>"$d/usr/local/share/clip/installed"
    out=$(DESTDIR="$d" PREFIX=/usr/local "$ROOT/install.sh" --uninstall 2>&1) || return 1
    grep -q 'skipped odd manifest line: ../outside' <<<"$out" || { echo "$out"; return 1; }
    [ "$(find "$d" -type f)" = "$d/usr/outside" ]
}
# No DESTDIR, temp PREFIX (with a space): uninstall stops our tray (a
# copy of sleep named clip-gui), runs clip service off + unbind (bin/clip
# swapped for a logger) and removes the autostart entry only if it is ours.
t_install_live() {
    local p="$T/live $1" auto="$XDG_CONFIG_HOME/autostart/clip-gui.desktop" out pid i
    rm -f "$T/uninst.log" "$auto"
    PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null || return 1
    printf '#!/bin/sh\necho "$*" >>"%s/uninst.log"\n' "$T" >"$p/bin/clip"
    cp /usr/bin/sleep "$p/bin/clip-gui"
    ( "$p/bin/clip-gui" 60 </dev/null >/dev/null 2>&1 & echo $! >"$T/fgui.pid" )
    pid=$(cat "$T/fgui.pid")
    mkdir -p "${auto%/*}"
    if [ "$1" = ours ]; then printf '[Desktop Entry]\nExec="%s"\n' "$p/bin/clip-gui" >"$auto"
    else printf '[Desktop Entry]\nExec=/opt/other/clip-gui\n' >"$auto"; fi
    out=$(PREFIX="$p" "$ROOT/install.sh" --uninstall 2>&1) || { echo "$out"; return 1; }
    for i in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; echo "tray not stopped"; return 1; }
    grep -q "stopping clip-gui (pid $pid)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/uninst.log")" = "$(printf 'service off\nunbind')" ] || return 1
    if [ "$1" = ours ]; then [ ! -e "$auto" ] || return 1
    else [ -e "$auto" ] && grep -q "note: kept $auto" <<<"$out" || return 1; fi
    out=$(find "$p" -type f ! -name mimeinfo.cache ! -name icon-theme.cache); [ -z "$out" ] || { echo "$out"; return 1; }
}
t_install_foreign() {   # a bin/clip we didn't install is not replaced or removed
    local p="$T/fpfx" out
    mkdir -p "$p/bin"; printf '#!/bin/sh\n# not ours\n' >"$p/bin/clip"
    out=$(PREFIX="$p" "$ROOT/install.sh" --no-build 2>&1); [ $? -ne 0 ] || return 1
    grep -q "$p/bin/clip exists and was not installed by this script" <<<"$out" || { echo "$out"; return 1; }
    grep -q 'not ours' "$p/bin/clip" && [ ! -e "$p/bin/clip-gui" ] || return 1
    out=$(PREFIX="$p" "$ROOT/install.sh" --uninstall 2>&1); [ $? -ne 0 ] && grep -q 'nothing removed' <<<"$out" || return 1
    grep -q 'not ours' "$p/bin/clip" || return 1
    DESTDIR="$T/fdest" PREFIX="$p" "$ROOT/install.sh" --no-build >/dev/null || return 1   # DESTDIR is checked, not PREFIX
    PREFIX="$p" "$ROOT/install.sh" --no-build --force >/dev/null && ! grep -q 'not ours' "$p/bin/clip" \
        && grep -qx bin/clip "$p/share/clip/installed"
}
export -f t_install t_install_live t_install_foreign
if [ -x "$ROOT/gui/target/release/clip-gui" ]; then
    check "install.sh into DESTDIR + uninstall" t_install
    check "uninstall (live): tray, service, unbind, our autostart" "t_install_live ours"
    check "uninstall (live): keeps someone else's autostart" "t_install_live other"
    check "install refuses a foreign bin/clip" t_install_foreign
fi

check "no real notify/clipboard calls" "[ ! -e '$T/real-calls.log' ] || { cat '$T/real-calls.log'; false; }"

echo "$n checks, $([ $fail = 0 ] && echo all passed || echo FAILURES)"
exit "$fail"
