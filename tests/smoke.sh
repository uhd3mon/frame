#!/usr/bin/env bash
# Smoke tests for bin/frame. No screenshots, clipboard, notifications or
# shortcut changes: capture/bind commands run with FRAME_DRY_RUN=1 against
# a temp config and temp XDG dirs. The few real runs (KWin grab, GNOME read
# failure, installs) use fake tools put first on PATH; fake notify-send /
# wl-copy log any call so we can check that nothing real was attempted, and
# a fake systemctl keeps the install checks off the real user manager.
# Run from anywhere: bash tests/smoke.sh
set -uo pipefail

FRAME="$(cd "$(dirname "$0")/.." && pwd)/bin/frame"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/run" "$T/fake"
export FRAME_CONFIG="$T/config" XDG_RUNTIME_DIR="$T/run" XDG_DATA_HOME="$T/data" \
       XDG_CONFIG_HOME="$T/cfg" XDG_STATE_HOME="$T/state" XDG_CACHE_HOME="$T/cache" \
       TMPDIR="$T" FRAME_DRY_RUN=1 XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE \
       FRAME_GUI="$T/fake/frame-gui"
unset HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK
export PATH="$T/fake:$PATH" FRAME T
fail=0 n=0

# ---- fake tools
cat >"$T/fake/gsettings" <<'EOF'
#!/bin/sh
# FAKE_GSETTINGS: list (default) | old (has a 0.3 frame-replay) | fail | garbage. Logs every call.
echo "$*" >>"$T/gsettings.log"
[ "$1" = get ] || exit 0
P=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings
case "${FAKE_GSETTINGS:-list}" in
    fail) echo "No such schema" >&2; exit 1 ;;
    garbage) echo "No schemas installed" ;;
    old) echo "['$P/custom0/', '$P/frame-replay/', '$P/frame-all/']" ;;
    *) echo "['$P/custom0/', '$P/frame-all/']" ;;
esac
EOF
cat >"$T/fake/hyprctl" <<'EOF'
#!/bin/sh
[ "$1" = binds ] && echo '[{"modmask":72,"key":"A","dispatcher":"exec","arg":"firefox"}]'
exit 0
EOF
cat >"$T/fake/swaymsg" <<'EOF'
#!/bin/sh
[ "$2" = get_config ] && printf '%s\n' '{"config":"set $mod Mod4\nbindsym --release $mod+Mod1+a exec foo\n"}'
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
printf '#!/bin/sh\necho "$*" >>"$T/systemctl.log"\nexit 0\n' >"$T/fake/systemctl"
for t in notify-send wl-copy; do
    printf '#!/bin/sh\n[ "$1" = --help ] && { echo "  -A, --action"; exit 0; }\necho "$0 $*" >>"$T/real-calls.log"\n' >"$T/fake/$t"
done
chmod +x "$T/fake/"*

check() {
    local name="$1"; shift
    n=$((n + 1))
    if bash -c "$*" >"$T/out" 2>&1; then echo "ok   $name"
    else echo "FAIL $name"; sed 's/^/     /' "$T/out"; fail=1; fi
}
conf() { printf '%s\n' "save_dir = $T/docs" "$@" >"$FRAME_CONFIG"; }
lib() { echo "source '$FRAME'; $*"; }   # call a function from the script
# real (non-dry) runs: never clipboard or notifications
realconf() { conf 'clipboard = false' 'notify = false' "$@"; }
# cfgval KEY: the value load_config ends up with
cfgval() { lib "load_config >/dev/null; printf %s \"\${CFG[$1]}\""; }

check "syntax"            "bash -n '$FRAME'"
check "help"              "'$FRAME' --help | grep -q '^Usage: frame' && ! '$FRAME' --help | grep -Eqi 'replay|record|daemon|service|video'"
check "version 0.4.0"     "[ \"\$('$FRAME' --version)\" = 'frame 0.4.0' ]"
check "gui version 0.4.0" "grep -qx 'version = \"0.4.0\"' '$(dirname "$(dirname "$FRAME")")/gui/Cargo.toml'"
check "unknown command=2" "'$FRAME' nope; [ \$? -eq 2 ]"
for c in replay record daemon service monitors _saved; do
    check "removed command: $c = 2" "out=\$('$FRAME' $c on 2>&1); [ \$? -eq 2 ] && grep -q 'unknown command: $c' <<<\"\$out\""
done
check "config writes default" "'$FRAME' config | grep -qx '$FRAME_CONFIG' && grep -q '^bind.all = Meta+Alt+A' '$FRAME_CONFIG' && grep -q '^save_dir = ~/Documents' '$FRAME_CONFIG' && grep -qF '\$VARS are not' '$FRAME_CONFIG' && ! grep -Eq 'video|replay|record|audio' '$FRAME_CONFIG'"
check "default config parses" "'$FRAME' --detect | grep -qx 'images=$HOME/Documents/frame/images' && [ \"\$('$FRAME' check 2>&1)\" = ok ]"
check "default config keys" "[ \"\$(grep -Eo '^[a-z_.]+' '$FRAME_CONFIG' | paste -sd' ')\" = 'save_dir image_format clipboard notify bind.region bind.window bind.monitor bind.all' ]"

