#!/usr/bin/env bash
# Offline contract tests: every git, gh and make call is intercepted. No real
# repository, ref, network, credential, build, or release is touched.
set -euo pipefail
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$work/bin"
printf '#!/usr/bin/env bash\nexit 0\n' >"$work/bin/sleep"
chmod +x "$work/bin/sleep"
helper=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/publish-release.sh
export HEAD_FIXTURE=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
export OTHER_FIXTURE=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
export PUBLIC_FIXTURE=cccccccccccccccccccccccccccccccccccccccc
export PATH="$work/bin:$PATH"

cat >"$work/bin/git" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "${GIT_PAGER:-}" == cat && "${PAGER:-}" == cat && "${GH_PAGER:-}" == cat ]] || {
    echo 'Publisher must disable all interactive pagers' >&2
    exit 98
}
printf 'git %s\n' "$*" >>"$FIXTURE/calls"
case "$1" in
    status)
        if [[ "$SCENARIO" == status-error ]]; then exit 128; fi
        [[ "$SCENARIO" != dirty ]] || echo '?? untracked';;
    symbolic-ref) if [[ "$SCENARIO" == detached ]]; then exit 1; fi; echo "${BRANCH_FIXTURE:-main}";;
    rev-parse)
        case "$*" in
            *--git-common-dir*) echo "$FIXTURE/git-common";;
            *--abbrev-ref*) echo "${UPSTREAM_FIXTURE:-origin/main}";;
            *refs/tags/*) if [[ "$SCENARIO" == tag-mismatch ]]; then echo "$OTHER_FIXTURE"; else echo "$HEAD_FIXTURE"; fi;;
            *'@{upstream}'*) if [[ "$SCENARIO" == behind ]]; then echo "$OTHER_FIXTURE"; else echo "$HEAD_FIXTURE"; fi;;
            *) echo "$HEAD_FIXTURE";;
        esac;;
    ls-remote)
        if [[ "$SCENARIO" == git-transient && ! -f "$FIXTURE/git-retried" ]]; then
            touch "$FIXTURE/git-retried"; echo 'connection reset' >&2; exit 128
        fi
        if [[ "$SCENARIO" == git-network ]]; then exit 128; fi
        if [[ "$*" == *refs/heads/main* ]]; then
            if [[ "$SCENARIO" == stale-upstream ]]; then printf '%s\trefs/heads/main\n' "$OTHER_FIXTURE";
            else printf '%s\trefs/heads/main\n' "$HEAD_FIXTURE"; fi
        elif [[ "$SCENARIO" == remote-tag-mismatch ]]; then
            printf '%s\trefs/tags/v9.8.7\n' "$OTHER_FIXTURE"
        elif [[ "$*" == *MoekaProject/community* && -f "$FIXTURE/public-tag" ]]; then
            printf '%s\trefs/tags/v9.8.7\n' "$PUBLIC_FIXTURE"
        elif [[ "$*" != *MoekaProject/community* && -f "$FIXTURE/private-tag" ]]; then
            printf '%s\trefs/tags/v9.8.7\n%s\trefs/tags/v9.8.7^{}\n' "$OTHER_FIXTURE" "$HEAD_FIXTURE"
        fi;;
    cat-file) :;;
    show-ref) [[ -f "$FIXTURE/local-tag" || "$SCENARIO" == tag-mismatch || "$SCENARIO" == existing-tag ]];;
    tag)
        [[ "$*" == "tag -a v9.8.7 $HEAD_FIXTURE -m Release v9.8.7" ]] || exit 90
        touch "$FIXTURE/local-tag";;
    push)
        [[ "$*" == 'push origin refs/tags/v9.8.7:refs/tags/v9.8.7' && "$*" != *force* ]] || exit 91
        touch "$FIXTURE/private-tag"
        if [[ "$SCENARIO" == push-response-lost && ! -f "$FIXTURE/push-lost" ]]; then
            touch "$FIXTURE/push-lost"; echo 'connection reset' >&2; exit 1
        fi;;
    for-each-ref) echo 'Release v9.8.7';;
    *) echo "UNEXPECTED git: $*" >&2; exit 99;;
esac
MOCK

cat >"$work/bin/make" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf 'make %s\n' "$*" >>"$FIXTURE/calls"
case "$*" in
    publish-tools-check) ;;
    signed-release)
        for arch in arm64 x86_64; do
            printf 'signed DMG %s\n' "$arch" >"dist/Moeka-macos-$arch.dmg"
            printf 'signed ZIP %s\n' "$arch" >"dist/Moeka-macos-$arch.zip"
        done;;
    windows|linux)
        for arch in arm64 x86_64; do printf '%s %s\n' "$1" "$arch" >"dist/Moeka-$1-$arch.zip"; done
        if [[ "$1" == linux && "$SCENARIO" == published-during-build ]]; then echo published >"$FIXTURE/state"; fi;;
    *) echo "UNEXPECTED make: $*" >&2; exit 99;;
esac
MOCK

cat >"$work/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf 'gh %s\n' "$*" >>"$FIXTURE/calls"
state=$(cat "$FIXTURE/state")
api=repos/MoekaProject/community
if [[ "$1" == api ]]; then
    if [[ "$*" == *'--method POST'* && "$*" == *"$api/git/refs"* ]]; then
        [[ "$*" == *'ref=refs/tags/v9.8.7'* && "$*" == *"sha=$PUBLIC_FIXTURE"* ]] || exit 97
        touch "$FIXTURE/public-tag"
        if [[ "$SCENARIO" == public-tag-response-lost ]]; then echo 'connection reset' >&2; exit 1; fi
        exit 0
    fi
    shift
    endpoint= include=false query=
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --method) [[ "$2" == GET ]] || exit 97; shift 2;;
            --include) include=true; shift;;
            --paginate) shift;;
            --jq) query=$2; shift 2;;
            -H) shift 2;;
            *) endpoint=$1; shift;;
        esac
    done
    case "$endpoint" in
        "$api")
            if [[ "$SCENARIO" == no-permission ]]; then printf 'MoekaProject/community\tfalse\n';
            else printf 'MoekaProject/community\ttrue\n'; fi;;
        "$api/git/ref/heads/main") printf '%s\n' "$PUBLIC_FIXTURE";;
        "$api/releases/tags/v9.8.7")
            [[ "$include" == true && "$query" == '[.id, .draft, .tag_name, .target_commitish] | @tsv' ]] || exit 98
            if [[ "$SCENARIO" == read-transient && ! -f "$FIXTURE/read-retried" ]]; then
                touch "$FIXTURE/read-retried"; printf 'partial\n'; echo 'TLS handshake timeout' >&2; exit 1
            fi
            if [[ "$SCENARIO" == read-exhausted ]]; then echo 'unexpected EOF' >&2; exit 1; fi
            if [[ "$SCENARIO" == removed-draft && "$(grep -c -- '--include' "$FIXTURE/calls")" -ge 3 ]]; then
                state=absent
                echo absent >"$FIXTURE/state"
            fi
            case "$SCENARIO" in
                network) echo 'connection reset; text says HTTP 404' >&2; exit 1;;
                forbidden) printf 'HTTP/2.0 403 Forbidden\r\n\r\n'; exit 1;;
                server-error) printf 'HTTP/2.0 500 Server Error\r\n\r\n'; exit 1;;
                malformed) printf 'HTTP/2.0 200 OK\r\n\r\ngarbage\n'; exit 0;;
                hidden-draft) printf 'HTTP/2.0 404 Not Found\r\n\r\n'; exit 1;;
            esac
            if [[ "$state" == absent ]]; then printf 'HTTP/2.0 404 Not Found\r\n\r\n'; exit 1; fi
            printf 'HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n'
            draft=true; [[ "$state" != published ]] || draft=false
            target=$PUBLIC_FIXTURE; [[ "$SCENARIO" != draft-target-mismatch ]] || target=$OTHER_FIXTURE
            printf '123\t%s\tv9.8.7\t%s\n' "$draft" "$target";;
        "$api/releases?per_page=100")
            if [[ "$SCENARIO" == list-network ]]; then exit 1; fi
            if [[ "$SCENARIO" == list-transient && ! -f "$FIXTURE/list-retried" ]]; then
                touch "$FIXTURE/list-retried"; echo partial; echo 'unexpected EOF' >&2; exit 1
            fi
            if [[ "$SCENARIO" == hidden-draft ]]; then printf '123\ttrue\tv9.8.7\t%s\n' "$PUBLIC_FIXTURE"; fi;;
        "$api/releases/123")
            draft=true; [[ "$state" != published ]] || draft=false
            prerelease=false; [[ "$SCENARIO" != edit-lost-beta ]] || prerelease=true
            printf '123\tv9.8.7\t%s\t%s\t%s\n' "$draft" "$prerelease" "$PUBLIC_FIXTURE";;
        "$api/releases/latest")
            if [[ "$SCENARIO" == edit-lost-not-latest ]]; then echo 456; else echo 123; fi;;
        "$api/releases/123/assets?per_page=100")
            if [[ "$SCENARIO" == extra-asset ]]; then printf '9\tunexpected.zip\t1\t-\tuploaded\n'; fi
            i=0
            for file in "$FIXTURE"/remote/*; do
                [[ -f "$file" ]] || continue
                i=$((i + 1))
                size=$(wc -c <"$file" | tr -d '[:space:]')
                digest="sha256:$(shasum -a 256 "$file" | awk '{print $1}')"
                [[ "$SCENARIO" != digest-mismatch ]] || digest="sha256:$OTHER_FIXTURE"
                [[ "$SCENARIO" != size-mismatch ]] || size=$((size + 1))
                [[ "$SCENARIO" != no-digest ]] || digest=-
                printf '%s\t%s\t%s\t%s\tuploaded\n' "$i" "${file##*/}" "$size" "$digest"
            done;;
        "$api/releases/assets/"*)
            id=${endpoint##*/}; i=0
            for file in "$FIXTURE"/remote/*; do
                [[ -f "$file" ]] || continue
                i=$((i + 1)); if [[ "$i" == "$id" ]]; then cat "$file"; exit 0; fi
            done
            exit 1;;
        *) echo "UNEXPECTED gh API: $endpoint" >&2; exit 99;;
    esac
elif [[ "$1" == release ]]; then
    [[ "$*" != *--clobber* && "$*" != *delete* ]] || exit 96
    case "$2" in
        create)
            [[ "$state" == absent && "$*" == *--draft* && "$*" == *--verify-tag* && "$*" == *"--target $PUBLIC_FIXTURE"* ]] || exit 95
            if [[ "$SCENARIO" == create-unknown ]]; then echo 'connection reset' >&2; exit 1; fi
            echo draft >"$FIXTURE/state"
            if [[ "$SCENARIO" == create-response-lost ]]; then echo 'unexpected EOF' >&2; exit 1; fi;;
        upload)
            [[ "$state" == draft ]] || exit 94
            file=${!#}
            if [[ "$SCENARIO" == interrupted && -f "$FIXTURE/remote/Moeka-macos-arm64.dmg" ]]; then exit 1; fi
            if [[ "$SCENARIO" == upload-transient && ! -f "$FIXTURE/upload-retried" ]]; then
                touch "$FIXTURE/upload-retried"; echo 'connection reset' >&2; exit 1
            fi
            [[ ! -e "$FIXTURE/remote/${file##*/}" ]] || exit 93
            cp "$file" "$FIXTURE/remote/"
            if [[ "$SCENARIO" == upload-lost-corrupt && ! -f "$FIXTURE/upload-lost" ]]; then
                touch "$FIXTURE/upload-lost"; printf corrupted >"$FIXTURE/remote/${file##*/}"
                echo 'unexpected EOF' >&2; exit 1
            fi
            if [[ "$SCENARIO" == upload-response-lost && ! -f "$FIXTURE/upload-lost" ]]; then
                touch "$FIXTURE/upload-lost"; echo 'unexpected EOF' >&2; exit 1
            fi;;
        edit)
            [[ "$state" == draft && "$*" == *--draft=false* ]] || exit 92
            if [[ "$SCENARIO" == beta ]]; then
                [[ "$*" == *--prerelease* && "$*" == *--latest=false* ]] || exit 92
            else
                [[ "$*" == *--latest && "$*" != *--prerelease* ]] || exit 92
            fi
            if [[ "$SCENARIO" == edit-transient && ! -f "$FIXTURE/edit-retried" ]]; then
                touch "$FIXTURE/edit-retried"; echo 'connection reset' >&2; exit 1
            fi
            echo published >"$FIXTURE/state"
            if [[ "$SCENARIO" == edit-response-lost || "$SCENARIO" == edit-lost-not-latest ]]; then
                echo 'unexpected EOF' >&2; exit 1
            fi;;
        *) echo "UNEXPECTED gh release: $*" >&2; exit 99;;
    esac
