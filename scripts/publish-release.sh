#!/usr/bin/env bash
# Official releases are local-only. Invoke through make publish / publish-check.
set -euo pipefail
export GH_HOST=github.com
export GH_PROMPT_DISABLED=1
export GIT_TERMINAL_PROMPT=0

die() { printf 'Release refused: %s\n' "$*" >&2; exit 1; }
mode=${1:-}
tag=${2:-}
repo=${3:-MoekaProject/community}
[[ $# -le 3 && ( "$mode" == check || "$mode" == publish ) ]] || die 'Usage: publish-release.sh check|publish vX.Y.Z [MoekaProject/community]'
[[ "$repo" == MoekaProject/community ]] || die 'Official releases must stay in MoekaProject/community.'
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'Expected a stable vX.Y.Z tag.'
[[ "$tag" != v0.1.12 ]] || die 'v0.1.12 is protected; never modify or republish it.'
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
for tool in git gh shasum cmp; do
    command -v "$tool" >/dev/null || die "Missing tool: $tool"
done
tmp=$(mktemp -d)
lock=
cleanup() {
    rm -rf "$tmp"
    if [[ -n "$lock" ]]; then rmdir "$lock"; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
api="repos/$repo"
destination="https://github.com/$repo.git"
release_id=
release_state=absent
head=
public_head=

# --include supplies the actual HTTP status, not a substring of an error message.
# Only a positively observed HTTP 404 means absence; all other errors stop us.
release_query='[.id, .draft, .tag_name, .target_commitish] | @tsv'
read_release() {
    local result code row id draft remote_tag target extra
    if gh api --method GET --include "$api/releases/tags/$tag" --jq "$release_query" >"$tmp/response" 2>"$tmp/error"; then
        code=$(awk '/^HTTP\// { gsub("\r", ""); code=$2 } END {print code}' "$tmp/response")
        [[ "$code" == 200 ]] || die "Unexpected release HTTP status: $code"
        result=$(sed 's/\r$//' "$tmp/response" | awk 'body {print} /^$/ {body=1}')
    else
        code=$(awk '/^HTTP\// { gsub("\r", ""); code=$2 } END {print code}' "$tmp/response")
        [[ "$code" == 404 ]] || die "Cannot determine release state (HTTP ${code:-unknown}); network/auth/API errors are not absence."
        # Authenticated listing also finds drafts that the tag endpoint omits.
        result=$(gh api --method GET --paginate "$api/releases?per_page=100" \
            --jq ".[] | select(.tag_name == \"$tag\") | $release_query") || die 'Cannot list drafts after HTTP 404.'
    fi
    release_id=
    release_state=absent
    [[ -n "$result" ]] || { [[ "$code" == 404 ]] || die 'Empty release response.'; return; }
    [[ "$result" != *$'\n'* ]] || die 'Multiple releases for the requested tag.'
    row=$result
    IFS=$'\t' read -r id draft remote_tag target extra <<< "$row"
    [[ "$id" =~ ^[0-9]+$ && "$remote_tag" == "$tag" && -z "$extra" ]] || die 'Malformed release response.'
    [[ "$draft" == true ]] || die 'Release is already published (or has invalid draft state); assets are immutable.'
    [[ "$target" == "$public_head" ]] || die 'Existing draft target does not match community main; manual review required.'
    release_id=$id
    release_state=draft
}

remote_tag_commit() {
    local remote=$1 expected=$2 refs direct peeled
    refs=$(git ls-remote "$remote" "refs/tags/$tag" "refs/tags/$tag^{}") || die "Cannot read tag from $remote"
    direct=$(awk -v ref="refs/tags/$tag" '$2 == ref {print $1}' <<< "$refs")
    peeled=$(awk -v ref="refs/tags/$tag^{}" '$2 == ref {print $1}' <<< "$refs")
    if [[ -n "$refs" ]]; then
        [[ "$direct" =~ ^[0-9a-f]{40}$ ]] || die "Malformed remote tag at $remote"
        [[ "${peeled:-$direct}" == "$expected" ]] || die "Remote tag mismatch at $remote"
    fi
    printf '%s' "$direct"
}

local_checks() {
    local version branch upstream remote_main commit file tree_state
    tree_state=$(git status --porcelain=v1 --untracked-files=all) || die 'Cannot inspect working tree.'
    [[ -z "$tree_state" ]] || die 'Working tree is dirty, including untracked files.'
    branch=$(git symbolic-ref --quiet --short HEAD) || die 'Detached HEAD is not publishable.'
    [[ "$branch" == main ]] || die 'Publish from main only.'
    upstream=$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}') || die 'main needs origin/main as upstream.'
    [[ "$upstream" == origin/main ]] || die 'main must track origin/main.'
    head=$(git rev-parse --verify 'HEAD^{commit}')
    [[ "$(git rev-parse '@{upstream}')" == "$head" ]] || die 'HEAD differs from upstream; sync main separately (no automatic branch push).'
    remote_main=$(git ls-remote --exit-code origin refs/heads/main) || die 'Cannot verify live origin/main.'
    [[ "$remote_main" == "$head"$'\trefs/heads/main' ]] || die 'Live origin/main differs from HEAD.'
    version=$(awk -F '"' '/^\[package\]/ {package=1; next} /^\[/ {package=0} package && /^version = / {print $2; exit}' Cargo.toml)
    [[ "$tag" == "v$version" ]] || die 'Tag/version mismatch with Cargo.toml.'
    for file in LICENSE THIRD_PARTY_NOTICES.md Cargo.lock; do
        [[ -f "$file" && ! -L "$file" ]] || die "Required release review/distribution file missing: $file"
        git cat-file -e "HEAD:$file" || die "Release file must be committed: $file"
    done
    if git show-ref --verify --quiet "refs/tags/$tag"; then
        commit=$(git rev-parse --verify "refs/tags/$tag^{commit}") || die 'Local tag is not a commit.'
        [[ "$commit" == "$head" ]] || die 'Local tag mismatch: existing tag does not point to HEAD.'
    fi
    remote_tag_commit origin "$head" >/dev/null
}

check() {
    local permission
    local_checks
    # Without write access GitHub may hide a draft behind 404. Require the
    # repository identity and permission before interpreting release absence.
    permission=$(gh api --method GET "$api" --jq '[.full_name, .permissions.push] | @tsv') || die 'Cannot authenticate release repository access.'
    [[ "$permission" == "$repo"$'\ttrue' ]] || die 'Authenticated contents-write access is needed to see and resume drafts.'
    public_head=$(gh api --method GET "$api/git/ref/heads/main" --jq '.object.sha') \
        || die 'Cannot read community main.'
    [[ "$public_head" =~ ^[0-9a-f]{40}$ ]] || die 'Malformed community main ref.'
    remote_tag_commit "$destination" "$public_head" >/dev/null
    read_release
}

check
if [[ "$mode" == check ]]; then
    printf 'Read-only checks passed: %s at %s; remote release: %s.\n' "$tag" "$head" "$release_state"
    exit 0
fi

# Prevent two local invocations (including linked worktrees) from interleaving.
lock_dir=$(git rev-parse --git-common-dir)/moeka-publish.lock
mkdir "$lock_dir" 2>/dev/null || die 'Another publisher holds the local lock; investigate before removing it.'
lock=$lock_dir
original_head=$head
initial_state=$release_state
initial_id=$release_id
assets=(Moeka-macos-arm64.dmg Moeka-macos-x86_64.dmg
        Moeka-macos-arm64.zip Moeka-macos-x86_64.zip
        Moeka-windows-arm64.zip Moeka-windows-x86_64.zip
    Moeka-linux-arm64.zip Moeka-linux-x86_64.zip)
    [[ ! -L dist ]] || die 'Unsafe dist symlink.'
mkdir -p dist

if [[ "$initial_state" == absent ]]; then
    make publish-tools-check
    make signed-release
    make windows
    make linux
else
    printf 'Resuming draft %s without rebuilding; retaining original dist bytes.\n' "$release_id"
fi
# Builds may take hours: recheck everything before touching any refs or release.
check
[[ "$head" == "$original_head" ]] || die 'HEAD changed during the build.'
[[ "$release_state" == "$initial_state" ]] || die 'Release state changed during the build; retry after review.'
[[ "$release_id" == "$initial_id" ]] || die 'Draft was replaced; retry only after manual review.'
for asset in "${assets[@]:0:8}"; do
    [[ -s "dist/$asset" && ! -L "dist/$asset" ]] || die "Missing or unsafe artifact: dist/$asset"
done
if ! git show-ref --verify --quiet "refs/tags/$tag"; then
    git tag -a "$tag" "$head" -m "Release $tag" # New annotated tag only; no force.
fi
# The source tag stays in the private origin. The public community tag points
# to community's own main commit, so GitHub's automatic source archives contain
# only the public community repository rather than Moeka application source.
remote_ref=$(remote_tag_commit origin "$head")
if [[ -z "$remote_ref" ]]; then
    git push origin "refs/tags/$tag:refs/tags/$tag"
fi
remote_ref=$(remote_tag_commit origin "$head")
[[ -n "$remote_ref" ]] || die 'Private source tag was not visible after push.'
public_ref=$(remote_tag_commit "$destination" "$public_head")
if [[ -z "$public_ref" ]]; then
    gh api --method POST "$api/git/refs" -f ref="refs/tags/$tag" -f sha="$public_head" >/dev/null \
        || die 'Cannot create community release tag.'
fi
public_ref=$(remote_tag_commit "$destination" "$public_head")
[[ -n "$public_ref" ]] || die 'Community release tag was not visible after creation.'
read_release
if [[ "$initial_state" == draft ]]; then
    [[ "$release_id" == "$initial_id" ]] || die 'Original draft was removed or replaced; it will not be recreated.'
fi
if [[ "$release_state" == absent ]]; then
    notes=$(git for-each-ref "refs/tags/$tag" --format='%(contents)')
    [[ -n "$notes" ]] || notes="Release $tag"
    gh release create "$tag" --repo "$repo" --verify-tag --target "$public_head" \
        --title "$tag" --notes "$notes" --draft
    read_release
fi
[[ "$release_state" == draft ]] || die 'A draft is required before any upload.'
draft_id=$release_id

assert_draft() {
    read_release
    [[ "$release_state" == draft && "$release_id" == "$draft_id" ]] || die 'Draft disappeared or was replaced.'
}

load_assets() {
    local id name size digest state extra expected found seen='|'
    gh api --method GET --paginate "$api/releases/$draft_id/assets?per_page=100" \
        --jq '.[] | [.id, .name, .size, (.digest // "-"), .state] | @tsv' >"$tmp/assets" \
        || die 'Cannot inspect draft assets.'
    while IFS=$'\t' read -r id name size digest state extra; do
        [[ -n "$id" ]] || continue
        [[ "$id" =~ ^[0-9]+$ && "$size" =~ ^[0-9]+$ && "$state" == uploaded && -z "$extra" ]] || die 'Invalid or incomplete remote asset; review draft manually.'
        found=false
        for expected in "${assets[@]}"; do
            [[ "$name" != "$expected" ]] || found=true
        done
        [[ "$found" == true && "$seen" != *"|$name|"* ]] || die "Unexpected or duplicate remote asset: $name"
        seen="$seen$name|"
    done <"$tmp/assets"
}

verify_asset() {
    local file=$1 row id name size digest state bytes hash
    row=$(awk -F '\t' -v name="$file" '$2 == name {print}' "$tmp/assets")
    [[ -n "$row" ]] || die "Uploaded asset is missing: $file"
    IFS=$'\t' read -r id name size digest state <<< "$row"
    bytes=$(wc -c <"dist/$file" | tr -d '[:space:]')
    [[ "$bytes" == "$size" ]] || die "Remote size mismatch: $file"
    hash=$(shasum -a 256 "dist/$file" | awk '{print $1}')
    if [[ "$digest" != '-' ]]; then
        [[ "$digest" == "sha256:$hash" ]] || die "Remote digest mismatch: $file"
    else
        # Older assets may lack GitHub's digest. Size alone is not identity.
        gh api --method GET "$api/releases/assets/$id" -H 'Accept: application/octet-stream' >"$tmp/download" \
            || die "Cannot download asset for verification: $file"
        cmp -s "dist/$file" "$tmp/download" || die "Remote content mismatch: $file"
    fi
}

assert_draft
load_assets
for asset in "${assets[@]}"; do
    assert_draft
    if ! awk -F '\t' -v name="$asset" '$2 == name {found=1} END {exit !found}' "$tmp/assets"; then
        # No --clobber: a concurrent upload fails instead of overwriting bytes.
        gh release upload "$tag" --repo "$repo" "dist/$asset"
        load_assets
    fi
    verify_asset "$asset"
done
check
[[ "$head" == "$original_head" ]] || die 'HEAD changed during upload.'
assert_draft
load_assets
for asset in "${assets[@]}"; do verify_asset "$asset"; done
assert_draft
gh release edit "$tag" --repo "$repo" --draft=false --latest
printf 'Published %s in %s. Assets will not be replaced by this script.\n' "$tag" "$repo"