# ---- config parsing and validation
conf 'image_format = jpg   # comment' "\$(touch $T/pwned)" 'bind.window ='
check "config never executed" "'$FRAME' --detect >/dev/null 2>&1; [ ! -e '$T/pwned' ]"
check "config values"     "[ \"\$($(cfgval image_format) 2>/dev/null)\" = jpg ] && [ -z \"\$($(cfgval bind.window) 2>/dev/null)\" ] && '$FRAME' --detect 2>/dev/null | grep -qx 'images=$T/docs/frame/images'"
printf '%s\n' "save_dir = \"$T/my#docs\"   # real comment" "bind.region = 'Meta+Alt+R'#x" '  # indented comment' "image_format = 'webp'" >"$FRAME_CONFIG"
check "# only after space, quotes stripped" "out=\$('$FRAME' --detect 2>&1); [ \$? -eq 2 ] && grep -q \"bind.region: ''Meta+Alt+R'#x' is not a shortcut\" <<<\"\$out\""
printf '%s\n' "save_dir = \"$T/my#docs\"   # real comment" '  # indented comment' "image_format = 'webp'" >"$FRAME_CONFIG"
check "quoted values, # inside a value" "out=\$('$FRAME' --detect 2>&1) && grep -qx 'images=$T/my#docs/frame/images' <<<\"\$out\" && [ \"\$($(cfgval image_format))\" = webp ] && ! grep -q ignored <<<\"\$out\""
printf '%s\n' "save_dir = $T/d" 'image_format=jpg#no-space' >"$FRAME_CONFIG"
check "jpg#x is a value, not jpg" "'$FRAME' --detect; [ \$? -eq 2 ]"
for bad in 'image_format = gif' 'image_format = mp4' 'save_dir = docs' 'save_dir = ~kevin/x' 'save_dir = $HOME/x' \
           'notify = maybe' 'clipboard = 2' 'bind.monitor = Meta+Foo'; do
    printf '%s\n' "save_dir = $T/docs" "$bad" >"$FRAME_CONFIG"
    [[ "$bad" == save_dir* ]] && printf '%s\n' "$bad" >"$FRAME_CONFIG"
    check "rejects: $bad" "'$FRAME' --detect; [ \$? -eq 2 ]"
done
conf 'image_format = webp   # c' 'notify = off'
check "check: good config" "out=\$('$FRAME' check 2>&1) && [ \"\$out\" = ok ]"
conf 'notify = maybe'
check "check: bad config exits 2" "out=\$('$FRAME' check 2>&1); [ \$? -eq 2 ] && grep -qx 'frame: notify must be true or false' <<<\"\$out\" && ! grep -qx ok <<<\"\$out\""
# A Frame 0.3 config: its video keys (even with values 0.3 would reject)
# and replay/record shortcuts are skipped without a warning; other
# unknown keys still get one.
conf 'image_format = jpg' 'video_format = mp4' 'replay = true' 'replay_seconds = 99999' 'video_source = DP-2' \
     'video_fps = fast' 'video_quality = max' 'video_codec = h265' 'audio = default_output' \
     'bind.replay = Meta+Alt+C' 'bind.record = not a key'
