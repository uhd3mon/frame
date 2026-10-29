#!/usr/bin/env bash
# Install frame and frame-gui under $PREFIX (default ~/.local):
#   bin/frame, bin/frame-gui, share/applications/frame.desktop,
#   share/icons/hicolor/{scalable,NxN}/apps/frame.{svg,png}
# and a list of them in share/frame/installed (uninstall removes only what
# that list names). DESTDIR (optional) is put in front of every path, for
# packaging and tests; the files still refer to $PREFIX.
#
#   ./install.sh               build frame-gui (cargo, release) and install
#   ./install.sh --no-build    install the frame-gui already in gui/target/release
#   ./install.sh --force       also replace a bin/frame or bin/frame-gui we didn't install
#   ./install.sh --no-autostart   don't make Frame start at login
#   ./install.sh --uninstall   stop frame (shortcuts, tray), remove the files
#
# A real install also removes an old Clipped install from the same PREFIX
# (share/clipped/installed): stops clipped-gui, runs clipped service off
# and unbind, removes its files and login entry. Its config and captures stay.
# It also cleans up after Frame < 0.4 (video): turns off and removes
# frame.service and the frame-replay / frame-record shortcut entries.
# A first real install (no DESTDIR, nothing installed here before) turns on
# "Start Frame when I log in" (~/.config/autostart/frame-gui.desktop, the
# same entry the settings checkbox writes). Reinstalls keep your choice.
# A real install stops a running Frame (any version) before replacing its
# files, then starts the new tray if one was running or Frame starts at login.
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"
PREFIX="${PREFIX:-$HOME/.local}"
PREFIX="${PREFIX%/}"
DESTDIR="${DESTDIR:-}"
ROOT="$DESTDIR$PREFIX"
MANIFEST=share/frame/installed
SIZES=(16 22 24 32 48 64 128 256)

