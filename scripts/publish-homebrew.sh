#!/usr/bin/env bash
set -euo pipefail
export GH_HOST=github.com GH_PAGER=cat GIT_PAGER=cat PAGER=cat GH_PROMPT_DISABLED=1 GIT_TERMINAL_PROMPT=0
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/release-network.sh"

die() { printf 'Homebrew update refused: %s\n' "$*" >&2; exit 1; }
tag=${1:-}
mode=${2:-publish}
[[ $# -le 2 && "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
    || die 'Usage: publish-homebrew.sh vX.Y.Z [check|publish]'
[[ "$tag" != v0.1.12 ]] || die 'v0.1.12 is protected.'
[[ "$mode" == check || "$mode" == publish ]] || die 'Expected check or publish.'
for tool in git gh jq shasum ruby; do
    command -v "$tool" >/dev/null || die "Missing tool: $tool"
done
repo=MoekaProject/community
tap=https://github.com/MoekaProject/homebrew-moeka.git
version=${tag#v}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

release_retry_read gh api --method GET "repos/$repo/releases/tags/$tag" >"$work/release.json"
jq -e --arg tag "$tag" '
    .tag_name == $tag and .draft == false and .prerelease == false
    and (.assets | type == "array")
' "$work/release.json" >/dev/null || die 'Expected a published stable release.'
for arch in arm64 x86_64; do
    name="Moeka-macos-$arch.dmg"
    jq -e --arg name "$name" '
        [.assets[] | select(.name == $name)] |
        length == 1 and (.[0] |
            .state == "uploaded" and (.size | type == "number") and .size > 0
            and (.digest | type == "string")
            and (.digest | test("^sha256:[0-9a-f]{64}$")))
    ' "$work/release.json" >/dev/null || die "Missing, duplicate or invalid asset: $name"
done

# Only the public tap is cloned or pushed; private source history never enters it.
clone_tap() {
    rm -rf "$work/tap"
    git clone --quiet --single-branch --branch main "$tap" "$work/tap"
}
release_retry_read clone_tap
cd "$work/tap"
cask=Casks/moeka.rb
[[ -f "$cask" && ! -L "$cask" ]] || die 'Expected a regular Casks/moeka.rb.'
current=$(sed -nE 's/^  version "([0-9]+\.[0-9]+\.[0-9]+)"$/\1/p' "$cask")
[[ "$current" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'Cannot parse current cask version.'
jq -en --arg current "$current" --arg target "$version" '
    ($target | split(".") | map(tonumber)) >= ($current | split(".") | map(tonumber))
' >/dev/null || die "Refusing downgrade from $current to $version."

for arch in arm64 x86_64; do
    name="Moeka-macos-$arch.dmg"
    download_dmg() {
        rm -f "$work/$name"
        gh release download "$tag" --repo "$repo" --pattern "$name" --dir "$work"
    }
    release_retry_read download_dmg
    size=$(wc -c <"$work/$name" | tr -d '[:space:]')
    hash=$(shasum -a 256 "$work/$name" | awk '{print $1}')
    jq -e --arg name "$name" --arg hash "sha256:$hash" --argjson size "$size" '
        .assets[] | select(.name == $name) | .digest == $hash and .size == $size
    ' "$work/release.json" >/dev/null || die "Downloaded asset integrity mismatch: $name"
    if [[ "$arch" == arm64 ]]; then arm_hash=$hash; else intel_hash=$hash; fi
done

ruby - "$cask" "$version" "$arm_hash" "$intel_hash" <<'RUBY'
path, version, arm, intel = ARGV
text = File.read(path)
replacements = {
  /^  version "[0-9]+\.[0-9]+\.[0-9]+"$/ => "  version \"#{version}\"",
  /^  sha256 arm:   "[0-9a-f]{64}",$/ => "  sha256 arm:   \"#{arm}\",",
  /^         intel: "[0-9a-f]{64}"$/ => "         intel: \"#{intel}\""
}
replacements.each do |pattern, value|
  abort "Unexpected cask format: #{pattern}" unless text.scan(pattern).length == 1
  text = text.sub(pattern, value)
end
File.write(path, text)
RUBY
ruby -c "$cask"
git --no-pager diff --check
if git --no-pager diff --quiet -- "$cask"; then
    printf 'Homebrew already publishes %s; both DMGs verified.\n' "$tag"
    exit 0
fi
[[ "$current" != "$version" ]] || die 'Same-version checksum change requires manual investigation.'
git --no-pager diff -- "$cask"
if [[ "$mode" == check ]]; then
    printf 'Verified Homebrew update to %s (check only; no push).\n' "$tag"
    exit 0
fi

git add -- "$cask"
original_head=$(git rev-parse HEAD)
git commit -m "moeka: update to $version" \
    -m 'Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>'
# A concurrent update rejects this non-forced push; rerun to revalidate, never rebase stale hashes.
head=$(git rev-parse HEAD)
for attempt in 1 2 3; do
    if git push origin HEAD:refs/heads/main >"$work/push-out" 2>"$work/push-error"; then
        cat "$work/push-out"; cat "$work/push-error" >&2
        break
    fi
    remote=$(release_retry_read git ls-remote --exit-code origin refs/heads/main) \
        || die 'Cannot determine tap state after push; inspect remote before retrying.'
    [[ "$remote" != "$head"$'\trefs/heads/main' ]] || break
    [[ "$remote" == "$original_head"$'\trefs/heads/main' ]] \
        || die 'Tap changed concurrently; rerun to revalidate, never rebase stale hashes.'
    release_retry_wait "$attempt" "$work/push-out" "$work/push-error" \
        || die 'Tap push failed; rerun the Homebrew command after resolving the error.'
done
remote=$(release_retry_read git ls-remote --exit-code origin refs/heads/main)
[[ "$remote" == "$head"$'\trefs/heads/main' ]] || die 'Cannot confirm published tap HEAD; inspect remote state before retrying.'
printf 'Published Homebrew %s in MoekaProject/homebrew-moeka.\n' "$tag"