check "old video keys ignored silently" "out=\$('$FRAME' check 2>&1) && [ \"\$out\" = ok ] && [ \"\$($(cfgval image_format) 2>&1)\" = jpg ]"
conf 'video_fps = 60' 'colour = red'
check "unknown key still warned" "out=\$('$FRAME' check 2>&1) && grep -qx \"frame: $FRAME_CONFIG:3: unknown key 'colour'\" <<<\"\$out\" && ! grep -q video_fps <<<\"\$out\""
t_u3000() {   # U+3000 before # is no space for bash either (load_config runs under LC_ALL=C)
    printf 'save_dir = %s\343\200\200#c\nbind.all = Meta+\342\204\252\n' "$T/docs" >"$T/u3000"
    local out; out=$(FRAME_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$FRAME"; load_config; printf %s "${CFG[save_dir]}"' 2>&1)
    [ $? -eq 2 ] && grep -q "bind.all: 'Meta+.*' is not a shortcut" <<<"$out" || { echo "$out"; return 1; }   # Kelvin sign is not k
    sed -i '$d' "$T/u3000"
    out=$(FRAME_CONFIG="$T/u3000" LC_ALL=en_US.UTF-8 bash -c 'source "$FRAME"; load_config; printf %s "${CFG[save_dir]}"') || return 1
    [ "$out" = "$(printf '%s\343\200\200#c' "$T/docs")" ] || { echo "got [$out]"; return 1; }
    [ "${LC_ALL:-unset}" = "$(FRAME_CONFIG="$T/u3000" bash -c 'source "$FRAME"; load_config >/dev/null 2>&1; echo "${LC_ALL:-unset}"')" ]   # local, not leaked
}
export -f t_u3000
check "config: U+3000 isn't space, Kelvin isn't k" t_u3000
check "check: missing config = defaults" "[ \"\$(FRAME_CONFIG='$T/none' '$FRAME' check)\" = ok ] && [ ! -e '$T/none' ]"
t_migrate() {   # one-time copy of the Clipped config, default path only
    local c="$T/mig" out
    mkdir -p "$c/clipped"; printf '# old\nimage_format = jpg\nvideo_fps = 30\n' >"$c/clipped/config"
    out=$(XDG_CONFIG_HOME="$c" FRAME_CONFIG='' "$FRAME" check 2>&1) || { echo "$out"; return 1; }
    [ "$out" = "$(printf 'frame: note: copied your Clipped config %s to %s\nok' "$c/clipped/config" "$c/frame/config")" ] || { echo "$out"; return 1; }
    cmp -s "$c/clipped/config" "$c/frame/config" || return 1
    echo 'image_format = webp' >"$c/frame/config"
    out=$(XDG_CONFIG_HOME="$c" FRAME_CONFIG='' "$FRAME" check 2>&1) && [ "$out" = ok ] || { echo "$out"; return 1; }
    grep -qx 'image_format = webp' "$c/frame/config" || return 1
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

check "detect: no video lines" "out=\$('$FRAME' --detect) && [ \"\$(cut -d= -f1 <<<\"\$out\" | paste -sd' ')\" = 'session desktop capture clipboard binds config images' ]"
check "kde wayland -> kwin" "'$FRAME' --detect | grep -qx 'capture=kwin'"
check "kde, no frame-gui -> not kwin" "FRAME_GUI='$T/none' '$FRAME' --detect | grep -Eqx 'capture=(spectacle|none)'"
check "kde x11 -> not kwin" "out=\$(XDG_SESSION_TYPE=x11 '$FRAME' --detect) && grep -q '^capture=' <<<\"\$out\" && ! grep -qx 'capture=kwin' <<<\"\$out\""
t_gui_next() {   # without FRAME_GUI: the frame-gui beside bin/frame wins
    mkdir -p "$T/nb" && cp "$T/fake/frame-gui" "$T/nb/" || return 1
    [ "$(env -u FRAME_GUI bash -c "source '$FRAME'; FRAME_BIN='$T/nb/frame'; gui_bin")" = "$T/nb/frame-gui" ]
}
export -f t_gui_next
check "frame-gui next to frame" t_gui_next
check "uri encodes UTF-8 bytes" "[ \"\$($(lib "file_uri '/h/é Д'"))\" = 'file:///h/%C3%A9%20%D0%94' ]"

# ---- screenshots + toast
mkdir -p "$T/fakeregion"; printf '#!/bin/sh\nexit 1\n' >"$T/fakeregion/slurp"; chmod +x "$T/fakeregion/slurp"  # dry run never calls it
check "region png (kde)"  "out=\$(PATH=\"$T/fakeregion:\$PATH\" '$FRAME' region 2>&1) && grep -q '^+ slurp -b #00000055' <<<\"\$out\" && grep -q '^+ $T/fake/frame-gui --grab area X Y W H $T/frame\.[A-Za-z0-9]*/shot.png\$' <<<\"\$out\" && ! grep -qE 'spectacle|crop=' <<<\"\$out\" && grep -q 'wl-copy --type image/png' <<<\"\$out\" && grep -Eq '/docs/frame/images/[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{2}-[0-9]{2}-[0-9]{2}_region.png\$' <<<\"\$out\" && [ -d '$T/docs/frame/images' ] && [ ! -e '$T/docs/frame/videos' ] && ! ls -d '$T'/frame.* 2>/dev/null"
check "toast kde: x-kde-urls + actions" "out=\$('$FRAME' all) && grep -q '^+ notify-send -a frame -i camera-photo -h string:x-kde-urls:file://$T/docs/frame/images/.*_all.png -A open=Open -A .folder=Show in folder. .Screenshot saved. ' <<<\"\$out\" && grep -qx 'Copied to clipboard.' <<<\"\$out\""
check "toast gnome: image-path" "XDG_CURRENT_DESKTOP=GNOME '$FRAME' window | grep -q 'notify-send -a frame -h string:image-path:$T/docs/frame/images/.*_window.png -A open=Open'"
conf 'image_format = webp'
check "window webp"       "out=\$('$FRAME' window) && grep -q 'frame-gui --grab window ' <<<\"\$out\" && grep -q 'libwebp' <<<\"\$out\" && grep -q 'image/webp' <<<\"\$out\""
conf 'image_format = jpg'
check "monitor jpg"       "out=\$('$FRAME' monitor) && grep -q -- '-q:v 2 .*_monitor.jpg\$' <<<\"\$out\" && grep -q 'image/jpeg' <<<\"\$out\""
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
    local out rc; rm -f "$T/gui.log"; export FRAME_DRY_RUN=0 PATH="$T/fakeslurp:$PATH"
    mkdir -p "$T/fakeslurp"
    printf '#!/bin/sh\necho "${FAKE_SLURP:-10 20 30 40}"; [ -z "$FAKE_SLURP" ]\n' >"$T/fakeslurp/slurp"
    chmod +x "$T/fakeslurp/slurp"
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
rm -rf "$T/docs"
realconf
check "kwin real mode (fake gui): save, fail, cancel" t_kwin_real
conf
if command -v grim >/dev/null; then
    check "hyprland grim"  "XDG_CURRENT_DESKTOP=Hyprland '$FRAME' window | grep -q '^+ grim -g ' && XDG_CURRENT_DESKTOP=sway '$FRAME' monitor | grep -q '^+ grim -o '"
fi
check "rundir: refuses planted /tmp dir" "mkdir -p '$T/tp' '$T/elsewhere' && ln -s '$T/elsewhere' '$T/tp/frame-$UID' && out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp' '$FRAME' bind 2>&1); [ \$? -eq 1 ] && grep -q 'not a directory owned by you' <<<\"\$out\""
check "rundir: fallback is 700" "out=\$(env -u XDG_RUNTIME_DIR TMPDIR='$T/tp2' bash -c 'mkdir -p \$TMPDIR; '\"'$FRAME'\"' bind') && [ \"\$(stat -c %a '$T/tp2/frame-$UID')\" = 700 ]"

# ---- binds
conf 'bind.window ='
mkdir -p "$T/state/frame"; echo 'kde window Meta+Alt+W' >"$T/state/frame/binds"
check "kde bind" "out=\$('$FRAME' bind) && grep -q 'setShortcut .\[\"frame-region.desktop\",\"_launch\",\"frame region\",\"frame region\"\]. .\[402653266\]. 6' <<<\"\$out\" && grep -q 'rm -f .*frame-window.desktop' <<<\"\$out\" && grep -q 'save $T/state/frame/binds: kde region Meta+Alt+R monitor Meta+Alt+M all Meta+Alt+A\$' <<<\"\$out\" && ! grep -Eq 'write .*frame-re(play|cord)|setShortcut .*frame-re(play|cord).* .\[[0-9]' <<<\"\$out\" && [ ! -e '$T/data/applications/frame-region.desktop' ]"
# Installing 0.4 deletes frame-replay/record.desktop before the tray runs
# bind: KDE must still lose those actions with no state line and no file.
check "kde bind/unbind drop old replay/record without files or state" "[ ! -e '$T/data/applications/frame-replay.desktop' ] && [ ! -e '$T/data/applications/frame-record.desktop' ] && for c in bind unbind; do out=\$('$FRAME' \$c) && grep -q 'setShortcut .\[\"frame-replay.desktop\",\"_launch\",\"frame replay\",\"frame replay\"\]. .\[\]. 4' <<<\"\$out\" && grep -q 'KGlobalAccel.unregister frame-replay.desktop _launch' <<<\"\$out\" && grep -q 'KGlobalAccel.unregister frame-record.desktop _launch' <<<\"\$out\" || exit 1; done; echo 'kde window Meta+Alt+W' >'$T/state/frame/binds'"
# Shortcuts a Frame 0.3 bound for replay / record are removed by bind and unbind.
printf '%s\n' 'kde replay Meta+Alt+C' 'kde region Meta+Alt+R' >"$T/state/frame/binds"
mkdir -p "$T/data/applications"; echo '[Desktop Entry]' >"$T/data/applications/frame-record.desktop"
check "kde bind drops old replay/record" "out=\$('$FRAME' bind) && grep -q 'KGlobalAccel.unregister frame-replay.desktop _launch' <<<\"\$out\" && grep -q 'KGlobalAccel.unregister frame-record.desktop _launch' <<<\"\$out\" && grep -q 'rm -f $T/data/applications/frame-record.desktop' <<<\"\$out\" && ! grep -Eq 'write .*frame-re(play|cord)\.desktop' <<<\"\$out\""
check "kde unbind drops old replay/record" "out=\$('$FRAME' unbind) && grep -q 'unregister frame-replay.desktop' <<<\"\$out\" && grep -q 'unregister frame-record.desktop' <<<\"\$out\" && grep -q 'unregister frame-region.desktop' <<<\"\$out\""
rm -f "$T/data/applications/frame-record.desktop"
printf '%s\n' 'kde window Meta+Alt+W' 'kde all Meta+Alt+A' 'hyprland region Meta+Shift+R' 'hyprland record Meta+Alt+V' >"$T/state/frame/binds"
check "unbind kde from state" "out=\$('$FRAME' unbind) && [ \$(grep -Ec 'KGlobalAccel.unregister frame-(window|all)' <<<\"\$out\") = 2 ] && grep -q 'frame-all.desktop' <<<\"\$out\" && ! grep -q 'frame-region' <<<\"\$out\""
check "unbind hyprland from state" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$FRAME' unbind) && grep -qF \"hyprctl keyword unbind 'SUPER SHIFT, r'\" <<<\"\$out\" && grep -qF \"hyprctl keyword unbind 'SUPER ALT, v'\" <<<\"\$out\" && ! grep -q 'ALT, r' <<<\"\$out\""
conf
check "hyprland bind"     "XDG_CURRENT_DESKTOP=Hyprland '$FRAME' bind | grep -qF \"hyprctl keyword bind 'SUPER ALT, r, exec, $FRAME region'\""
check "hyprland skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=Hyprland '$FRAME' bind 2>&1) && grep -q 'Meta+Alt+A is already bound in Hyprland, skipped (all)' <<<\"\$out\" && ! grep -q 'ALT, a, exec' <<<\"\$out\" && grep -q 'save .*: hyprland region Meta+Alt+R window Meta+Alt+W monitor Meta+Alt+M\$' <<<\"\$out\""
check "sway bind (one string)" "XDG_CURRENT_DESKTOP=sway '$FRAME' bind 2>/dev/null | grep -qF \"swaymsg 'bindsym Mod4+Mod1+r exec $FRAME region'\""
check "sway skips taken combo" "out=\$(XDG_CURRENT_DESKTOP=sway '$FRAME' bind 2>&1) && grep -q 'Meta+Alt+A is already bound in sway, skipped (all)' <<<\"\$out\" && ! grep -q 'Mod1+a exec' <<<\"\$out\""
check "gnome bind"        "out=\$(XDG_CURRENT_DESKTOP=GNOME '$FRAME' bind) && grep -qF \"binding ''\\\\''<Super><Alt>m'\\\\'''\" <<<\"\$out\" && grep -qF \"custom-keybindings '['\\\\''/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/'\\\\''\" <<<\"\$out\""
check "gnome bind drops old frame-replay" "out=\$(FAKE_GSETTINGS=old XDG_CURRENT_DESKTOP=GNOME '$FRAME' bind) && grep -q 'gsettings reset .*custom-keybindings/frame-replay/ binding' <<<\"\$out\" && ! grep 'set org.gnome.settings-daemon.plugins.media-keys custom-keybindings' <<<\"\$out\" | grep -q frame-replay"
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

# ---- a frame path with a space (and other shell/desktop-entry specials)
mkdir -p "$T/my dir"; cp "$FRAME" "$T/my dir/frame"
S="$T/my dir/frame"; export S
check "spaces: kde Exec=" "'$S' bind | grep -qF 'Exec=\"$S\" region'"
check "spaces: hyprland exec" "XDG_CURRENT_DESKTOP=Hyprland '$S' bind | grep -qF \"exec, '\\''$S'\\'' region\""
check "spaces: sway one string" "XDG_CURRENT_DESKTOP=sway '$S' bind 2>/dev/null | grep -qF \"+ swaymsg 'bindsym Mod4+Mod1+r exec '\\''$S'\\'' region'\""
t_gnome_spaces() {   # command must be the GVariant string of: '<path>' region
    out=$(XDG_CURRENT_DESKTOP=GNOME "$S" bind) || return 1
    want="+ gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/frame-region/ command $(source "$FRAME"; quote "$(gv_str "$(quote "$S") region")")"
    grep -qxF "$want" <<<"$out" && [ "$(source "$FRAME"; gv_str "'$S' region")" = "'\\'$S\\' region'" ]
}
export -f t_gnome_spaces
check "spaces: gnome command" t_gnome_spaces
t_quoting() {
    source "$FRAME"
    local p='/a b/$x%"q\'
    [ "$(desktop_arg "$p")" = '"/a b/\\$x%%\\"q\\\\"' ] || { echo "desktop: $(desktop_arg "$p")"; return 1; }
    [ "$(gv_str "it's \\")" = "'it\\'s \\\\'" ] || { echo "gvariant: $(gv_str "it's \\")"; return 1; }
}
export -f t_quoting
check "quoting helpers" t_quoting
if command -v desktop-file-validate >/dev/null; then
    check "spaces: desktop file validates" "$(lib "printf '[Desktop Entry]\nType=Application\nName=frame\nExec=%s region\n' \"\$(desktop_arg '$S')\" >'$T/v.desktop'") && desktop-file-validate '$T/v.desktop'"
fi

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

# ---- status
printf '%s\n' 'kde region Meta+Alt+R' 'kde replay Meta+Alt+C' 'kde all Meta+Alt+A' 'kde record Meta+Alt+V' 'hyprland window Meta+W' >"$T/state/frame/binds"
check "status: shortcuts without old replay/record" "out=\$('$FRAME' status) && grep -qx 'shortcuts: region all' <<<\"\$out\""
rm -f "$T/state/frame/binds"
check "status: tray not running" "out=\$('$FRAME' status) && grep -qx 'tray: not running' <<<\"\$out\" && grep -qx 'images: $T/docs/frame/images' <<<\"\$out\" && ! grep -Eqi 'replay|record|daemon|service' <<<\"\$out\""
t_status_tray() {   # tray.pid = "PID STARTTIME"; a reused PID doesn't count
    local s st f out
    mkdir -p "$T/run/frame"
    /usr/bin/sleep 30 & s=$!
    st=$(cat "/proc/$s/stat"); st="${st##*") "}"; read -ra f <<<"$st"
    echo "$s ${f[19]}" >"$T/run/frame/tray.pid"
    out=$("$FRAME" status); grep -qx "tray: running (pid $s)" <<<"$out" || { kill "$s"; echo "$out"; return 1; }
    echo "$s 1" >"$T/run/frame/tray.pid"
    out=$("$FRAME" status); kill "$s"; rm -f "$T/run/frame/tray.pid"
    grep -qx 'tray: not running' <<<"$out" || { echo "$out"; return 1; }
}
export -f t_status_tray
check "status: live tray pid, stale one ignored" t_status_tray

# ---- GUI (gui/, Rust): only where cargo exists
ROOT="$(dirname "$(dirname "$FRAME")")"; export ROOT
if command -v cargo >/dev/null; then
    check "gui: cargo test" "cargo test --quiet --manifest-path '$ROOT/gui/Cargo.toml'"
fi
check "install.sh --help: whole header" "out=\$('$ROOT/install.sh' --help) && grep -q 'Reinstalls keep your choice.' <<<\"\$out\" && ! grep -q '^set ' <<<\"\$out\""

# ---- install.sh, run from a copy of the source tree whose "built"
# frame-gui only logs its start, so an install that starts the tray never
# starts the real one. A fake display makes that start deterministic.
SRC="$T/src"; export SRC
mkdir -p "$SRC/bin" "$SRC/gui/icons/png" "$SRC/gui/target/release"
cp "$ROOT/install.sh" "$SRC/"; cp "$FRAME" "$SRC/bin/"; cp "$ROOT/gui/icons/frame.svg" "$SRC/gui/icons/"
cp "$ROOT"/gui/icons/png/frame-[0-9]*.png "$SRC/gui/icons/png/"
printf '#!/bin/sh\necho "$0 $*" >>"%s/tray-start.log"\n' "$T" >"$SRC/gui/target/release/frame-gui"
chmod +x "$SRC/gui/target/release/frame-gui"
inst() { WAYLAND_DISPLAY="${FAKE_DISPLAY-smoke-0}" DISPLAY= "$SRC/install.sh" "$@"; }
# pidline PID: "PID STARTTIME", the pid-file format of frame and frame-gui.
pidline() { local s f; s=$(cat "/proc/$1/stat"); s="${s##*") "}"; read -ra f <<<"$s"; echo "$1 ${f[19]}"; }
# bg_sleep PROG: start PROG 60 outside this shell (so it's no zombie of
# ours once killed) and print its pid.
bg_sleep() { ( "$1" 60 </dev/null >/dev/null 2>&1 & echo $! ); }
export -f inst pidline bg_sleep
t_install() {   # into a temp DESTDIR; uninstall removes exactly the manifest
    local d="$T/dest" n out
    rm -f "$XDG_CONFIG_HOME/autostart/frame-gui.desktop"
    DESTDIR="$d" PREFIX=/usr/local inst --no-build >/dev/null || return 1
    n=$(find "$d" -type f | wc -l); [ "$n" = 13 ] || { echo "installed $n files"; return 1; }
    [ ! -e "$XDG_CONFIG_HOME/autostart/frame-gui.desktop" ] || { echo "DESTDIR install wrote autostart"; return 1; }
    [ "$(wc -l <"$d/usr/local/share/frame/installed")" = 12 ] || return 1
    grep -qx 'Exec=/usr/local/bin/frame-gui --open' "$d/usr/local/share/applications/frame.desktop" || return 1
    grep -qx 'Categories=Utility;' "$d/usr/local/share/applications/frame.desktop" || return 1
    grep -qx 'X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2' "$d/usr/local/share/applications/frame.desktop" || return 1
    ! grep -Eqi 'record|replay|video' "$d/usr/local/share/applications/frame.desktop" || { echo "video words in frame.desktop"; return 1; }
    if command -v desktop-file-validate >/dev/null; then
        out=$(desktop-file-validate "$d/usr/local/share/applications/frame.desktop") && [ -z "$out" ] || { echo "$out"; return 1; }
    fi
    echo keep >"$d/usr/outside"; echo ../outside >>"$d/usr/local/share/frame/installed"
    out=$(DESTDIR="$d" PREFIX=/usr/local inst --uninstall 2>&1) || return 1
    grep -q 'skipped odd manifest line: ../outside' <<<"$out" || { echo "$out"; return 1; }
    [ "$(find "$d" -type f)" = "$d/usr/outside" ]
}
# No DESTDIR, temp PREFIX (with a space): uninstall stops our tray (a
# copy of sleep named frame-gui), runs frame unbind (bin/frame swapped for
# a logger) and removes the autostart entry only if it is ours.
t_install_live() {
    local p="$T/live $1" auto="$XDG_CONFIG_HOME/autostart/frame-gui.desktop" out pid i
    rm -f "$T/uninst.log" "$auto"
    PREFIX="$p" inst --no-build >/dev/null || return 1
    printf '#!/bin/sh\necho "$*" >>"%s/uninst.log"\n' "$T" >"$p/bin/frame"
    cp /usr/bin/sleep "$p/bin/frame-gui"
    ( "$p/bin/frame-gui" 60 </dev/null >/dev/null 2>&1 & echo $! >"$T/fgui.pid" )
    pid=$(cat "$T/fgui.pid")
    mkdir -p "${auto%/*}"
    if [ "$1" = ours ]; then printf '[Desktop Entry]\nExec="%s"\n' "$p/bin/frame-gui" >"$auto"
    else printf '[Desktop Entry]\nExec=/opt/other/frame-gui\n' >"$auto"; fi
    out=$(PREFIX="$p" inst --uninstall 2>&1) || { echo "$out"; return 1; }
    for i in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; echo "tray not stopped"; return 1; }
    grep -q "stopping frame-gui (pid $pid)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/uninst.log")" = unbind ] || { cat "$T/uninst.log"; return 1; }
    if [ "$1" = ours ]; then [ ! -e "$auto" ] || return 1
    else [ -e "$auto" ] && grep -q "note: kept $auto" <<<"$out" || return 1; fi
    out=$(find "$p" -type f ! -name mimeinfo.cache ! -name icon-theme.cache); [ -z "$out" ] || { echo "$out"; return 1; }
}
# A live install over a running Frame 0.3: its tray (a copy of sleep whose
# binary was since replaced, so /proc/PID/exe ends in " (deleted)"), its
# `frame daemon` and recorder (plain sleeps named by daemon.pid /
# record.pid) are stopped before the copy; a stale replay.pid (wrong start
# time) is left alone. The new tray (the logging fake) is started after,
# and again on a reinstall with our autostart on, but not when that entry
# is turned off or there is no display. Our old autostart Comment is updated.
t_install_restart() {
    local p="$T/rpfx" run="$T/run/frame" auto="$XDG_CONFIG_HOME/autostart/frame-gui.desktop" out g d r s
    mkdir -p "$run"; rm -f "$T/tray-start.log" "$auto"
    PREFIX="$p" inst --no-build --no-autostart >/dev/null || return 1
    [ ! -e "$T/tray-start.log" ] || { echo "started a tray with nothing running"; return 1; }
    rm "$p/bin/frame-gui"; cp /usr/bin/sleep "$p/bin/frame-gui"; g=$(bg_sleep "$p/bin/frame-gui")
    rm "$p/bin/frame-gui"; cp /usr/bin/sleep "$p/bin/frame-gui"
    [[ "$(readlink "/proc/$g/exe")" == *" (deleted)" ]] || { echo "setup: exe not deleted"; kill "$g"; return 1; }
    # The recorder stands in for gpu-screen-recorder: it ends on INT (a
    # sleep started in the background ignores INT, like any & job of sh).
    printf '#!/usr/bin/env python3\nimport signal, sys, time\nsignal.signal(signal.SIGINT, lambda *a: sys.exit(0))\ntime.sleep(int(sys.argv[1]))\n' >"$T/fake-gsr"
    chmod +x "$T/fake-gsr"
    d=$(bg_sleep /usr/bin/sleep); r=$(bg_sleep "$T/fake-gsr"); s=$(bg_sleep /usr/bin/sleep)
    sleep 0.3   # let python set its INT handler
    pidline "$g" >"$run/tray.pid"; pidline "$d" >"$run/daemon.pid"
    { pidline "$r"; echo "$T/rec.mp4"; echo gsr; } >"$run/record.pid"; echo "$s 1" >"$run/replay.pid"
    out=$(PREFIX="$p" inst --no-build --no-autostart 2>&1); local rc=$?
    local alive=""; for x in "$g" "$d" "$r"; do kill -0 "$x" 2>/dev/null && { alive+=" $x"; kill "$x"; }; done
    kill -0 "$s" 2>/dev/null || { echo "stale replay.pid pid $s was killed"; return 1; }
    kill "$s"
    [ $rc = 0 ] && [ -z "$alive" ] || { echo "rc=$rc still running:$alive"; echo "$out"; return 1; }
    grep -q "stopping frame-gui (pid $g)" <<<"$out" && grep -q "stopping Frame 0.3's daemon.pid process (pid $d)" <<<"$out" \
        && grep -q "stopping Frame 0.3's record.pid process (pid $r)" <<<"$out" && ! grep -q replay.pid <<<"$out" \
        && grep -qx "started the Frame tray ($p/bin/frame-gui)" <<<"$out" || { echo "$out"; return 1; }
    [ ! -e "$run/daemon.pid" ] && [ ! -e "$run/record.pid" ] && [ -e "$run/replay.pid" ] || { ls "$run"; return 1; }
    sleep 0.2; [ "$(cat "$T/tray-start.log")" = "$p/bin/frame-gui " ] || { cat "$T/tray-start.log"; return 1; }
    rm -f "$run/tray.pid" "$run/replay.pid" "$T/tray-start.log"
    # No tray running, but our autostart is on (a 0.3 entry): started, Comment updated.
    printf '[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: replay buffer and shortcuts\nExec=%s\nX-GNOME-Autostart-enabled=true\n' "$p/bin/frame-gui" >"$auto"
    out=$(PREFIX="$p" inst --no-build 2>&1) || { echo "$out"; return 1; }
    grep -qx "updated $auto" <<<"$out" && grep -qx "started the Frame tray ($p/bin/frame-gui)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(sed -n 4p "$auto")" = 'Comment=Frame tray: screenshot shortcuts' ] && grep -qx "Exec=$p/bin/frame-gui" "$auto" || { cat "$auto"; return 1; }
    out=$(PREFIX="$p" FAKE_DISPLAY= inst --no-build 2>&1) && ! grep -q updated <<<"$out" \
        && grep -q '^note: no desktop session here' <<<"$out" || { echo "$out"; return 1; }
    sed -i 's/enabled=true/enabled=false/' "$auto"
    out=$(PREFIX="$p" inst --no-build 2>&1) && ! grep -q started <<<"$out" || { echo "$out"; return 1; }
    sleep 0.2; [ "$(wc -l <"$T/tray-start.log")" = 1 ] || { cat "$T/tray-start.log"; return 1; }
    printf '#!/bin/sh\n' >"$p/bin/frame"   # uninstall must not run the real frame unbind
    PREFIX="$p" inst --uninstall >/dev/null
}
t_install_foreign() {   # a bin/frame we didn't install is not replaced or removed
    local p="$T/fpfx" out
    mkdir -p "$p/bin"; printf '#!/bin/sh\n# not ours\n' >"$p/bin/frame"
    out=$(PREFIX="$p" inst --no-build 2>&1); [ $? -ne 0 ] || return 1
    grep -q "$p/bin/frame exists and was not installed by this script" <<<"$out" || { echo "$out"; return 1; }
    grep -q 'not ours' "$p/bin/frame" && [ ! -e "$p/bin/frame-gui" ] || return 1
    out=$(PREFIX="$p" inst --uninstall 2>&1); [ $? -ne 0 ] && grep -q 'nothing removed' <<<"$out" || return 1
    grep -q 'not ours' "$p/bin/frame" || return 1
    DESTDIR="$T/fdest" PREFIX="$p" inst --no-build >/dev/null || return 1   # DESTDIR is checked, not PREFIX
    PREFIX="$p" inst --no-build --force >/dev/null && ! grep -q 'not ours' "$p/bin/frame" \
        && grep -qx bin/frame "$p/share/frame/installed"
}
# A first real install turns on start-at-login; a reinstall keeps the
# user's choice; --no-autostart skips it. Temp PREFIX, temp XDG_CONFIG_HOME.
t_install_autostart() {
    local p="$T/apfx" auto="$XDG_CONFIG_HOME/autostart/frame-gui.desktop" out
    rm -f "$auto"
    out=$(PREFIX="$p" inst --no-build 2>&1) || { echo "$out"; return 1; }
    grep -q '^Frame starts when you log in' <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$auto")" = "$(printf '[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: screenshot shortcuts\nExec=%s\nIcon=frame\nTerminal=false\nX-GNOME-Autostart-enabled=true' "$p/bin/frame-gui")" ] || { cat "$auto"; return 1; }
    if command -v desktop-file-validate >/dev/null; then desktop-file-validate "$auto" || return 1; fi
    rm -f "$auto"   # the user turned it off; a reinstall leaves that alone
    PREFIX="$p" inst --no-build >/dev/null && [ ! -e "$auto" ] || { echo "reinstall re-added autostart"; return 1; }
    printf '#!/bin/sh\n' >"$p/bin/frame"   # uninstall must not run the real frame unbind
    PREFIX="$p" inst --uninstall >/dev/null || return 1
    PREFIX="$T/apfx2" inst --no-build --no-autostart >/dev/null && [ ! -e "$auto" ] || return 1
    printf '#!/bin/sh\n' >"$T/apfx2/bin/frame"
    PREFIX="$T/apfx2" inst --uninstall >/dev/null
    out=$(PREFIX="$p" inst --bogus 2>&1); [ $? -ne 0 ] && grep -q 'unknown option: --bogus' <<<"$out"
}
# A real install over Frame 0.3 (video): turns off and removes its
# frame.service unit (fake systemctl) and the frame-replay / frame-record
# shortcut entries; leaves other entries alone. DESTDIR installs don't.
t_install_video_era() {
    local p="$T/vpfx" unit="$XDG_CONFIG_HOME/systemd/user/frame.service" apps="$XDG_DATA_HOME/applications" out
    mkdir -p "${unit%/*}" "$apps"
    echo '[Service]' >"$unit"
    for a in replay record region; do echo '[Desktop Entry]' >"$apps/frame-$a.desktop"; done
    rm -f "$T/systemctl.log"
    DESTDIR="$T/vdest" PREFIX="$p" inst --no-build >/dev/null || return 1
    [ -e "$unit" ] && [ -e "$apps/frame-replay.desktop" ] && [ ! -e "$T/systemctl.log" ] || { echo "DESTDIR touched the old install"; return 1; }
    out=$(PREFIX="$p" inst --no-build --no-autostart 2>&1) || { echo "$out"; return 1; }
    grep -q "removing the old video service ($unit)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/systemctl.log")" = "$(printf -- '--user disable --now frame.service\n--user daemon-reload')" ] || { cat "$T/systemctl.log"; return 1; }
    [ ! -e "$unit" ] && [ ! -e "$apps/frame-replay.desktop" ] && [ ! -e "$apps/frame-record.desktop" ] \
        && [ -e "$apps/frame-region.desktop" ] || { echo "wrong files removed"; return 1; }
    rm -f "$T/systemctl.log" "$apps/frame-region.desktop"
    PREFIX="$p" inst --no-build --no-autostart >/dev/null && [ ! -e "$T/systemctl.log" ] || { echo "reinstall ran systemctl"; return 1; }
    printf '#!/bin/sh\n' >"$p/bin/frame"   # uninstall must not run the real frame unbind
    PREFIX="$p" inst --uninstall >/dev/null
}
# A real install over an old Clipped install (same temp PREFIX): stops its
# clipped-gui (a copy of sleep), runs its service off + unbind (bin/clipped
# is a logger), removes the files its manifest lists, the manifest and its
# autostart entry; keeps unlisted files, ~/.config/clipped and captures.
t_install_clipped() {
    local p="$T/oldc" auto="$XDG_CONFIG_HOME/autostart/clipped-gui.desktop" out pid f
    rm -f "$T/clipped.log"
    mkdir -p "$p/bin" "$p/share/clipped" "$p/share/applications" "${auto%/*}" "$XDG_CONFIG_HOME/clipped" "$T/docs/clipped/images"
    printf '#!/bin/sh\necho "$*" >>"%s/clipped.log"\n' "$T" >"$p/bin/clipped"; chmod +x "$p/bin/clipped"
    cp /usr/bin/sleep "$p/bin/clipped-gui"
    echo '[Desktop Entry]' >"$p/share/applications/clipped.desktop"
    echo mine >"$p/share/applications/other.desktop"
    printf '%s\n' bin/clipped bin/clipped-gui share/applications/clipped.desktop >"$p/share/clipped/installed"
    printf '[Desktop Entry]\nExec=%s\n' "$p/bin/clipped-gui" >"$auto"
    echo 'image_format = jpg' >"$XDG_CONFIG_HOME/clipped/config"
    echo shot >"$T/docs/clipped/images/a.png"
    ( "$p/bin/clipped-gui" 60 </dev/null >/dev/null 2>&1 & echo $! >"$T/cgui.pid" )
    pid=$(cat "$T/cgui.pid")
    # DESTDIR installs (packaging) leave the old install alone.
    DESTDIR="$T/cdest" PREFIX="$p" inst --no-build >/dev/null || return 1
    kill -0 "$pid" 2>/dev/null && [ -e "$p/bin/clipped" ] && [ ! -e "$T/clipped.log" ] || { echo "DESTDIR touched Clipped"; return 1; }
    out=$(PREFIX="$p" inst --no-build --no-autostart 2>&1) || { echo "$out"; return 1; }
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; echo "clipped-gui not stopped"; return 1; }
    grep -q "stopping clipped-gui (pid $pid)" <<<"$out" || { echo "$out"; return 1; }
    [ "$(cat "$T/clipped.log")" = "$(printf 'service off\nunbind')" ] || { cat "$T/clipped.log"; return 1; }
    for f in bin/clipped bin/clipped-gui share/applications/clipped.desktop share/clipped/installed share/clipped; do
        [ ! -e "$p/$f" ] || { echo "left $f"; return 1; }
    done
    [ ! -e "$auto" ] && [ -e "$p/share/applications/other.desktop" ] || return 1
    [ -e "$XDG_CONFIG_HOME/clipped/config" ] && [ -e "$T/docs/clipped/images/a.png" ] || { echo "config or captures removed"; return 1; }
    [ -x "$p/bin/frame" ] && [ -x "$p/bin/frame-gui" ] && [ -f "$p/share/frame/installed" ] || return 1
    printf '#!/bin/sh\n' >"$p/bin/frame"   # uninstall must not run the real frame unbind
    PREFIX="$p" inst --uninstall >/dev/null
}
export -f t_install t_install_live t_install_restart t_install_foreign t_install_autostart t_install_video_era t_install_clipped
check "install.sh into DESTDIR + uninstall" t_install
check "uninstall (live): tray, unbind, our autostart" "t_install_live ours"
check "uninstall (live): keeps someone else's autostart" "t_install_live other"
check "install stops a running 0.3 (tray, daemon, recorder), restarts the tray" t_install_restart
check "install refuses a foreign bin/frame" t_install_foreign
check "install: autostart on first real install only" t_install_autostart
check "install removes Frame 0.3's service + video shortcuts" t_install_video_era
check "install removes an old Clipped install" t_install_clipped
check "no real notify/clipboard calls" "[ ! -e '$T/real-calls.log' ] || { cat '$T/real-calls.log'; false; }"

echo "$n checks, $([ $fail = 0 ] && echo all passed || echo FAILURES)"
exit "$fail"