die() { echo "install.sh: $*" >&2; exit 1; }
warn() { echo "install.sh: $*" >&2; }
[[ "$PREFIX" == /* ]] || die "PREFIX must be absolute (got '$PREFIX')"

files() {  # every installed path, relative to $ROOT
    echo bin/frame
    echo bin/frame-gui
    echo share/applications/frame.desktop
    echo share/icons/hicolor/scalable/apps/frame.svg
    local s; for s in "${SIZES[@]}"; do echo "share/icons/hicolor/${s}x$s/apps/frame.png"; done
}

ours() { [[ -f "$ROOT/$MANIFEST" ]] && grep -qxF "$1" "$ROOT/$MANIFEST"; }

# Exec= argument: desktop-entry escaping (same rules as desktop_arg in bin/frame).
desktop_arg() {
    [[ "$1" =~ ^[A-Za-z0-9/._+-]+$ ]] && { echo "$1"; return; }
    local s="${1//\\/"\\\\"}"
    s="${s//\"/"\\\""}"; s="${s//\`/"\\\`"}"; s="${s//\$/"\\\$"}"
    s="\"$s\""; s="${s//\\/"\\\\"}"
    echo "${s//%/"%%"}"
}

# KWin lets frame-gui (--grab) use its ScreenShot2 API only because this
# entry names it: Exec's first word must be the absolute final path (PREFIX,
# never DESTDIR) of the running binary.
desktop_entry() {
    cat <<EOF
[Desktop Entry]
Type=Application
Name=Frame
GenericName=Screenshot Tool
Comment=Screenshots with global shortcuts
Exec=$(desktop_arg "$PREFIX/bin/frame-gui") --open
Icon=frame
Terminal=false
Categories=Utility;
Keywords=screenshot;screen;capture;snip;
StartupWMClass=frame
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
EOF
}

refresh_caches() {  # only for a real install, never under DESTDIR
    [[ -z "$DESTDIR" ]] || return 0
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$ROOT/share/applications" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -q -t -f "$ROOT/share/icons/hicolor" 2>/dev/null || true
    fi
}

# wait_gone SECONDS PID...: wait until those processes are gone; prints
# the ones still running after that and fails.
wait_gone() {
    local secs="$1" p i left; shift
    for ((i = 0; i < secs * 10; i++)); do
        left=()
        for p; do if kill -0 "$p" 2>/dev/null; then left+=("$p"); fi; done
        ((${#left[@]})) || return 0
        sleep 0.1
    done
    echo "${left[*]}"; return 1
}

# stop_gui NAME: TERM every process running our bin/NAME (frame-gui or the
# old clipped-gui; tray or settings window) and wait up to 12 s (an old
# 0.3 tray gives its daemon 10 s to stop). Matched by /proc/PID/exe, also
# when that binary was replaced since it started ("PATH (deleted)"), so
# other programs are never touched.
stop_gui() {
    local name="$1" exe x p pids=() left
    if [[ -e "$ROOT/bin/$name" ]]; then exe="$(readlink -f "$ROOT/bin/$name")"
    elif [[ -d "$ROOT/bin" ]]; then exe="$(readlink -f "$ROOT/bin")/$name"
    else return 0; fi
    for p in /proc/[0-9]*; do
        x="$(readlink "$p/exe" 2>/dev/null)" || continue
        if [[ "$x" == "$exe" || "$x" == "$exe (deleted)" ]]; then pids+=("${p#/proc/}"); fi
    done
    ((${#pids[@]})) || return 0
    echo "stopping $name (pid ${pids[*]})"
    kill -TERM "${pids[@]}" 2>/dev/null || true
    left="$(wait_gone 12 "${pids[@]}")" || warn "$name still running (pid $left); not killed"
}

RUNDIR="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}/frame-$UID}/frame"

# The PID in a pid file ("PID STARTTIME", as bin/frame and frame-gui write
# them) if that same process still runs; a stale file never matches a
# reused PID.
live_pid() {
    local pid start s f
    [[ -f "$1" ]] && read -r pid start _ <"$1" || return 1
    [[ "$pid" =~ ^[0-9]+$ && -n "$start" ]] || return 1
    s="$(cat "/proc/$pid/stat" 2>/dev/null)" || return 1
    s="${s##*") "}"; read -ra f <<<"$s"
    [[ "${f[19]}" == "$start" ]] && echo "$pid"
}

# Frame 0.3 ran `frame daemon` (a bash script, so not found by stop_gui
# when it runs without the tray, from frame.service or a terminal) and
# gpu-screen-recorder for replay / record. Stop whatever their pid files
# in $RUNDIR still name: INT for the recorders (so a recording is
# finished, not cut; TERM if that doesn't stop them in 10 s), TERM for
# the daemon.
stop_video_era() {
    local f sig pid left
    for f in record.pid:INT daemon.pid:TERM replay.pid:INT; do
        sig="${f#*:}"; f="$RUNDIR/${f%:*}"
        pid="$(live_pid "$f")" || continue
        echo "stopping Frame 0.3's ${f##*/} process (pid $pid)"
        kill "-$sig" "$pid" 2>/dev/null || true
        if [[ "$sig" == INT ]] && ! wait_gone 10 "$pid" >/dev/null; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
        left="$(wait_gone 12 "$pid")" || { warn "pid $left still running; not killed"; continue; }
        rm -f "$f"
    done
}

# Before files are replaced: a running tray (any version) holds gui.lock
# and keeps the old binary running, so the new one couldn't start.
# Sets TRAY_WAS_RUNNING for start_tray.
TRAY_WAS_RUNNING=0
stop_running() {
    [[ -z "$DESTDIR" ]] || return 0
    if live_pid "$RUNDIR/tray.pid" >/dev/null; then TRAY_WAS_RUNNING=1; fi
    stop_gui frame-gui
    stop_video_era
}

AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/frame-gui.desktop"
AUTOSTART_COMMENT="Frame tray: screenshot shortcuts"

# Same text as autostart_entry() in gui/src/settings.rs.
autostart_entry() {
    printf '[Desktop Entry]\nType=Application\nName=Frame\nComment=%s\nExec=%s\nIcon=frame\nTerminal=false\nX-GNOME-Autostart-enabled=true\n' \
        "$AUTOSTART_COMMENT" "$(desktop_arg "$PREFIX/bin/frame-gui")"
}

add_autostart() {
    [[ -e "$AUTOSTART" || -L "$AUTOSTART" ]] && return 0
    mkdir -p "${AUTOSTART%/*}"
    autostart_entry >"$AUTOSTART"
    echo "Frame starts when you log in ($AUTOSTART; turn off in Settings)"
}

