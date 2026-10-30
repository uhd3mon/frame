#!/usr/bin/env bash
# Build dist/Frame-x86_64.AppImage: one file that runs on any x86_64 Linux
# with glibc >= 2.35 (see packaging/appimage/README.md).
#
#   packaging/appimage/build.sh            build frame in Docker (Ubuntu 22.04), then pack
#   packaging/appimage/build.sh --host     build with the local cargo (needs the host's glibc)
#   packaging/appimage/build.sh --no-build pack the binary from the last build again
#
# Downloads (appimagetool, runtime) are pinned in versions.env, cached in
# target/appimage/tools and checked by sha256 before use.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
# shellcheck source=versions.env
. "$HERE/versions.env"

WORK="$ROOT/target/appimage"
TOOLS="$WORK/tools"
DIST="$ROOT/dist"
OUT="$DIST/Frame-x86_64.AppImage"
MODE=docker

for a in "$@"; do
    case "$a" in
        --host) MODE=host ;;
        --no-build) MODE=none ;;
        -h|--help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "build.sh: unknown option: $a" >&2; exit 2 ;;
    esac
done

# fetch URL SHA256 FILE: download once, then always check the sha256.
fetch() {
    local url="$1" sum="$2" file="$3"
    if [[ ! -f "$file" ]]; then
        echo "download $url"
        curl -fsSL -o "$file.part" "$url"
        mv "$file.part" "$file"
    fi
    if ! echo "$sum  $file" | sha256sum -c --quiet -; then
        echo "build.sh: sha256 mismatch for $file (delete it to download again)" >&2
        exit 1
    fi
    chmod +x "$file"
}

mkdir -p "$TOOLS" "$DIST"
fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256" "$TOOLS/appimagetool-x86_64.AppImage"
fetch "$RUNTIME_URL" "$RUNTIME_SHA256" "$TOOLS/runtime-x86_64"

case "$MODE" in
    docker)
        BIN="$WORK/target/release/frame"
        tag="frame-appimage-build:$RUST_TOOLCHAIN"
        docker build -q -t "$tag" \
            --build-arg BUILD_IMAGE="$BUILD_IMAGE" \
            --build-arg RUSTUP_INIT_URL="$RUSTUP_INIT_URL" \
            --build-arg RUSTUP_INIT_SHA256="$RUSTUP_INIT_SHA256" \
            --build-arg RUST_TOOLCHAIN="$RUST_TOOLCHAIN" \
            "$HERE" >/dev/null
        mkdir -p "$WORK/cargo-home"
        docker run --rm -u "$(id -u):$(id -g)" \
            -v "$ROOT:/src" -w /src \
            -e HOME=/tmp -e CARGO_HOME=/src/target/appimage/cargo-home \
            -e CARGO_TARGET_DIR=/src/target/appimage/target \
            "$tag" cargo build --release --locked
        ;;
    host)
        BIN="$ROOT/target/release/frame"
        (cd "$ROOT" && cargo build --release --locked)
        ;;
    none)
        BIN="$WORK/target/release/frame"
        [[ -x "$BIN" ]] || BIN="$ROOT/target/release/frame"
        ;;
esac
[[ -x "$BIN" ]] || { echo "build.sh: no binary at $BIN" >&2; exit 1; }

# Newest glibc symbol version the binary needs = the AppImage's minimum.
glibc=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)
echo "binary: $BIN (needs $glibc)"

APPDIR="$WORK/AppDir"
rm -rf "$APPDIR"
cp -r "$HERE/AppDir" "$APPDIR"
install -Dm755 "$BIN" "$APPDIR/usr/bin/frame"
install -Dm644 "$ROOT/icons/frame.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/frame.svg"
for s in 16 22 24 32 48 64 128 256; do
    install -Dm644 "$ROOT/icons/png/frame-$s.png" "$APPDIR/usr/share/icons/hicolor/${s}x$s/apps/frame.png"
done
cp "$ROOT/icons/png/frame-256.png" "$APPDIR/frame.png"
ln -s frame.png "$APPDIR/.DirIcon"
install -Dm644 "$ROOT/LICENSE" "$APPDIR/usr/share/licenses/frame/LICENSE"

# The tool is an AppImage too: extract-and-run needs no FUSE on the build PC.
rm -f "$OUT"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOLS/appimagetool-x86_64.AppImage" \
    --no-appstream --runtime-file "$TOOLS/runtime-x86_64" "$APPDIR" "$OUT"
chmod +x "$OUT"
echo "built $OUT ($(du -h "$OUT" | cut -f1), sha256 $(sha256sum "$OUT" | cut -d' ' -f1))"
