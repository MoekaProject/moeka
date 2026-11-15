#!/usr/bin/env bash
# Offline fixtures: no real keychain, credential, network request or signature.
set -euo pipefail
helper="$(cd "$(dirname "$0")" && pwd)/ci-macos-signing.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/bin"
cat >"$work/bin/security" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >>"$CALLS"
for arg in "$@"; do last="$arg"; done
case "$1" in
  list-keychains)
    if [[ "$*" == 'list-keychains -d user' ]]; then
      printf '    "/tmp/fixture login.keychain-db"\n'
    elif [[ "$*" == *moeka-signing/signing.keychain-db* ]]; then
      touch "$RUNNER_TEMP/search-list-installed"
    else
      [[ "$*" == 'list-keychains -d user -s /tmp/fixture login.keychain-db' ]]
      rm -f "$RUNNER_TEMP/search-list-installed"
    fi ;;
  create-keychain) touch "$last" ;;
  set-keychain-settings|unlock-keychain|set-key-partition-list) ;;
  import)
    if [[ "$SCENARIO" == invalid-p12 ]]; then exit 22; fi
    [[ "$(cat "$2")" == fixture-certificate ]] ;;
  find-identity)
    if [[ "$SCENARIO" != missing-identity ]]; then
      printf '  1) 0000000000000000000000000000000000000000 "%s"\n' "$DEVELOPER_ID_APPLICATION"
    fi ;;
  delete-keychain) rm "$2" ;;
  *) echo "Unexpected security command: $1" >&2; exit 99 ;;
esac
MOCK
cat >"$work/bin/codesign" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ -f "$RUNNER_TEMP/search-list-installed" ]]
echo signing-probe >>"$CALLS"
if [[ "$SCENARIO" == signing-failed ]]; then exit 24; fi
MOCK
cat >"$work/bin/xcrun" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1 $2 $3" == 'notarytool store-credentials moeka-ci' ]]
[[ " $* " == *" --keychain $RUNNER_TEMP/moeka-signing/signing.keychain-db "* ]]
[[ " $* " == *" --apple-id $APPLE_ID "* && " $* " == *" --team-id $APPLE_TEAM_ID "* ]]
[[ " $* " == *" --password $APPLE_APP_SPECIFIC_PASSWORD "* ]]
echo notarization-credentials >>"$CALLS"
if [[ "$SCENARIO" == rejected-notary ]]; then exit 23; fi
MOCK
chmod +x "$work/bin/security" "$work/bin/xcrun" "$work/bin/codesign"
export PATH="$work/bin:$PATH"
export GITHUB_ACTIONS=true RUNNER_ENVIRONMENT=github-hosted
export MACOS_CERTIFICATE_BASE64
MACOS_CERTIFICATE_BASE64="$(printf fixture-certificate | base64)"
export MACOS_CERTIFICATE_PASSWORD=fixture-export-password
export APPLE_ID=fixture@example.invalid APPLE_APP_SPECIFIC_PASSWORD=fixture-app-password
export APPLE_TEAM_ID=ABCDEFGHIJ
export DEVELOPER_ID_APPLICATION='Developer ID Application: Fixture (ABCDEFGHIJ)'

for scenario in success invalid-p12 missing-identity signing-failed rejected-notary; do
  export SCENARIO="$scenario" RUNNER_TEMP="$work/$scenario" CALLS="$work/$scenario.calls"
  mkdir "$RUNNER_TEMP"
  status=0
  bash "$helper" setup >"$work/$scenario.log" 2>&1 || status=$?
  if [[ "$scenario" == success ]]; then
    [[ "$status" == 0 && -f "$RUNNER_TEMP/moeka-signing/signing.keychain-db" ]]
    [[ ! -e "$RUNNER_TEMP/moeka-signing/certificate.p12" ]]
    bash "$helper" cleanup
    bash "$helper" cleanup
    grep -q '^notarization-credentials$' "$CALLS"
  else
    [[ "$status" != 0 ]]
    grep -q '^delete-keychain$' "$CALLS"
    if [[ "$scenario" != rejected-notary ]]; then
      ! grep -q '^notarization-credentials$' "$CALLS"
    fi
  fi
  [[ ! -e "$RUNNER_TEMP/moeka-signing" ]]
  [[ ! -e "$RUNNER_TEMP/search-list-installed" ]]
  ! grep -Eq 'fixture-export-password|fixture-app-password|fixture-certificate' "$work/$scenario.log"
  printf 'PASS signing credential setup: %s\n' "$scenario"
done

export RUNNER_TEMP="$work/refusal" CALLS="$work/refusal.calls"
mkdir "$RUNNER_TEMP"
if RUNNER_ENVIRONMENT=self-hosted bash "$helper" setup >"$work/refusal.log" 2>&1; then exit 1; fi
if APPLE_TEAM_ID=WRONGTEAM1 bash "$helper" setup >>"$work/refusal.log" 2>&1; then exit 1; fi
if MACOS_CERTIFICATE_PASSWORD= bash "$helper" setup >>"$work/refusal.log" 2>&1; then exit 1; fi
[[ ! -e "$CALLS" && ! -e "$RUNNER_TEMP/moeka-signing" ]]
echo 'PASS signing refuses unsupported runner, mismatched team and missing credential'
