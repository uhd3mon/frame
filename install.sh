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
#   ./install.sh --uninstall   stop frame (service, shortcuts, tray), remove the files
#
# A real install also removes an old Clipped install from the same PREFIX
# (share/clipped/installed): stops clipped-gui, runs clipped service off
# and unbind, removes its files and login entry. Its config and captures stay.
# A first real install (no DESTDIR, nothing installed here before) turns on
# "Start Frame when I log in" (~/.config/autostart/frame-gui.desktop, the
# same entry the settings checkbox writes). Reinstalls keep your choice.
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
GenericName=Screenshot & Recording
Comment=Screenshots, replay clips and recordings with global shortcuts
Exec=$(desktop_arg "$PREFIX/bin/frame-gui") --open
Icon=frame
Terminal=false
Categories=Utility;
Keywords=screenshot;screen;capture;record;recording;replay;video;
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

# stop_gui NAME: TERM every process running our bin/NAME (frame-gui or the
# old clipped-gui; tray or settings window) and wait up to 12 s: the tray
# gives its daemon 10 s to stop. Matched by /proc/PID/exe, so other
# programs are never touched.
stop_gui() {
    local name="$1" exe p pids=() left i
    [[ -e "$ROOT/bin/$name" ]] || return 0
    exe="$(readlink -f "$ROOT/bin/$name")"
    for p in /proc/[0-9]*; do
        if [[ "$(readlink "$p/exe" 2>/dev/null)" == "$exe" ]]; then pids+=("${p#/proc/}"); fi
    done
    ((${#pids[@]})) || return 0
    echo "stopping $name (pid ${pids[*]})"
    kill -TERM "${pids[@]}" 2>/dev/null || true
    for ((i = 0; i < 120; i++)); do
        left=()
        for p in "${pids[@]}"; do if kill -0 "$p" 2>/dev/null; then left+=("$p"); fi; done
        ((${#left[@]})) || return 0
        sleep 0.1
    done
    warn "$name still running (pid ${left[*]}); not killed"
}

AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/frame-gui.desktop"

# Same text as autostart_entry() in gui/src/settings.rs.
autostart_entry() {
    printf '[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: shortcuts and replay buffer\nExec=%s\nIcon=frame\nTerminal=false\nX-GNOME-Autostart-enabled=true\n' \
        "$(desktop_arg "$PREFIX/bin/frame-gui")"
}

add_autostart() {
    [[ -e "$AUTOSTART" || -L "$AUTOSTART" ]] && return 0
    mkdir -p "${AUTOSTART%/*}"
    autostart_entry >"$AUTOSTART"
    echo "Frame starts when you log in ($AUTOSTART; turn off in Settings)"
}

# remove_autostart FILE NAME: the login entry FILE, if it starts our bin/NAME.
remove_autostart() {
    local auto="$1" name="$2" e
    [[ -f "$auto" ]] || return 0
    for e in "$PREFIX/bin/$name" "$(readlink -f "$ROOT/bin/$name" 2>/dev/null)"; do
        if [[ -n "$e" ]] && grep -qxF "Exec=$(desktop_arg "$e")" "$auto"; then
            rm -f "$auto"; echo "removed $auto"; return 0
        fi
    done
    echo "note: kept $auto (it starts a different $name)"
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

uninstall() {
    [[ -f "$ROOT/$MANIFEST" ]] || die "$ROOT/$MANIFEST not found: nothing installed by this script there, nothing removed"
    if [[ -z "$DESTDIR" ]]; then  # a live install: stop what it runs first
        stop_gui frame-gui
        if ours bin/frame && [[ -x "$ROOT/bin/frame" ]]; then
            "$ROOT/bin/frame" service off || warn "frame service off failed (continuing)"
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
    remove_clipped
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
    if [[ -z "$DESTDIR" ]] && ((autostart && first)); then add_autostart; fi
    case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "note: $PREFIX/bin is not on PATH" ;; esac
}

for a in "$@"; do
    case "$a" in
        --uninstall|--no-build|--force|--no-autostart) ;;
        -h|--help) sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $a (see --help)" ;;
    esac
done
if [[ " $* " == *" --uninstall "* ]]; then uninstall; else install_all "$@"; fi