else
    echo "UNEXPECTED gh: $*" >&2; exit 99
fi
MOCK
chmod +x "$work/bin/"*

count=0
fixture() {
    count=$((count + 1))
    export FIXTURE="$work/case-$count" SCENARIO=$1
    unset BRANCH_FIXTURE UPSTREAM_FIXTURE
    mkdir -p "$FIXTURE/"{scripts,packaging,git-common,remote}
    cp "$helper" "$FIXTURE/scripts/publish-release.sh"
    cp "${helper%/*}/release-network.sh" "$FIXTURE/scripts/release-network.sh"
    printf '[package]\nversion = "9.8.7"\n' >"$FIXTURE/Cargo.toml"
    touch "$FIXTURE/LICENSE" "$FIXTURE/THIRD_PARTY_NOTICES.md" "$FIXTURE/Cargo.lock" "$FIXTURE/calls"
    echo absent >"$FIXTURE/state"
}
invoke() { bash "$FIXTURE/scripts/publish-release.sh" "$@" >"$FIXTURE/output" 2>&1; }
fail() { echo "FAIL: $*" >&2; cat "$FIXTURE/output" >&2; exit 1; }
reject() {
    local message=$1; shift
    if invoke "$@"; then fail "Unexpected success: $SCENARIO"; fi
    grep -Fq "$message" "$FIXTURE/output" || fail "Missing refusal: $message"
    if grep -Eq '^(git (push|tag )|gh release |make )' "$FIXTURE/calls"; then fail "Refusal had side effects: $SCENARIO"; fi
    printf 'PASS refusal: %s\n' "$SCENARIO"
}

