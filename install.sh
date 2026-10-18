#!/usr/bin/env bash
# Install clipped and clipped-gui under $PREFIX (default ~/.local):
#   bin/clipped, bin/clipped-gui, share/applications/clipped.desktop,
#   share/icons/hicolor/{scalable,NxN}/apps/clipped.{svg,png}
# and a list of them in share/clipped/installed (uninstall removes only what
# that list names). DESTDIR (optional) is put in front of every path, for
# packaging and tests; the files still refer to $PREFIX.
#
#   ./install.sh               build clipped-gui (cargo, release) and install
#   ./install.sh --no-build    install the clipped-gui already in gui/target/release
#   ./install.sh --force       also replace a bin/clipped or bin/clipped-gui we didn't install
#   ./install.sh --uninstall   stop clipped (service, shortcuts, tray), remove the files
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"
PREFIX="${PREFIX:-$HOME/.local}"
PREFIX="${PREFIX%/}"
DESTDIR="${DESTDIR:-}"
ROOT="$DESTDIR$PREFIX"
MANIFEST=share/clipped/installed
SIZES=(16 22 24 32 48 64 128 256)

die() { echo "install.sh: $*" >&2; exit 1; }
warn() { echo "install.sh: $*" >&2; }
[[ "$PREFIX" == /* ]] || die "PREFIX must be absolute (got '$PREFIX')"

files() {  # every installed path, relative to $ROOT
    echo bin/clipped
    echo bin/clipped-gui
    echo share/applications/clipped.desktop
    echo share/icons/hicolor/scalable/apps/clipped.svg
    local s; for s in "${SIZES[@]}"; do echo "share/icons/hicolor/${s}x$s/apps/clipped.png"; done
}

ours() { [[ -f "$ROOT/$MANIFEST" ]] && grep -qxF "$1" "$ROOT/$MANIFEST"; }

# Exec= argument: desktop-entry escaping (same rules as desktop_arg in bin/clipped).
desktop_arg() {
    [[ "$1" =~ ^[A-Za-z0-9/._+-]+$ ]] && { echo "$1"; return; }
    local s="${1//\\/"\\\\"}"
    s="${s//\"/"\\\""}"; s="${s//\`/"\\\`"}"; s="${s//\$/"\\\$"}"
    s="\"$s\""; s="${s//\\/"\\\\"}"
    echo "${s//%/"%%"}"
}

# KWin lets clipped-gui (--grab) use its ScreenShot2 API only because this
# entry names it: Exec's first word must be the absolute final path (PREFIX,
# never DESTDIR) of the running binary.
desktop_entry() {
    cat <<EOF
[Desktop Entry]
Type=Application
Name=Clipped
GenericName=Screenshot & Recording
Comment=Screenshots, replay clips and recordings with global shortcuts
Exec=$(desktop_arg "$PREFIX/bin/clipped-gui") --open
Icon=clipped
Terminal=false
Categories=Utility;
Keywords=screenshot;screen;capture;record;recording;replay;video;
StartupWMClass=clipped
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

# TERM every process running our clipped-gui (tray or settings window), wait
# up to 3 s. Matched by /proc/PID/exe, so other programs are never touched.
stop_gui() {
    local exe p pids=() left i
    [[ -e "$ROOT/bin/clipped-gui" ]] || return 0
    exe="$(readlink -f "$ROOT/bin/clipped-gui")"
    for p in /proc/[0-9]*; do
        if [[ "$(readlink "$p/exe" 2>/dev/null)" == "$exe" ]]; then pids+=("${p#/proc/}"); fi
    done
    ((${#pids[@]})) || return 0
    echo "stopping clipped-gui (pid ${pids[*]})"
    kill -TERM "${pids[@]}" 2>/dev/null || true
    for ((i = 0; i < 30; i++)); do
        left=()
        for p in "${pids[@]}"; do if kill -0 "$p" 2>/dev/null; then left+=("$p"); fi; done
        ((${#left[@]})) || return 0
        sleep 0.1
    done
    warn "clipped-gui still running (pid ${left[*]}); not killed"
}

# The "Start tray at login" entry clipped-gui writes, if it starts our clipped-gui.
remove_autostart() {
    local auto="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/clipped-gui.desktop" e
    [[ -f "$auto" ]] || return 0
    for e in "$PREFIX/bin/clipped-gui" "$(readlink -f "$ROOT/bin/clipped-gui" 2>/dev/null)"; do
        if [[ -n "$e" ]] && grep -qxF "Exec=$(desktop_arg "$e")" "$auto"; then
            rm -f "$auto"; echo "removed $auto"; return 0
        fi
    done
    echo "note: kept $auto (it starts a different clipped-gui)"
}

uninstall() {
    local f
    [[ -f "$ROOT/$MANIFEST" ]] || die "$ROOT/$MANIFEST not found: nothing installed by this script there, nothing removed"
    if [[ -z "$DESTDIR" ]]; then  # a live install: stop what it runs first
        stop_gui
        if ours bin/clipped && [[ -x "$ROOT/bin/clipped" ]]; then
            "$ROOT/bin/clipped" service off || warn "clipped service off failed (continuing)"
            "$ROOT/bin/clipped" unbind || warn "clipped unbind failed (continuing)"
        fi
        remove_autostart
    fi
    while IFS= read -r f; do
        # Only plain relative paths: never follow a list that points outside $ROOT.
        if [[ ! "$f" =~ ^[A-Za-z0-9][A-Za-z0-9._+/-]*$ || "/$f/" == */../* ]]; then
            warn "skipped odd manifest line: $f"; continue
        fi
        if [[ -e "$ROOT/$f" || -L "$ROOT/$f" ]]; then rm -f "$ROOT/$f"; echo "removed $ROOT/$f"; fi
    done <"$ROOT/$MANIFEST"
    rm -f "$ROOT/$MANIFEST"
    rmdir "$ROOT/${MANIFEST%/*}" 2>/dev/null || true
    refresh_caches
    return 0
}

install_all() {
    local build=1 force=0 gui=gui/target/release/clipped-gui s f a
    for a in "$@"; do
        case "$a" in --no-build) build=0 ;; --force) force=1 ;; esac
    done
    for f in bin/clipped bin/clipped-gui; do
        if [[ -e "$ROOT/$f" || -L "$ROOT/$f" ]] && ! ours "$f" && ((!force)); then
            die "$ROOT/$f exists and was not installed by this script (not in $ROOT/$MANIFEST); move it away or use --force"
        fi
    done
    if ((build)); then
        command -v cargo >/dev/null || die "cargo not found (install Rust, or use --no-build)"
        cargo build --release --manifest-path gui/Cargo.toml
    fi
    [[ -x "$gui" ]] || die "$gui is missing; build it first"
    install -Dm755 bin/clipped "$ROOT/bin/clipped"
    install -Dm755 "$gui" "$ROOT/bin/clipped-gui"
    install -Dm644 gui/icons/clipped.svg "$ROOT/share/icons/hicolor/scalable/apps/clipped.svg"
    for s in "${SIZES[@]}"; do
        install -Dm644 "gui/icons/png/clipped-$s.png" "$ROOT/share/icons/hicolor/${s}x$s/apps/clipped.png"
    done
    mkdir -p "$ROOT/share/applications" "$ROOT/${MANIFEST%/*}"
    desktop_entry >"$ROOT/share/applications/clipped.desktop"
    if command -v desktop-file-validate >/dev/null; then
        desktop-file-validate "$ROOT/share/applications/clipped.desktop"
    fi
    files >"$ROOT/$MANIFEST"
    refresh_caches
    files | sed "s|^|installed $ROOT/|"
    echo "installed $ROOT/$MANIFEST"
    case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "note: $PREFIX/bin is not on PATH" ;; esac
}

case "${1:-}" in
    --uninstall) uninstall ;;
    ""|--no-build|--force) install_all "$@" ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//' ;;
    *) die "unknown option: $1 (see --help)" ;;
esac