# autostart_ours FILE NAME: the login entry FILE starts our bin/NAME.
autostart_ours() {
    local auto="$1" name="$2" e
    [[ -f "$auto" ]] || return 1
    for e in "$PREFIX/bin/$name" "$(readlink -f "$ROOT/bin/$name" 2>/dev/null)"; do
        if [[ -n "$e" ]] && grep -qxF "Exec=$(desktop_arg "$e")" "$auto"; then return 0; fi
    done
    return 1
}

# remove_autostart FILE NAME: the login entry FILE, if it starts our bin/NAME.
remove_autostart() {
    [[ -f "$1" ]] || return 0
    if autostart_ours "$1" "$2"; then rm -f "$1"; echo "removed $1"
    else echo "note: kept $1 (it starts a different $2)"; fi
}

# Our login entry from an older Frame (0.3 said "replay buffer"): give it
# today's Comment; the rest (Exec, enabled or not) is left as it is.
update_autostart() {
    autostart_ours "$AUTOSTART" frame-gui || return 0
    grep -q '^Comment=' "$AUTOSTART" && ! grep -qxF "Comment=$AUTOSTART_COMMENT" "$AUTOSTART" || return 0
    sed "s/^Comment=.*/Comment=$AUTOSTART_COMMENT/" "$AUTOSTART" >"$AUTOSTART.tmp" && mv "$AUTOSTART.tmp" "$AUTOSTART"
    echo "updated $AUTOSTART"
}