fixture protected; reject 'v0.1.12 is protected' publish v0.1.12
fixture wrong-repo; reject 'must stay' publish v9.8.7 other/repo
fixture malformed-tag; reject 'stable vX.Y.Z' check v9.8.7-rc1
fixture dirty; reject 'Working tree is dirty' publish v9.8.7
fixture status-error; reject 'Cannot inspect working tree' publish v9.8.7
fixture detached; reject 'Detached HEAD' check v9.8.7
fixture branch; export BRANCH_FIXTURE=feature; reject 'main only' check v9.8.7
fixture upstream; export UPSTREAM_FIXTURE=imeoer/main; reject 'track origin/main' check v9.8.7
fixture behind; reject 'differs from upstream' check v9.8.7
fixture stale-upstream; reject 'Live origin/main' check v9.8.7
fixture version; reject 'Tag/version mismatch' check v9.8.8
fixture tag-mismatch; reject 'Local tag mismatch' publish v9.8.7
fixture remote-tag-mismatch; reject 'Remote tag mismatch' publish v9.8.7
fixture git-network; reject 'Cannot verify live origin/main' publish v9.8.7
fixture published; echo published >"$FIXTURE/state"; reject 'already published' publish v9.8.7
fixture network; reject 'Cannot determine release state' publish v9.8.7
fixture forbidden; reject 'Cannot determine release state' publish v9.8.7
fixture server-error; reject 'Cannot determine release state' publish v9.8.7
fixture malformed; reject 'Malformed release response' publish v9.8.7
fixture list-network; reject 'Cannot list drafts' publish v9.8.7
fixture no-permission; reject 'contents-write access' publish v9.8.7
fixture draft-target-mismatch; echo draft >"$FIXTURE/state"; reject 'draft target' publish v9.8.7
fixture read-exhausted; reject 'Cannot determine release state' check v9.8.7
[[ "$(grep -c -- '--include' "$FIXTURE/calls")" == 3 ]] || fail 'Read retry must stop after three attempts'
fixture forbidden; reject 'Cannot determine release state' check v9.8.7
[[ "$(grep -c -- '--include' "$FIXTURE/calls")" == 1 ]] || fail '403 must not be retried'
fixture server-error; reject 'Cannot determine release state' check v9.8.7
[[ "$(grep -c -- '--include' "$FIXTURE/calls")" == 3 ]] || fail 'HTTP 500 must be retried at most three times'

