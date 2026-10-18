#!/bin/sh
# Regenerates every app icon file from assets/app-icon/silex.svg.
# Needs macOS (iconutil) and rsvg-convert (brew install librsvg).
# The generated files are committed, so building or packaging never runs this.
set -eu
cd "$(dirname "$0")/.."

src=assets/app-icon/silex.svg
out=assets/app-icon
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# PNGs: window icon, Linux hicolor sizes and the sources of .icns and .ico.
for size in 16 32 48 64 128 256 512 1024; do
    rsvg-convert -w "$size" -h "$size" "$src" -o "$out/silex-$size.png"
done

# macOS: an .iconset folder turned into Silex.icns.
iconset="$tmp/Silex.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    cp "$out/silex-$size.png" "$iconset/icon_${size}x${size}.png"
    cp "$out/silex-$((size * 2)).png" "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o "$out/Silex.icns"

# Windows: PNG images inside one .ico.
cargo run --quiet --example pack_ico -- "$out/silex.ico" \
    "$out/silex-16.png" "$out/silex-32.png" "$out/silex-48.png" \
    "$out/silex-64.png" "$out/silex-128.png" "$out/silex-256.png"

echo "Iconos regenerados en $out"
