#!/usr/bin/env bash
#
# Generate a macOS AppIcon.icns from the padded 1024 app-icon master.
#
# Usage: generate_app_icon.sh <source-1024.png> <output.icns>
#
# The master is a 1024x1024 canvas with the standard macOS margin and a baked
# drop shadow, so this script only downsamples it into the iconset. Keeping it
# pure sips/iconutil means `make dmg` needs no Python.

set -euo pipefail

SOURCE="${1:-packaging/macos/AppIcon-1024.png}"
OUTPUT="${2:-packaging/macos/AppIcon.icns}"

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
mkdir -p "$ICONSET_DIR"

# name              pixel size
render() {
  local size="$1" name="$2"
  sips -z "$size" "$size" "$SOURCE" --out "$ICONSET_DIR/$name" >/dev/null
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