for scenario in read-transient list-transient git-transient; do
    fixture "$scenario"
    invoke check v9.8.7 || fail "Transient read did not recover: $scenario"
    printf 'PASS bounded transient read: %s\n' "$scenario"
done
fixture upload-lost-corrupt
if invoke publish v9.8.7; then fail 'Corrupt uploaded asset accepted'; fi
[[ "$(grep -c '^gh release upload ' "$FIXTURE/calls")" == 1 ]] || fail 'Corrupt asset was re-uploaded'
grep -q 'Remote size mismatch' "$FIXTURE/output" || fail 'Missing corrupted asset refusal'
fixture edit-lost-not-latest
if invoke publish v9.8.7; then fail 'Unconfirmed latest state accepted'; fi
[[ "$(grep -c '^gh release edit ' "$FIXTURE/calls")" == 1 ]] || fail 'Published release was edited again'
grep -q 'not the latest release' "$FIXTURE/output" || fail 'Missing latest release refusal'
echo 'PASS lost-response integrity and latest-state checks'
fixture create-unknown
if invoke publish v9.8.7; then fail 'Unknown draft creation outcome accepted'; fi
[[ "$(grep -c '^gh release create ' "$FIXTURE/calls")" == 1 ]] || fail 'Uncertain draft creation must never be repeated'
grep -q 'Draft creation outcome is uncertain' "$FIXTURE/output" || fail 'Missing uncertain creation error'
echo 'PASS uncertain draft creation stops safely'

