#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "usage: $0 <staging-dir> <dmg-path> <volume-name> <app-display-name> <background-image>" >&2
  exit 2
fi

staging_dir="$1"
dmg_path="$2"
volume_name="$3"
app_display_name="$4"
background_image="$5"
dmg_format="${DMG_FORMAT:-ULFO}"

background_dir="$staging_dir/.background"
background_name="install-bg.jpg"
rw_dmg="$(mktemp "${TMPDIR:-/tmp}/moeka-dmg.XXXXXX.rw.dmg")"
mount_dir=""
device=""
window_left=120
window_top=120
window_width=548
window_height=313
window_right=$((window_left + window_width))
window_bottom=$((window_top + window_height))

cleanup() {
  if [[ -n "$device" ]]; then
    hdiutil detach "$device" -quiet >/dev/null 2>&1 || true
  fi
  rm -f "$rw_dmg"
}
trap cleanup EXIT

mkdir -p "$background_dir"
cp "$background_image" "$background_dir/$background_name"
sips -z "$window_height" "$window_width" -s dpiWidth 72 -s dpiHeight 72 "$background_dir/$background_name" >/dev/null
rm -f "$dmg_path"

hdiutil create \
  -volname "$volume_name" \
  -srcfolder "$staging_dir" \
  -fs HFS+ \
  -format UDRW \
  -ov \
  "$rw_dmg" >/dev/null

attach_output="$(hdiutil attach "$rw_dmg" -readwrite -noverify -noautoopen)"
device="$(awk '/Apple_HFS/ { print $1; exit }' <<<"$attach_output")"
mount_dir="$(awk '/Apple_HFS/ { print $3; exit }' <<<"$attach_output")"
if [[ -z "$device" || -z "$mount_dir" ]]; then
  echo "failed to mount writable DMG" >&2
  exit 1
fi

chflags hidden "$mount_dir/.background" || true

osascript <<OSA
tell application "Finder"
    tell disk "$volume_name"
        open
        delay 0.2
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {$window_left, $window_top, $window_right, $window_bottom}
        set viewOptions to the icon view options of container window
        set arrangement of viewOptions to not arranged
        set icon size of viewOptions to 128
        set backgroundPicture to POSIX file "$mount_dir/.background/$background_name" as alias
        set background picture of viewOptions to backgroundPicture
        set position of item "$app_display_name.app" of container window to {165, 150}
        set position of item "Applications" of container window to {383, 150}
        update without registering applications
        delay 0.5
        close
        delay 0.5
    end tell
end tell
OSA

sync
      if [[ ! -f "$mount_dir/.DS_Store" ]]; then
        echo "Finder did not write $mount_dir/.DS_Store; DMG layout was not persisted" >&2
        exit 1
      fi

hdiutil detach "$device" -quiet
device=""

hdiutil convert "$rw_dmg" -format "$dmg_format" -o "$dmg_path" >/dev/null
