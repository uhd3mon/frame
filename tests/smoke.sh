#!/usr/bin/env bash
# Smoke tests for bin/clip. Takes no screenshots.
# Run from the project root: bash tests/smoke.sh
set -uo pipefail

CLIP="$(dirname "$0")/../bin/clip"
fail=0

check() {
    local name="$1"; shift
    if "$@"; then echo "ok   $name"; else echo "FAIL $name"; fail=1; fi
}

check "syntax"        bash -n "$CLIP"
check "help"          bash -c "'$CLIP' --help | grep -q Usage"
check "version"       bash -c "'$CLIP' --version | grep -q '^clip '"
check "detect keys"   bash -c "'$CLIP' --detect | grep -q '^capture='"
check "kde->spectacle" bash -c "env -u HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP=KDE '$CLIP' --detect | grep -Eq '^capture=(spectacle|none)$'"
check "bad option=2"  bash -c "'$CLIP' --nope >/dev/null 2>&1; [ \$? -eq 2 ]"
check "bad mode=2"    bash -c "'$CLIP' -m nope >/dev/null 2>&1; [ \$? -eq 2 ]"

exit "$fail"
