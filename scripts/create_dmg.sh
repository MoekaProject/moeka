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
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/moeka-dmg.XXXXXX")"
rw_dmg="$temp_dir/writable.dmg"
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
  rm -rf "$temp_dir"
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
attach_line="$(awk '/Apple_HFS/ && /\/Volumes\// { print; exit }' <<<"$attach_output")"
device="$(awk '{ print $1 }' <<<"$attach_line")"
mount_dir="$(sed -E 's|^.*[[:space:]](/Volumes/.*)$|\1|' <<<"$attach_line")"
if [[ -z "$device" || -z "$mount_dir" || ! -d "$mount_dir" ]]; then
  printf '%s\n' "$attach_output" >&2
  echo "failed to mount writable DMG" >&2
  exit 1
fi

chflags hidden "$mount_dir/.background" || true

osascript - "$mount_dir" "$app_display_name" "$background_name" <<OSA
on run argv
    set mountDir to item 1 of argv
    set appName to (item 2 of argv) & ".app"
    set backgroundName to item 3 of argv
    set mountedVolume to POSIX file mountDir as alias
    set backgroundPicture to POSIX file (mountDir & "/.background/" & backgroundName) as alias
    tell application "Finder"
        set volumeWindow to make new Finder window to mountedVolume
        delay 0.5
        set current view of volumeWindow to icon view
        set toolbar visible of volumeWindow to false
        set statusbar visible of volumeWindow to false
        set the bounds of volumeWindow to {$window_left, $window_top, $window_right, $window_bottom}
        set viewOptions to the icon view options of volumeWindow
        set arrangement of viewOptions to not arranged
        set icon size of viewOptions to 128
        set background picture of viewOptions to backgroundPicture
        set position of item appName of mountedVolume to {165, 150}
        set position of item "Applications" of mountedVolume to {383, 150}
        update mountedVolume without registering applications
        delay 0.5
        close volumeWindow
        delay 0.5
    end tell
end run
OSA

sync
if [[ ! -f "$mount_dir/.DS_Store" ]]; then
  echo "Finder did not write $mount_dir/.DS_Store; DMG layout was not persisted" >&2
  exit 1
fi

hdiutil detach "$device" -quiet
device=""

hdiutil convert "$rw_dmg" -format "$dmg_format" -o "$dmg_path" >/dev/null