for scenario in push-response-lost public-tag-response-lost create-response-lost upload-transient upload-response-lost edit-transient edit-response-lost; do
    fixture "$scenario"
    invoke publish v9.8.7 || fail "Reconciled publish failed: $scenario"
    [[ "$(cat "$FIXTURE/state")" == published ]] || fail 'Release did not publish'
    [[ "$(grep -c '^gh release create ' "$FIXTURE/calls")" == 1 ]] || fail 'Draft creation repeated'
    if [[ "$scenario" == upload-response-lost ]]; then
        [[ "$(grep -c '^gh release upload ' "$FIXTURE/calls")" == 8 ]] || fail 'Uploaded asset was re-uploaded after response loss'
    fi
    if [[ "$scenario" == edit-response-lost ]]; then
        [[ "$(grep -c '^gh release edit ' "$FIXTURE/calls")" == 1 ]] || fail 'Published release was edited again'
    fi
    if [[ "$scenario" == public-tag-response-lost ]]; then
        [[ "$(grep -c '^gh api --method POST .*git/refs' "$FIXTURE/calls")" == 1 ]] || fail 'Existing public tag was recreated'
    fi
    printf 'PASS reconciled transient write: %s\n' "$scenario"
done

for scenario in absent existing-tag hidden-draft; do
    fixture "$scenario"
    invoke check v9.8.7 || fail "Read-only check: $scenario"
    if grep -Eq '^(git (push|tag )|gh release |make )' "$FIXTURE/calls"; then fail 'Read-only check mutated state'; fi
    printf 'PASS read-only: %s\n' "$scenario"
