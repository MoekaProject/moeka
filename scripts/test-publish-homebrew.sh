#!/usr/bin/env bash
# Offline tests use a disposable local tap and intercept all GitHub/network calls.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
export REAL_GIT
REAL_GIT=$(command -v git)
export GIT_AUTHOR_NAME='Homebrew Test' GIT_AUTHOR_EMAIL='test@example.invalid'
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
mkdir -p "$work/bin"
printf '#!/usr/bin/env bash\nexit 0\n' >"$work/bin/sleep"
chmod +x "$work/bin/sleep"
cat >"$work/bin/git" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "${GIT_PAGER:-}" == cat && "${PAGER:-}" == cat && "${GH_PAGER:-}" == cat ]] || {
    echo 'Publisher must disable all interactive pagers' >&2
    exit 98
}
printf 'git %s\n' "$*" >>"$FIXTURE/calls"
if [[ "$1" == clone ]]; then
    [[ "$*" == "clone --quiet --single-branch --branch main https://github.com/MoekaProject/homebrew-moeka.git "* ]] || exit 99
    if [[ "$SCENARIO" == clone-transient && ! -f "$FIXTURE/clone-retried" ]]; then
        touch "$FIXTURE/clone-retried"; mkdir -p "$7"; echo partial >"$7/partial"
        echo 'connection reset' >&2; exit 1
    fi
    "$REAL_GIT" clone --quiet --single-branch --branch main "$FIXTURE/remote.git" "$7"
elif [[ "$1" == push ]]; then
    [[ "$*" == 'push origin HEAD:refs/heads/main' ]] || exit 99
    [[ "$SCENARIO" != push-failure ]] || { echo 'push rejected' >&2; exit 1; }
    if [[ "$SCENARIO" == push-transient && ! -f "$FIXTURE/push-retried" ]]; then
        touch "$FIXTURE/push-retried"; echo 'TLS handshake timeout' >&2; exit 1
    fi
    "$REAL_GIT" "$@"
    if [[ "$SCENARIO" == push-response-lost ]]; then echo 'unexpected EOF' >&2; exit 1; fi
else
    "$REAL_GIT" "$@"
fi
MOCK
cat >"$work/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf 'gh %s\n' "$*" >>"$FIXTURE/calls"
if [[ "$*" == 'api --method GET repos/MoekaProject/community/releases/tags/v0.1.29' ]]; then
    [[ "$SCENARIO" != api-failure ]] || { echo 'API unavailable' >&2; exit 1; }
    if [[ "$SCENARIO" == api-transient && ! -f "$FIXTURE/api-retried" ]]; then
        touch "$FIXTURE/api-retried"; echo partial; echo 'unexpected EOF' >&2; exit 1
    fi
    cat "$FIXTURE/release.json"
elif [[ "$1 $2" == 'release download' ]]; then
    [[ "$3" == v0.1.29 && "$4 $5" == '--repo MoekaProject/community' &&
       "$6" == --pattern && "$8" == --dir ]] || exit 99
    [[ "$SCENARIO" != download-failure ]] || { echo 'download failed' >&2; exit 1; }
    if [[ "$SCENARIO" == download-transient && ! -f "$FIXTURE/download-retried" ]]; then
        touch "$FIXTURE/download-retried"; echo partial >"$9/$7"; echo 'connection reset' >&2; exit 1
    fi
    cp "$FIXTURE/$7" "$9/$7"
    if [[ "$SCENARIO" == corrupt-download ]]; then printf 'tampered' >>"$9/$7"; fi
else
    echo "UNEXPECTED gh: $*" >&2
    exit 99
fi
MOCK
chmod +x "$work/bin/git" "$work/bin/gh"
export PATH="$work/bin:$PATH"

