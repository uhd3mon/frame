#!/usr/bin/env bash
# Regenerate gui/icons/png/*.png from the SVGs, plus preview.png (untracked).
# Needs rsvg-convert. The preview also needs ImageMagick (magick).
set -euo pipefail
cd "$(dirname "$0")"

names=(clip clip-idle clip-replay clip-recording)
sizes=(16 22 24 32 48 64)

mkdir -p png
for n in "${names[@]}"; do
    for s in "${sizes[@]}"; do
        rsvg-convert -w "$s" -h "$s" "$n.svg" -o "png/$n-$s.png"
    done
done
for s in 128 256; do
    rsvg-convert -w "$s" -h "$s" clip.svg -o "png/clip-$s.png"
done

command -v magick >/dev/null || { echo "magick not found; skipped preview.png"; exit 0; }

# Preview: one row per icon, each size at 1:1 then the same size blown up
# 4x with nearest-neighbour, so 16 px pixels can be judged by eye. Dark
# (Breeze Dark panel) strip on top, light strip below.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
strip() { # bg out
    local bg=$1 out=$2 rows=() n s cells
    for n in "${names[@]}"; do
        cells=()
        for s in 16 22 32 64; do
            magick -size 72x72 "xc:$bg" "png/$n-$s.png" -gravity center \
                -composite "$tmp/a-$n-$s.png"
            magick -size 260x260 "xc:$bg" \
                \( "png/$n-$s.png" -filter point -resize 400% \) \
                -gravity center -composite "$tmp/b-$n-$s.png"
            cells+=("$tmp/a-$n-$s.png" "$tmp/b-$n-$s.png")
        done
        magick "${cells[@]}" -background "$bg" -gravity west -splice 12x0 \
            +append "$tmp/row-$n.png"
        rows+=("$tmp/row-$n.png")
    done
    magick "${rows[@]}" -background "$bg" -append -bordercolor "$bg" \
        -border 12 "$out"
}
strip '#232629' "$tmp/dark.png"
strip '#eff0f1' "$tmp/light.png"
magick "$tmp/dark.png" "$tmp/light.png" -append preview.png
echo "wrote png/ ($(ls png | wc -l) files) and preview.png"
