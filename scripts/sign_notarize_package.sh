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
source "$script_dir/release-network.sh"
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
if xcrun notarytool submit "$notary_zip" --keychain-profile "$notary_profile" --no-s3-acceleration --wait --timeout 15m \
    >"$temp_dir/notary-out" 2>"$temp_dir/notary-error"; then
  cat "$temp_dir/notary-out"
  cat "$temp_dir/notary-error" >&2
  grep -Eq '^[[:space:]]*status:[[:space:]]*Accepted[[:space:]]*$' "$temp_dir/notary-out" \
      || { echo "Notarization did not report an accepted result; no package will be created." >&2; exit 1; }
else
  status=$?
  cat "$temp_dir/notary-out"
  cat "$temp_dir/notary-error" >&2
  # Never submit the archive twice: a lost response may have reached Apple.
  id=$(awk '$1 == "id:" && $2 ~ /^[0-9A-Fa-f-]+$/ {print $2}' "$temp_dir/notary-out" "$temp_dir/notary-error" | sort -u)
  if ! release_network_transient "$temp_dir/notary-out" "$temp_dir/notary-error" \
      || [[ ! "$id" =~ ^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$ ]]; then
    echo "Notarization outcome is unknown; inspect the original submission before retrying." >&2
    exit "$status"
  fi
  command -v jq >/dev/null || { echo "jq is required to verify the existing notarization submission." >&2; exit 1; }
  echo "Waiting for existing notarization submission $id; not resubmitting." >&2
  release_retry_read xcrun notarytool wait "$id" --keychain-profile "$notary_profile" --output-format json --timeout 15m \
      >"$temp_dir/notary-result.json"
  jq -e --arg id "$id" '.id == $id and .status == "Accepted"' "$temp_dir/notary-result.json" >/dev/null \
      || { echo "Existing notarization submission was not accepted." >&2; exit 1; }
fi
# Stapling the same accepted ticket to the same signed bundle is idempotent.
release_retry_read xcrun stapler staple "$app_path"
release_retry_read xcrun stapler validate "$app_path"
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
