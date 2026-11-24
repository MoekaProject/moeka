#!/usr/bin/env bash
set -euo pipefail

[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] \
  || { echo "An ephemeral GitHub-hosted signing runner is required." >&2; exit 1; }
[[ $# == 2 ]] || { echo "Usage: $0 PAYLOAD_DIRECTORY macos-x86_64|macos-aarch64" >&2; exit 2; }
directory="$1"
target="$2"
case "$target" in macos-x86_64|macos-aarch64) ;; *) echo "Unsupported signing target" >&2; exit 2 ;; esac
: "${DEVELOPER_ID_APPLICATION:?Missing signing identity}"
: "${SIGNING_KEYCHAIN:?Missing temporary signing keychain}"
: "${NOTARY_OUTPUT_DIR:?Missing notarization recovery directory}"
: "${MOEKA_REMOTE_SOURCE_SHA:?Missing source identity}"
app_version="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
server_version="$(awk -F '"' '/^version = / { print $2; exit }' crates/moeka-remote-server/Cargo.toml)"
ruby scripts/remote-artifact.rb verify "$directory" "$app_version" "$server_version" "$MOEKA_REMOTE_SOURCE_SHA" "$target"
binary="$directory/$target.bin"
mkdir -p "$NOTARY_OUTPUT_DIR"
codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID_APPLICATION" \
  --keychain "$SIGNING_KEYCHAIN" "$binary"
codesign --verify --strict "$binary"
archive="$NOTARY_OUTPUT_DIR/$target.zip"
ditto -c -k --keepParent "$binary" "$archive"
xcrun notarytool submit "$archive" --keychain-profile moeka-ci \
  --keychain "$SIGNING_KEYCHAIN" --wait --output-format json >"$NOTARY_OUTPUT_DIR/$target.json"
ruby -rjson -e 'abort "Remote helper notarization was not accepted" unless JSON.parse(File.read(ARGV.fetch(0))).fetch("status") == "Accepted"' \
  "$NOTARY_OUTPUT_DIR/$target.json"
codesign --verify --strict "$binary"
ruby scripts/remote-artifact.rb record "$directory" "$app_version" "$server_version" "$MOEKA_REMOTE_SOURCE_SHA" "$target"