# After a live install: start the new tray if one was running before, or
# if Frame starts at login (our entry, not turned off), so the user is on
# the new version without logging out. Detached, no output.
start_tray() {
    [[ -z "$DESTDIR" ]] || return 0
    if ((!TRAY_WAS_RUNNING)); then
        autostart_ours "$AUTOSTART" frame-gui || return 0
        grep -Eqx 'X-GNOME-Autostart-enabled=false|Hidden=true' "$AUTOSTART" && return 0
    fi
    if [[ -z "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
        echo "note: no desktop session here; the Frame tray starts when you next log in"; return 0
    fi
    if setsid -f "$ROOT/bin/frame-gui" </dev/null >/dev/null 2>&1; then
        echo "started the Frame tray ($ROOT/bin/frame-gui)"
    else warn "could not start $ROOT/bin/frame-gui; start Frame from the menu"; fi
}

# remove_listed MANIFEST: delete the files a manifest lists, then the list.
remove_listed() {
    local list="$1" f
    while IFS= read -r f; do
        # Only plain relative paths: never follow a list that points outside $ROOT.
        if [[ ! "$f" =~ ^[A-Za-z0-9][A-Za-z0-9._+/-]*$ || "/$f/" == */../* ]]; then
            warn "skipped odd manifest line: $f"; continue
        fi
        if [[ -e "$ROOT/$f" || -L "$ROOT/$f" ]]; then rm -f "$ROOT/$f"; echo "removed $ROOT/$f"; fi
    done <"$ROOT/$list"
    rm -f "$ROOT/$list"
    rmdir "$ROOT/${list%/*}" 2>/dev/null || true
}

OLD_MANIFEST=share/clipped/installed

# Frame replaces Clipped. A live install removes an old Clipped install
# from the same PREFIX first: its tray, service and shortcuts would
# otherwise fight Frame's. Its config (~/.config/clipped) and captures
# (<save_dir>/clipped) are kept; frame copies the config once on first run.
remove_clipped() {
    [[ -z "$DESTDIR" && -f "$ROOT/$OLD_MANIFEST" ]] || return 0
    echo "removing the old Clipped install ($ROOT/$OLD_MANIFEST)"
    stop_gui clipped-gui
    if grep -qxF bin/clipped "$ROOT/$OLD_MANIFEST" && [[ -x "$ROOT/bin/clipped" ]]; then
        "$ROOT/bin/clipped" service off || warn "clipped service off failed (continuing)"
        "$ROOT/bin/clipped" unbind || warn "clipped unbind failed (continuing)"
    fi
    remove_autostart "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/clipped-gui.desktop" clipped-gui
    remove_listed "$OLD_MANIFEST"
}

# Frame < 0.4 recorded video: `frame service on` wrote a systemd user unit
# that ran `frame daemon` (gone now), and KDE shortcuts for replay / record
# were .desktop entries. Live installs and uninstalls remove both; the
# tray's `frame bind` also drops those shortcuts from kglobalaccel.
remove_video_era() {
    [[ -z "$DESTDIR" ]] || return 0
    local unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/frame.service" a d
    if [[ -f "$unit" ]]; then
        echo "removing the old video service ($unit)"
        systemctl --user disable --now frame.service || warn "could not stop frame.service (continuing)"
        rm -f "$unit"
        systemctl --user daemon-reload || true
    fi
    for a in replay record; do
        d="${XDG_DATA_HOME:-$HOME/.local/share}/applications/frame-$a.desktop"
        if [[ -e "$d" || -L "$d" ]]; then rm -f "$d"; echo "removed $d"; fi
    done
}

uninstall() {
    [[ -f "$ROOT/$MANIFEST" ]] || die "$ROOT/$MANIFEST not found: nothing installed by this script there, nothing removed"
    if [[ -z "$DESTDIR" ]]; then  # a live install: stop what it runs first
        stop_running
        remove_video_era
        if ours bin/frame && [[ -x "$ROOT/bin/frame" ]]; then
            "$ROOT/bin/frame" unbind || warn "frame unbind failed (continuing)"
        fi
        remove_autostart "$AUTOSTART" frame-gui
    fi
    remove_listed "$MANIFEST"
    refresh_caches
    return 0
}

install_all() {
    local build=1 force=0 autostart=1 first=1 gui=gui/target/release/frame-gui s f a
    for a in "$@"; do
        case "$a" in --no-build) build=0 ;; --force) force=1 ;; --no-autostart) autostart=0 ;; esac
    done
    [[ -f "$ROOT/$MANIFEST" ]] && first=0
    for f in bin/frame bin/frame-gui; do
        if [[ -e "$ROOT/$f" || -L "$ROOT/$f" ]] && ! ours "$f" && ((!force)); then
            die "$ROOT/$f exists and was not installed by this script (not in $ROOT/$MANIFEST); move it away or use --force"
        fi
    done
    if ((build)); then
        command -v cargo >/dev/null || die "cargo not found (install Rust, or use --no-build)"
        cargo build --release --manifest-path gui/Cargo.toml
    fi
    [[ -x "$gui" ]] || die "$gui is missing; build it first"
    stop_running
    remove_clipped
    remove_video_era
    install -Dm755 bin/frame "$ROOT/bin/frame"
    install -Dm755 "$gui" "$ROOT/bin/frame-gui"
    install -Dm644 gui/icons/frame.svg "$ROOT/share/icons/hicolor/scalable/apps/frame.svg"
    for s in "${SIZES[@]}"; do
        install -Dm644 "gui/icons/png/frame-$s.png" "$ROOT/share/icons/hicolor/${s}x$s/apps/frame.png"
    done
    mkdir -p "$ROOT/share/applications" "$ROOT/${MANIFEST%/*}"
    desktop_entry >"$ROOT/share/applications/frame.desktop"
    if command -v desktop-file-validate >/dev/null; then
        desktop-file-validate "$ROOT/share/applications/frame.desktop"
    fi
    files >"$ROOT/$MANIFEST"
    refresh_caches
    files | sed "s|^|installed $ROOT/|"
    echo "installed $ROOT/$MANIFEST"
    if [[ -z "$DESTDIR" ]]; then
        if ((autostart && first)); then add_autostart; fi
        update_autostart
        start_tray
    fi
    case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "note: $PREFIX/bin is not on PATH" ;; esac
}

for a in "$@"; do
    case "$a" in
        --uninstall|--no-build|--force|--no-autostart) ;;
        -h|--help) sed -n '2,/^[^#]/{/^#/s/^# \{0,1\}//p}' "$0"; exit 0 ;;
        *) die "unknown option: $a (see --help)" ;;
    esac
done
if [[ " $* " == *" --uninstall "* ]]; then uninstall; else install_all "$@"; fi
