#!/usr/bin/env bash
# Checks for install.sh, the thin wrapper around `frame install`. What
# install does (files, manifest, upgrades, stopping the tray, autostart)
# is tested by `cargo test` (src/install.rs).
#
# The wrapper runs from a copy of the source tree whose "built" frame only
# logs how it was called. The last check runs the real built frame (if
# there is one) into a temp PREFIX with a temp HOME and XDG dirs, no
# D-Bus, no display and a desktop with no shortcut backend, so nothing
# can touch the real session. Run from anywhere: bash tests/smoke.sh
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
export T ROOT
fail=0 n=0

check() {
    local name="$1"; shift
    n=$((n + 1))
    if bash -c "$*" >"$T/out" 2>&1; then echo "ok   $name"
    else echo "FAIL $name"; sed 's/^/     /' "$T/out"; fail=1; fi
}

SRC="$T/src"; export SRC
mkdir -p "$SRC/target/release" "$T/fakebin"
cp "$ROOT/install.sh" "$SRC/"
printf '#!/bin/sh\necho "$0 $* | PREFIX=$PREFIX DESTDIR=$DESTDIR" >>"%s/frame-runs.log"\n' "$T" >"$SRC/target/release/frame"
chmod +x "$SRC/target/release/frame"
# A cargo that only logs, so the default (build) path is checked without building.
printf '#!/bin/sh\necho "cargo $*" >>"%s/frame-runs.log"\n' "$T" >"$T/fakebin/cargo"
chmod +x "$T/fakebin/cargo"

check "install.sh syntax" "bash -n '$ROOT/install.sh'"
check "install.sh --help" "'$ROOT/install.sh' --help | grep -q 'run \`frame install\`' && ! '$ROOT/install.sh' --help | grep -q '^set -e'"

t_passthrough() {   # options and PREFIX / DESTDIR reach `frame install`
    rm -f "$T/frame-runs.log"
    PREFIX=/p DESTDIR="$T/d" "$SRC/install.sh" --no-build --force --no-autostart || return 1
    [ "$(cat "$T/frame-runs.log")" = "$SRC/target/release/frame install --force --no-autostart | PREFIX=/p DESTDIR=$T/d" ] || { cat "$T/frame-runs.log"; return 1; }
}

t_build() {   # without --no-build: cargo build --release, then install
    rm -f "$T/frame-runs.log"
    PATH="$T/fakebin:$PATH" PREFIX=/p "$SRC/install.sh" || return 1
    [ "$(cat "$T/frame-runs.log")" = "$(printf 'cargo build --release\n%s install | PREFIX=/p DESTDIR=' "$SRC/target/release/frame")" ] || { cat "$T/frame-runs.log"; return 1; }
}

t_uninstall() {   # --uninstall runs `frame uninstall` (built one, else the installed one)
    rm -f "$T/frame-runs.log"
    PREFIX=/p "$SRC/install.sh" --uninstall || return 1
    [ "$(cat "$T/frame-runs.log")" = "$SRC/target/release/frame uninstall | PREFIX=/p DESTDIR=" ] || { cat "$T/frame-runs.log"; return 1; }
    local s2="$T/src2" p="$T/ipfx"
    mkdir -p "$s2" "$p/bin"; cp "$ROOT/install.sh" "$s2/"; cp "$SRC/target/release/frame" "$p/bin/frame"
    rm -f "$T/frame-runs.log"
    PREFIX="$p" "$s2/install.sh" --uninstall || return 1
    [ "$(cat "$T/frame-runs.log")" = "$p/bin/frame uninstall | PREFIX=$p DESTDIR=" ] || { cat "$T/frame-runs.log"; return 1; }
    local out; out=$(PREFIX="$T/none" "$s2/install.sh" --uninstall 2>&1); [ $? -ne 0 ] && grep -q 'no frame binary to run' <<<"$out"
}

t_refuses() {   # unknown options and a missing build are errors, nothing runs
    rm -f "$T/frame-runs.log"
    local out s3="$T/src3"
    out=$("$SRC/install.sh" --bogus 2>&1); [ $? -ne 0 ] && grep -q 'unknown option: --bogus' <<<"$out" || { echo "$out"; return 1; }
    mkdir -p "$s3"; cp "$ROOT/install.sh" "$s3/"
    out=$("$s3/install.sh" --no-build 2>&1); [ $? -ne 0 ] && grep -q 'target/release/frame is missing' <<<"$out" || { echo "$out"; return 1; }
    [ ! -e "$T/frame-runs.log" ]
}

