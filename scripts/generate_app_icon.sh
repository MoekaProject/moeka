#!/usr/bin/env bash
#
# Generate a macOS AppIcon.icns from a single square PNG tile.
#
# Usage: generate_app_icon.sh <source.png> <output.icns>
#
# Defaults to the first sliced application-icon tile when no arguments are
# given, matching the tile picker in the settings panel. The tile is a
# full-bleed rounded square, so the standard macOS icon margin (~10% on each
# side, leaving the artwork at ~80.5% of the canvas) is added here before the
# .icns is built.

set -euo pipefail

SOURCE="${1:-assets/icons/app/icon-0.png}"
OUTPUT="${2:-packaging/macos/AppIcon.icns}"

# Fraction of the icon canvas occupied by the rounded-square artwork; the rest
# is transparent margin, matching Apple's macOS icon grid (824 / 1024).
CONTENT_RATIO=0.8047

if [[ ! -f "$SOURCE" ]]; then
  echo "error: source icon not found: $SOURCE" >&2
  exit 1
fi

if ! command -v sips >/dev/null 2>&1 || ! command -v iconutil >/dev/null 2>&1; then
  echo "error: sips and iconutil are required (macOS only)" >&2
  exit 1
fi

WORK_DIR="$(mktemp -d)"
ICONSET_DIR="$WORK_DIR/AppIcon.iconset"
MASTER="$WORK_DIR/master.png"
mkdir -p "$ICONSET_DIR"

# Build a padded 1024 master: shrink the tile to the content size and center it
# on a transparent 1024x1024 canvas (sips padding keeps the alpha channel).
CONTENT=$(awk "BEGIN { printf \"%d\", 1024 * $CONTENT_RATIO }")
sips -z "$CONTENT" "$CONTENT" "$SOURCE" --out "$WORK_DIR/content.png" >/dev/null
sips -p 1024 1024 "$WORK_DIR/content.png" --out "$MASTER" >/dev/null

# name              pixel size
render() {
  local size="$1" name="$2"
  sips -z "$size" "$size" "$MASTER" --out "$ICONSET_DIR/$name" >/dev/null
}

render 16   icon_16x16.png
render 32   icon_16x16@2x.png
render 32   icon_32x32.png
render 64   icon_32x32@2x.png
render 128  icon_128x128.png
render 256  icon_128x128@2x.png
render 256  icon_256x256.png
render 512  icon_256x256@2x.png
render 512  icon_512x512.png
render 1024 icon_512x512@2x.png

mkdir -p "$(dirname "$OUTPUT")"
iconutil -c icns "$ICONSET_DIR" -o "$OUTPUT"
rm -rf "$WORK_DIR"

echo "Created $OUTPUT from $SOURCE"