run_case() {
    export SCENARIO=$1 FIXTURE="$work/$1"
    local expected=$2 mode=${3:-publish} tag=${4:-v0.1.29} filter=.
    mkdir -p "$FIXTURE/seed/Casks"
    : >"$FIXTURE/calls"
    printf 'signed arm DMG\n' >"$FIXTURE/Moeka-macos-arm64.dmg"
    printf 'signed intel DMG\n' >"$FIXTURE/Moeka-macos-x86_64.dmg"
    local arm intel
    arm=$(shasum -a 256 "$FIXTURE/Moeka-macos-arm64.dmg" | awk '{print $1}')
    intel=$(shasum -a 256 "$FIXTURE/Moeka-macos-x86_64.dmg" | awk '{print $1}')
    jq -n --arg arm "$arm" --arg intel "$intel" '{
        tag_name:"v0.1.29", draft:false, prerelease:false,
        assets:[
            {name:"Moeka-macos-arm64.dmg", state:"uploaded", size:15, digest:("sha256:" + $arm)},
            {name:"Moeka-macos-x86_64.dmg", state:"uploaded", size:17, digest:("sha256:" + $intel)}
        ]
    }' >"$FIXTURE/release.json"
    sed 's/version "0.1.29"/version "0.1.11"/' "$root/packaging/homebrew/moeka.rb" >"$FIXTURE/seed/Casks/moeka.rb"
    case "$SCENARIO" in
        draft) filter='.draft = true';;
        prerelease) filter='.prerelease = true';;
        wrong-tag) filter='.tag_name = "v0.1.28"';;
        missing-asset) filter='.assets |= .[:1]';;
        duplicate-asset) filter='.assets += [.assets[0]]';;
        missing-digest) filter='del(.assets[1].digest)';;
        bad-digest) filter='.assets[0].digest = "sha256:bad"';;
        pending-asset) filter='.assets[0].state = "new"';;
        wrong-size) filter='.assets[0].size += 1';;
        wrong-hash) filter='.assets[0].digest = .assets[1].digest';;
        downgrade) sed -i.bak 's/0.1.11/0.1.30/' "$FIXTURE/seed/Casks/moeka.rb";;
        format) sed -i.bak 's/sha256 arm:/sha256 other:/' "$FIXTURE/seed/Casks/moeka.rb";;
        same-version) sed -i.bak 's/0.1.11/0.1.29/' "$FIXTURE/seed/Casks/moeka.rb";;
        noop)
            sed -i.bak -E \
                -e 's/0.1.11/0.1.29/' \
                -e "s/(sha256 arm:   \")[0-9a-f]+/\\1$arm/" \
                -e "s/(intel: \")[0-9a-f]+/\\1$intel/" "$FIXTURE/seed/Casks/moeka.rb";;
    esac
    jq "$filter" "$FIXTURE/release.json" >"$FIXTURE/updated.json"
    mv "$FIXTURE/updated.json" "$FIXTURE/release.json"
    if [[ "$SCENARIO" == malformed ]]; then printf 'not json\n' >"$FIXTURE/release.json"; fi
    "$REAL_GIT" init --quiet --initial-branch=main "$FIXTURE/seed"
    "$REAL_GIT" -C "$FIXTURE/seed" add Casks/moeka.rb
    "$REAL_GIT" -C "$FIXTURE/seed" commit --quiet -m initial
    "$REAL_GIT" clone --quiet --bare "$FIXTURE/seed" "$FIXTURE/remote.git"
    local before after status=0
    before=$("$REAL_GIT" --git-dir="$FIXTURE/remote.git" rev-parse main)
    (cd "$FIXTURE" && GIT_PAGER=interactive-pager PAGER=interactive-pager GH_PAGER=interactive-pager \
        GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=pager.diff GIT_CONFIG_VALUE_0=true \
        bash "$root/scripts/publish-homebrew.sh" "$tag" "$mode") >"$FIXTURE/output" 2>&1 || status=$?
    if [[ "$expected" == success && "$status" != 0 || "$expected" == failure && "$status" == 0 ]]; then
        cat "$FIXTURE/output"
        echo "FAIL: $SCENARIO (exit $status)" >&2
        exit 1
    fi
    after=$("$REAL_GIT" --git-dir="$FIXTURE/remote.git" rev-parse main)
    if [[ "$SCENARIO" == publish || "$SCENARIO" == *-transient || "$SCENARIO" == push-response-lost ]]; then
        grep -q '^git --no-pager diff --check$' "$FIXTURE/calls"
        [[ "$after" != "$before" ]]
        "$REAL_GIT" --git-dir="$FIXTURE/remote.git" show main:Casks/moeka.rb >"$FIXTURE/published.rb"
        grep -q 'version "0.1.29"' "$FIXTURE/published.rb"
        grep -q "$arm" "$FIXTURE/published.rb"
        grep -q "$intel" "$FIXTURE/published.rb"
        grep -q 'strategy :github_releases' "$FIXTURE/published.rb"
        [[ $("$REAL_GIT" --git-dir="$FIXTURE/remote.git" diff-tree --no-commit-id --name-only -r main) == Casks/moeka.rb ]]
        if [[ "$SCENARIO" == push-response-lost ]]; then
            [[ "$(grep -c '^git push ' "$FIXTURE/calls")" == 1 ]]
        fi
    else
        [[ "$after" == "$before" ]]
        if [[ "$SCENARIO" != push-failure ]]; then
            ! grep -q '^git push ' "$FIXTURE/calls"
        fi
    fi
    if [[ "$SCENARIO" == protected || "$SCENARIO" == invalid-tag || "$SCENARIO" == invalid-mode ]]; then
        [[ ! -s "$FIXTURE/calls" ]]
    fi
    printf 'PASS: %s\n' "$SCENARIO"
}

