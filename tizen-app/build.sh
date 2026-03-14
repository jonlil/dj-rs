#!/bin/bash
# build.sh — Build the dj-rs Tizen .wgt package (plain HTML/JS, no WASM)
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

DIST="dist"
WGT="dj-rs.wgt"
TIZEN="$HOME/tizen-studio/tools/ide/bin/tizen"

# Load cert credentials from local env file (not committed to git)
if [ -f ".env.tizen" ]; then
    # shellcheck source=/dev/null
    . ".env.tizen"
fi
CERT_PROFILE="${CERT_PROFILE:-djrs-dev}"

rm -rf "$DIST"
mkdir -p "$DIST"

echo "==> Copying app..."
cp app.html "$DIST/index.html"
cp public/config.xml "$DIST/config.xml"

echo "==> Generating icon..."
if command -v convert >/dev/null 2>&1; then
    convert -size 96x96 xc:'#121212' -fill '#1DB954' \
        -font DejaVu-Sans-Bold -pointsize 32 \
        -gravity Center -annotate 0 'dj' "$DIST/icon.png"
else
    printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00`\x00\x00\x00`\x08\x02\x00\x00\x00\xd0\xd3Zy\x00\x00\x00\x16IDATx\x9cc\xfc\xff\xff?\x03\x10\x18\xc4\xc8P!\x00\x00\x00\x00\xff\xff\x03\x00\x18\x88\x04\xdaJe\x94\x0c\x00\x00\x00\x00IEND\xaeB`\x82' > "$DIST/icon.png"
fi

echo "==> Signing and packaging..."
"$TIZEN" package -t wgt -- "$DIST"
mv "$DIST"/*.wgt "./$WGT" 2>/dev/null || true
echo "==> Done: $WGT"