done

fixture published-during-build
if invoke publish v9.8.7; then fail 'Published-during-build accepted'; fi
grep -Fq 'already published' "$FIXTURE/output" || fail 'Missing late refusal'
if grep -Eq '^(git (push|tag )|gh release )' "$FIXTURE/calls"; then fail 'Late check mutated refs/release'; fi
echo 'PASS build-to-publish recheck'

for scenario in success no-digest existing-tag; do
    fixture "$scenario"
    invoke publish v9.8.7 || fail "Fresh publish: $scenario"
    [[ "$(cat "$FIXTURE/state")" == published ]] || fail 'Draft not published'
    [[ "$(grep -c '^gh release upload ' "$FIXTURE/calls")" == 8 ]] || fail 'Expected eight binary assets'
    [[ "$(grep -c '^gh release create ' "$FIXTURE/calls")" == 1 ]] || fail 'Expected one draft'
    [[ "$(grep -c '^gh release edit ' "$FIXTURE/calls")" == 1 ]] || fail 'Expected one final publish'
    [[ ! -e "$FIXTURE/dist/Moeka-v9.8.7-source.tar.gz" && ! -e "$FIXTURE/dist/SHA256SUMS" ]] || fail 'Source-related assets must not be created'
    if grep -Eq '^git push .*MoekaProject/community|^git archive ' "$FIXTURE/calls"; then fail 'Private source was exposed to community'; fi
    [[ -f "$FIXTURE/public-tag" ]] || fail 'Community binary release tag was not created'
    if [[ "$scenario" == existing-tag ]] && grep -q '^git tag ' "$FIXTURE/calls"; then fail 'Existing HEAD tag was recreated'; fi
    if [[ "$scenario" == no-digest ]] && ! grep -q '/releases/assets/' "$FIXTURE/calls"; then fail 'Missing digest fallback'; fi
    : >"$FIXTURE/calls"
    reject 'already published' publish v9.8.7
    printf 'PASS fresh publish + immutability: %s\n' "$scenario"
done

fixture beta
invoke publish-beta v9.8.7 || fail 'Beta publish failed'
[[ "$(cat "$FIXTURE/state")" == published ]] || fail 'Beta draft not published'
[[ "$(grep -c '^gh release edit ' "$FIXTURE/calls")" == 1 ]] || fail 'Expected one final beta publish'
grep -q '^gh release edit .*--prerelease.*--latest=false' "$FIXTURE/calls" \
    || fail 'Beta release was not marked prerelease and non-latest'
echo 'PASS beta prerelease publish'

for scenario in size-mismatch digest-mismatch extra-asset; do
    fixture "$scenario"
    if invoke publish v9.8.7; then fail "Remote integrity failure accepted: $scenario"; fi
    [[ "$(cat "$FIXTURE/state")" == draft ]] || fail 'Integrity failure did not remain draft'
    if grep -q '^gh release edit ' "$FIXTURE/calls"; then fail 'Invalid assets published'; fi
    printf 'PASS remote integrity refusal: %s\n' "$scenario"
done

