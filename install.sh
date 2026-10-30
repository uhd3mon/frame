#!/usr/bin/env bash
# Install frame from source: build it, then run `frame install`, which
# copies the binary, its menu entry and icons under $PREFIX (default
# ~/.local), cleans up older Frame / Clipped installs, stops and restarts
# a running tray and turns on start-at-login on a first install. The
# AppImage installs itself the same way (`Frame-x86_64.AppImage install`,
# or the button in its settings window). DESTDIR (optional) is put in
# front of every path, for packaging; the files still refer to $PREFIX.
#
#   ./install.sh               build frame (cargo, release) and install
#   ./install.sh --no-build    install the frame already in target/release
#   ./install.sh --force       also replace a bin/frame we didn't install
#   ./install.sh --no-autostart   don't make Frame start at login
#   ./install.sh --uninstall   stop frame, remove its shortcuts and files
#
# All of the work is in src/install.rs (tested by `cargo test`).
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"
die() { echo "install.sh: $*" >&2; exit 1; }

build=1 uninstall=0 pass=()
for a in "$@"; do
    case "$a" in
        --no-build) build=0 ;;
        --uninstall) uninstall=1 ;;
        --force|--no-autostart) pass+=("$a") ;;
        -h|--help) sed -n '2,/^[^#]/{/^#/s/^# \{0,1\}//p}' "$0"; exit 0 ;;
        *) die "unknown option: $a (see --help)" ;;
    esac
done

bin=target/release/frame
if ((uninstall)); then
    # Any frame can uninstall; prefer the one built here, else the installed one.
    if [[ ! -x "$bin" ]]; then bin="${DESTDIR:-}${PREFIX:-$HOME/.local}/bin/frame"; fi
    [[ -x "$bin" ]] || die "no frame binary to run (build one, or nothing is installed)"
    exec "$bin" uninstall
fi
if ((build)); then
    command -v cargo >/dev/null || die "cargo not found (install Rust, or use --no-build)"
    cargo build --release
fi
[[ -x "$bin" ]] || die "$bin is missing; build it first"
exec "$bin" install "${pass[@]}"
