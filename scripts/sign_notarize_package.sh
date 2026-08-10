#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 9 ]]; then
  echo "usage: $0 <app> <dmg-staging> <dmg-path> <zip-path> <volume-name> <display-name> <background> <signing-identity> <notary-profile>" >&2
  exit 2
fi

app_path="$1"
dmg_staging="$2"
dmg_path="$3"
zip_path="$4"
volume_name="$5"
display_name="$6"
background_image="$7"
signing_identity="$8"
notary_profile="$9"
script_dir="$(cd "$(dirname "$0")" && pwd)"
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/moeka-notary.XXXXXX")"
notary_zip="$temp_dir/notary.zip"

cleanup() {
  rm -rf "$temp_dir"
}
trap cleanup EXIT

[[ -d "$app_path" ]] || { echo "missing app bundle: $app_path" >&2; exit 1; }
[[ -n "$signing_identity" ]] || { echo "Developer ID Application identity is empty" >&2; exit 1; }

codesign \
  --force \
  --timestamp \
  --options runtime \
  --sign "$signing_identity" \
  "$app_path"

codesign --verify --deep --strict --verbose=2 "$app_path"

ditto -c -k --sequesterRsrc --keepParent "$app_path" "$notary_zip"
xcrun notarytool submit "$notary_zip" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$app_path"
xcrun stapler validate "$app_path"
codesign --verify --deep --strict --verbose=2 "$app_path"
spctl --assess --type execute --verbose=2 "$app_path"

mkdir -p "$(dirname "$dmg_path")" "$(dirname "$zip_path")"
rm -rf "$dmg_staging"
mkdir -p "$dmg_staging"
cp -R "$app_path" "$dmg_staging/$display_name.app"
ln -sfn /Applications "$dmg_staging/Applications"
rm -f "$dmg_path" "$zip_path"

bash "$script_dir/create_dmg.sh" \
  "$dmg_staging" \
  "$dmg_path" \
  "$volume_name" \
  "$display_name" \
  "$background_image"

ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip_path"

echo "Created signed and notarized $dmg_path"
echo "Created signed and notarized $zip_path"