fixture interrupted
if invoke publish v9.8.7; then fail 'Interrupted upload succeeded'; fi
[[ "$(cat "$FIXTURE/state")" == draft ]] || fail 'Interrupted upload not left draft'
original=$(shasum -a 256 "$FIXTURE/remote/Moeka-macos-arm64.dmg")
# A changed local artifact must refuse resume before any ref/release writes.
cp "$FIXTURE/dist/Moeka-macos-arm64.dmg" "$FIXTURE/original.dmg"
echo changed >>"$FIXTURE/dist/Moeka-macos-arm64.dmg"
: >"$FIXTURE/calls"
export SCENARIO=changed-draft
reject 'Remote size mismatch' publish v9.8.7
cp "$FIXTURE/original.dmg" "$FIXTURE/dist/Moeka-macos-arm64.dmg"
: >"$FIXTURE/calls"
export SCENARIO=removed-draft
reject 'Original draft was removed or replaced' publish v9.8.7
echo draft >"$FIXTURE/state"
: >"$FIXTURE/calls"
export SCENARIO=resume
invoke publish v9.8.7 || fail 'Draft resume failed'
[[ "$(cat "$FIXTURE/state")" == published ]] || fail 'Resume did not publish'
[[ "$(grep -c '^gh release upload ' "$FIXTURE/calls")" == 7 ]] || fail 'Resume should upload only missing assets'
[[ "$original" == "$(shasum -a 256 "$FIXTURE/remote/Moeka-macos-arm64.dmg")" ]] || fail 'Original draft asset changed'
if grep -Eq '^(make |git (push|tag )|gh release create )' "$FIXTURE/calls"; then fail 'Resume rebuilt, retagged, or recreated release'; fi
echo 'PASS interrupted upload, changed-dist refusal, and no-overwrite draft resume'
notary_fixture="$work/notary"
mkdir -p "$notary_fixture/bin" "$notary_fixture/Moeka.app"
cp "${helper%/*}/sign_notarize_package.sh" "$notary_fixture/sign_notarize_package.sh"
cp "${helper%/*}/release-network.sh" "$notary_fixture/release-network.sh"
cat >"$notary_fixture/bin/mock" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
tool=${0##*/}
printf '%s %s\n' "$tool" "$*" >>"$NOTARY_CALLS"
case "$tool" in
    xcrun)
        if [[ "$1 $2" == 'notarytool submit' ]]; then
            [[ " $* " == *' --no-s3-acceleration '* && " $* " == *' --wait '* && " $* " == *' --timeout 15m '* ]] || exit 97
            if [[ "${NOTARY_SCENARIO:-}" == submit-lost || "${NOTARY_SCENARIO:-}" == wait-invalid ]]; then
                echo '  id: 01234567-89ab-cdef-0123-456789abcdef'
                echo 'unexpected EOF' >&2
                exit 1
            fi
            if [[ "${NOTARY_SCENARIO:-}" == submit-unknown ]]; then
                echo 'connection reset' >&2; exit 1
            fi
            if [[ "$NOTARY_RESULT" == 0 ]]; then echo '  status: Accepted'; fi
            exit "$NOTARY_RESULT"
        fi
        if [[ "$1 $2" == 'notarytool wait' ]]; then
            [[ " $* " == *' --timeout 15m '* && " $* " == *' --output-format json '* ]] || exit 97
            if [[ "${NOTARY_SCENARIO:-}" == submit-lost && ! -f "$NOTARY_CALLS.wait" ]]; then
                touch "$NOTARY_CALLS.wait"; echo partial; echo 'TLS handshake timeout' >&2; exit 1
            fi
            status=Accepted; [[ "${NOTARY_SCENARIO:-}" != wait-invalid ]] || status=Invalid
            printf '{"id":"01234567-89ab-cdef-0123-456789abcdef","status":"%s"}\n' "$status"
            exit 0
        fi
        if [[ "$1 $2" == 'stapler validate' && "${NOTARY_SCENARIO:-}" == validate-transient && ! -f "$NOTARY_CALLS.validate" ]]; then
            touch "$NOTARY_CALLS.validate"; echo 'network connection was lost' >&2; exit 68
        fi
        if [[ "$1 $2" == 'stapler staple' && "${NOTARY_SCENARIO:-}" == staple-transient && ! -f "$NOTARY_CALLS.staple" ]]; then
            touch "$NOTARY_CALLS.staple"; echo 'connection reset' >&2; exit 68
        fi
        [[ "$1 $2" == 'stapler staple' || "$1 $2" == 'stapler validate' ]] || exit 98;;
    ditto) touch "${!#}";;
    create_dmg.sh) touch "$2";;
    codesign|spctl) :;;
    *) exit 99;;