# The real binary, isolated: install (live mode, temp PREFIX with a
# space) then uninstall. Skipped when nothing is built.
t_real() {
    local b
    for b in "$ROOT/target/release/frame" "$ROOT/target/debug/frame"; do [ -x "$b" ] && break; done
    [ -x "$b" ] || { echo "skip: no built frame"; return 0; }
    local h="$T/home" p="$T/real pfx" out
    mkdir -p "$h" "$T/rrun"
    iso() { env -i PATH=/usr/bin:/bin HOME="$h" XDG_RUNTIME_DIR="$T/rrun" XDG_CONFIG_HOME="$h/cfg" XDG_DATA_HOME="$h/data" \
        XDG_STATE_HOME="$h/state" XDG_CACHE_HOME="$h/cache" XDG_CURRENT_DESKTOP=frame-test PREFIX="$p" "$@"; }
    out=$(iso "$b" install 2>&1) || { echo "$out"; return 1; }
    [ "$(wc -l <"$p/share/frame/installed")" = 11 ] && cmp -s "$b" "$p/bin/frame" || { echo "$out"; return 1; }
    grep -qx "Exec=\"$p/bin/frame\"" "$p/share/applications/frame.desktop" || { cat "$p/share/applications/frame.desktop"; return 1; }
    grep -qx 'X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2' "$p/share/applications/frame.desktop" || return 1
    grep -qx "Exec=\"$p/bin/frame\" tray" "$h/cfg/autostart/frame.desktop" || { echo "$out"; return 1; }
    grep -q '^note: no desktop session here' <<<"$out" || { echo "$out"; return 1; }
    if command -v desktop-file-validate >/dev/null; then
        desktop-file-validate "$p/share/applications/frame.desktop" "$h/cfg/autostart/frame.desktop" || return 1
    fi
    [ "$(iso "$p/bin/frame" --version)" = "$("$b" --version)" ] || return 1
    out=$(iso "$p/bin/frame" uninstall 2>&1) || { echo "$out"; return 1; }
    [ ! -e "$h/cfg/autostart/frame.desktop" ] && [ -z "$(find "$p" -type f ! -name mimeinfo.cache ! -name icon-theme.cache)" ] || { echo "$out"; find "$p" -type f; return 1; }
}

# Nothing else to install: on every desktop/session the built frame
# plans its screenshots, encoding and clipboard with no outside program
# (dry run, PATH = an empty dir, so any tool lookup would fail).
t_builtin() {
    local b
    for b in "$ROOT/target/release/frame" "$ROOT/target/debug/frame"; do [ -x "$b" ] && break; done
    [ -x "$b" ] || { echo "skip: no built frame"; return 0; }
    local h="$T/bhome" out d s m
    mkdir -p "$h/empty" "$T/brun"
    printf 'save_dir = %s/pics\nimage_format = webp\n' "$h" >"$h/conf"
    for d in KDE:wayland sway:wayland Hyprland:wayland GNOME:wayland other:wayland KDE:x11 GNOME:x11 other:x11; do
        s=${d#*:}; d=${d%%:*}
        for m in region window monitor all; do
            out=$(env -i PATH="$h/empty" HOME="$h" XDG_RUNTIME_DIR="$T/brun" FRAME_CONFIG="$h/conf" FRAME_DRY_RUN=1 \
                XDG_CURRENT_DESKTOP="$d" XDG_SESSION_TYPE="$s" "$b" "$m" 2>&1) || { echo "$d/$s $m: $out"; return 1; }
            grep -Eq 'slurp|grim|ffmpeg|wl-copy|xclip|maim|xdotool|gnome-screenshot|spectacle|import ' <<<"$out" && { echo "$d/$s $m: $out"; return 1; }
            grep -q '^+ save webp ' <<<"$out" && grep -q '^+ frame _clip .* image/webp [a-z0-9-]*$' <<<"$out" || { echo "$d/$s $m: $out"; return 1; }
        done
        out=$(env -i PATH="$h/empty" HOME="$h" FRAME_CONFIG="$h/conf" XDG_CURRENT_DESKTOP="$d" XDG_SESSION_TYPE="$s" "$b" --detect) || return 1
        grep -Eq '^capture=(kwin|screencopy|portal|x11)$' <<<"$out" && grep -Eq '^clipboard=(data-control|focus|x11)$' <<<"$out" || { echo "$out"; return 1; }
    done
    [ ! -e "$h/pics" ] || [ -z "$(find "$h/pics" -type f)" ]   # dry run wrote no picture
}

export -f t_passthrough t_build t_uninstall t_refuses t_real t_builtin
check "install.sh passes options and PREFIX/DESTDIR to frame install" t_passthrough
check "install.sh builds first by default" t_build
check "install.sh --uninstall runs frame uninstall" t_uninstall
check "install.sh refuses bad options and a missing build" t_refuses
check "real frame install/uninstall, isolated temp HOME" t_real
check "every desktop: capture, encode and clipboard need no other program" t_builtin

echo "$n checks, $([ $fail = 0 ] && echo all passed || echo FAILURES)"
exit "$fail"