run_case publish success
run_case check success check
run_case noop success
for scenario in api-transient clone-transient download-transient push-transient push-response-lost; do
    run_case "$scenario" success
done
for scenario in api-failure malformed draft prerelease wrong-tag missing-asset \
    duplicate-asset missing-digest bad-digest pending-asset wrong-size wrong-hash \
    download-failure corrupt-download downgrade format same-version push-failure; do
    run_case "$scenario" failure
done
run_case protected failure publish v0.1.12
run_case invalid-tag failure publish v0.1.29-beta
run_case invalid-mode failure invalid

cat >"$work/bin/bash" <<'MOCK'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >>"$FIXTURE/make-calls"
case "$*" in
    'scripts/publish-release.sh check v0.1.29 MoekaProject/community') ;;
    'scripts/publish-release.sh publish v0.1.29 MoekaProject/community')
        [[ "$SCENARIO" != release-fails ]];;
    'scripts/publish-release.sh publish-beta v0.1.29 MoekaProject/community') ;;
    'scripts/publish-homebrew.sh v0.1.29')
        [[ "$SCENARIO" != homebrew-fails ]];;
    *) echo "Unexpected publish command: $*" >&2; exit 99;;
esac
MOCK
chmod +x "$work/bin/bash"
export FIXTURE="$work/make-wiring"
mkdir -p "$FIXTURE"
ln -s "$root/Makefile" "$FIXTURE/Makefile"
for SCENARIO in wired release-fails homebrew-fails beta; do
    export SCENARIO
    : >"$FIXTURE/make-calls"
    goal=publish
    [[ "$SCENARIO" != beta ]] || goal=publish-beta
    status=0
    (cd "$FIXTURE" && make "$goal" v0.1.29) >"$FIXTURE/make-output" 2>&1 || status=$?
    if [[ "$SCENARIO" == wired || "$SCENARIO" == beta ]]; then
        [[ "$status" == 0 ]] || { cat "$FIXTURE/make-output"; exit 1; }
    else
        [[ "$status" != 0 ]] || { echo "Expected make failure: $SCENARIO" >&2; exit 1; }
    fi
    if [[ "$SCENARIO" == wired || "$SCENARIO" == homebrew-fails ]]; then
        grep -qx 'scripts/publish-homebrew.sh v0.1.29' "$FIXTURE/make-calls"
    else
        ! grep -q '^scripts/publish-homebrew.sh ' "$FIXTURE/make-calls"
    fi
    if [[ "$SCENARIO" == homebrew-fails ]]; then
        grep -q 'Retry only: make publish-homebrew TAG=v0.1.29' "$FIXTURE/make-output"
    fi
    printf 'PASS: make wiring %s\n' "$SCENARIO"
done
echo 'All Homebrew publishing tests passed.'
