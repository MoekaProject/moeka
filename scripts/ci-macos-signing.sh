#!/usr/bin/env bash
# Runs only on an ephemeral GitHub-hosted signing runner, never a developer keychain.
set -euo pipefail

[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] \
  || { echo "An ephemeral GitHub-hosted runner is required." >&2; exit 1; }
[[ "${RUNNER_TEMP:-}" == /* && "$RUNNER_TEMP" != / ]] \
  || { echo "RUNNER_TEMP must be an absolute temporary directory." >&2; exit 1; }
directory="$RUNNER_TEMP/moeka-signing"
keychain="$directory/signing.keychain-db"
certificate="$directory/certificate.p12"
search_list="$directory/original-search-list"
probe="$directory/signing-probe"

read_search_list() {
  original_keychains=()
  while IFS= read -r original; do
    [[ -n "$original" ]] || continue
    [[ "$original" == /* ]] || { echo "Unexpected keychain search-list entry." >&2; return 1; }
    original_keychains+=("$original")
  done <"$search_list"
}

cleanup() {
  local result=0
  if [[ -f "$search_list" ]]; then
    if read_search_list; then
      if (( ${#original_keychains[@]} > 0 )); then
        security list-keychains -d user -s "${original_keychains[@]}" || result=1
      else
        security list-keychains -d user -s || result=1
      fi
      if (( result == 0 )); then rm "$search_list" || result=1; fi
    else
      result=1
    fi
  fi
  if [[ -f "$keychain" ]]; then
    security delete-keychain "$keychain" || result=1
  fi
  rm -f "$certificate" "$probe" || result=1
  if [[ -d "$directory" ]]; then
    rmdir "$directory" || result=1
  fi
  return "$result"
}

case "${1:-}" in
  cleanup) cleanup; exit ;;
  setup) ;;
  *) echo "Usage: $0 setup|cleanup" >&2; exit 2 ;;
esac

: "${MACOS_CERTIFICATE_BASE64:?Missing MACOS_CERTIFICATE_BASE64}"
: "${MACOS_CERTIFICATE_PASSWORD:?Missing MACOS_CERTIFICATE_PASSWORD}"
: "${APPLE_ID:?Missing APPLE_ID}"
: "${APPLE_APP_SPECIFIC_PASSWORD:?Missing APPLE_APP_SPECIFIC_PASSWORD}"
: "${APPLE_TEAM_ID:?Missing APPLE_TEAM_ID}"
: "${DEVELOPER_ID_APPLICATION:?Missing DEVELOPER_ID_APPLICATION}"
[[ "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] \
  || { echo "APPLE_TEAM_ID must be a ten-character Team ID." >&2; exit 1; }
[[ "$DEVELOPER_ID_APPLICATION" == "Developer ID Application: "*" ($APPLE_TEAM_ID)" ]] \
  || { echo "Signing identity must be Developer ID Application for APPLE_TEAM_ID." >&2; exit 1; }

umask 077
mkdir "$directory"
trap 'status=$?; if (( status != 0 )); then cleanup || echo "Signing credential cleanup failed." >&2; fi; exit "$status"' EXIT
password="$(openssl rand -hex 32)"
printf '::add-mask::%s\n' "$password"
printf '%s' "$MACOS_CERTIFICATE_BASE64" | base64 --decode >"$certificate"
security list-keychains -d user |
  sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' >"$search_list"
read_search_list
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
if (( ${#original_keychains[@]} > 0 )); then
  security list-keychains -d user -s "$keychain" "${original_keychains[@]}"
else
  security list-keychains -d user -s "$keychain"
fi
security import "$certificate" -P "$MACOS_CERTIFICATE_PASSWORD" -k "$keychain" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
security find-identity -v -p codesigning "$keychain" |
  grep -F "\"$DEVELOPER_ID_APPLICATION\"" >/dev/null \
  || { echo "The P12 does not contain the configured valid signing identity." >&2; exit 1; }
cp /usr/bin/true "$probe"
codesign --force --timestamp=none --sign "$DEVELOPER_ID_APPLICATION" --keychain "$keychain" "$probe"
codesign --verify --strict "$probe"
rm "$probe"
xcrun notarytool store-credentials moeka-ci \
  --apple-id "$APPLE_ID" \
  --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" \
  --keychain "$keychain"
rm "$certificate"