esac
MOCK
chmod +x "$notary_fixture/bin/mock"
for tool in xcrun ditto codesign spctl; do
    cp "$notary_fixture/bin/mock" "$notary_fixture/bin/$tool"
done
cp "$notary_fixture/bin/mock" "$notary_fixture/create_dmg.sh"
for scenario in submit-lost validate-transient staple-transient wait-invalid submit-unknown; do
    calls="$notary_fixture/calls-$scenario"
    output="$notary_fixture/output-$scenario"
    status=0
    NOTARY_CALLS="$calls" NOTARY_RESULT=0 NOTARY_SCENARIO="$scenario" PATH="$notary_fixture/bin:$PATH" \
        bash "$notary_fixture/sign_notarize_package.sh" \
        "$notary_fixture/Moeka.app" "$output/staging" "$output/Moeka.dmg" \
        "$output/Moeka.zip" Moeka Moeka unused-background fake-identity fake-profile \
        >"$notary_fixture/log-$scenario" 2>&1 || status=$?
    if [[ "$scenario" == wait-invalid || "$scenario" == submit-unknown ]]; then
        [[ "$status" != 0 && ! -e "$output" ]] || { cat "$notary_fixture/log-$scenario"; exit 1; }
        ! grep -q '^xcrun stapler' "$calls"
    else
        [[ "$status" == 0 && -f "$output/Moeka.dmg" && -f "$output/Moeka.zip" ]] \
            || { cat "$notary_fixture/log-$scenario"; exit 1; }
    fi
    [[ "$(grep -c '^xcrun notarytool submit ' "$calls")" == 1 ]] || { echo 'FAIL: duplicate notarization submission' >&2; exit 1; }
    if [[ "$scenario" == submit-lost ]]; then
        [[ "$(grep -c '^xcrun notarytool wait ' "$calls")" == 2 ]]
    fi
    printf 'PASS notarization recovery: %s\n' "$scenario"
done
for notary_result in 0 1 138; do
    calls="$notary_fixture/calls-$notary_result"
    output="$notary_fixture/output-$notary_result"
    if NOTARY_CALLS="$calls" NOTARY_RESULT="$notary_result" PATH="$notary_fixture/bin:$PATH" \
        bash "$notary_fixture/sign_notarize_package.sh" \
        "$notary_fixture/Moeka.app" "$output/staging" "$output/Moeka.dmg" \
        "$output/Moeka.zip" Moeka Moeka unused-background fake-identity fake-profile \
        >"$notary_fixture/log-$notary_result" 2>&1; then
        [[ "$notary_result" == 0 ]] || { echo 'FAIL: notarization failure was ignored' >&2; exit 1; }
        [[ -f "$output/Moeka.dmg" && -f "$output/Moeka.zip" ]]
        grep -q 'xcrun stapler staple' "$calls"
        grep -q 'xcrun stapler validate' "$calls"
        grep -q 'spctl --assess' "$calls"
    else
        notary_exit=$?
        [[ "$notary_result" != 0 && "$notary_exit" == "$notary_result" ]] || {
            cat "$notary_fixture/log-$notary_result" >&2; exit 1;
        }
        if grep -Eq 'stapler|spctl|create_dmg.sh' "$calls" || [[ -e "$output" ]]; then
            echo 'FAIL: packaging continued after notarization failure' >&2; exit 1
        fi
    fi
    printf 'PASS notarization pipeline: exit %s\n' "$notary_result"
done
printf 'All offline release safety tests passed (%s publishing fixtures + 8 notarization fixtures).\n' "$count"
