# Release safety and local publishing

## Channels

Official GitHub Releases remain in **MoekaProject/community**. The supported
official release entry points are local `make publish vX.Y.Z` and
`make publish-beta vX.Y.Z` on the
maintainer's macOS signing machine. **v0.1.12 is permanently refused by the new
helper:** do not modify its tag, release, or assets and do not republish it.
Use a reviewed, bumped version for the next release.

[The candidate workflow](../.github/workflows/release.yml) is manual-only
(`workflow_dispatch`) and builds its selected ref. It retains macOS, Windows,
and Linux arm64/x86_64 candidates as Actions artifacts for 14 days. These are
unsigned, not notarized, not approved for distribution, and not public GitHub
Releases. It has `contents: read`, does not persist checkout credentials, and
has no release token, signing/notary secrets, tag trigger, or Homebrew update.
GitHub's internal artifact service authentication is still used by
`upload-artifact`; no extra publishing secret is needed.

Official releases currently publish binaries only. The application source
repository and its release tag remain private. The public `community` tag is
created against that repository's own `main` commit, so GitHub's automatic
source archives do not contain Moeka application source. Do not push the private
source tag or its reachable history to `MoekaProject/community`.

Packaging requires LICENSE and
THIRD_PARTY_NOTICES.md; an optional licenses/ directory is copied when present.
Missing notices fail packaging rather than silently producing incomplete
distributions. The macOS app (including the separate MAS bundle) carries these
in Resources; Windows and Linux packages carry them alongside the executable.
MAS packaging remains a separate existing channel, not an approval of App Store
terms compatibility or permission to distribute an unresolved GPL build.

## Required review: licensing and distribution

Before creating approval, a human must review at least:

- The GPL-3.0-or-later grant, contributor provenance, modifications and notices.
- All vendored, registry and Git dependencies, optional/platform features,
  fonts, icons, embedded resources, and bundled Linux shared libraries.
- Compatibility and the complete notices/license texts for actual shipped
  binaries, not just the top-level Rust package.
- The legal basis for binary distribution, dependency source availability and
  exact versions, build/install scripts, toolchain inputs, and any applicable
  source or Installation Information obligations.
- The private source/history and release artifacts for credentials or material
  without redistribution rights. Rotate exposed secrets; removing their current
  files does not remove them from Git history.

## Preflight and official release procedure

1. Complete the review above, bump the package version, and update/commit the
  lockfile separately. Run CI
   and review its results. Authenticate local Git and GitHub CLI without
   putting credentials in source or command history.
2. Synchronize `main` separately through the normal review/push process. It
   must track `origin/main`, and local HEAD, the tracking ref and a live
   `git ls-remote origin refs/heads/main` must agree. Publishing never pushes
   `main`. Verify the configured origin, including any push URL, is correct.
3. Run `make publish-check TAG=vX.Y.Z`. This is read-only: no fetch, build,
   signing, ref creation, upload or release edit. It checks the clean tree
   (including untracked files), stable tag syntax/version, main/upstream,
  committed license/notices/lockfile, and local and remote tag
   commits. An existing lightweight or annotated tag at HEAD is accepted.
   Any other commit is refused, never retagged.
4. Preflight authenticates access to MoekaProject/community. Only an actual
   HTTP 404 from the tag endpoint, followed by a successful paginated release
   listing, means absence. The listing also finds hidden drafts. Network,
   authentication, permission, malformed response and non-404 HTTP errors
   fail closed. A published release is refused **before expensive builds**.
5. After review, run `make publish vX.Y.Z` for a stable release or
  `make publish-beta vX.Y.Z` for a GitHub prerelease (not for v0.1.12). A fresh release
   checks signing/cross-build prerequisites, then uses the existing signed and
   notarized macOS pipeline plus Windows/Linux cross builds. Keep the default
   Moeka names and dist layout; the official asset allowlist is fixed, not a
   wildcard over potentially stale files. Do not override build commands or
   paths to reuse candidate binaries.
6. The helper repeats preflight after builds, checks unchanged HEAD, and creates
  an annotated source tag **only if absent**. That tag is pushed only to the
  private origin. A lightweight tag with the same name is created in the public
  community repository against community `main`; no Moeka source objects are
  pushed there. Existing remote tags are checked, not overwritten.
7. The helper creates a **draft before any upload** against community `main`,
   or resumes that same draft. It uploads eight binary archives (two macOS DMGs
   and six platform ZIPs). Unknown/duplicate/incomplete remote assets
   are refused. Existing asset names are never overwritten or deleted.
8. Every asset must match local byte size and GitHub's SHA-256 digest when
   supplied. If no digest is available, the helper downloads and byte-compares
   the asset; size alone is not sufficient. After a final recheck of local
   state, all assets and the manifest, it publishes the draft with `--latest`.

Git authentication must allow the non-forced tag pushes to both repositories;
GitHub CLI needs release write access to MoekaProject/community. Repository
permissions reported by the API do not prove that every fine-grained token
scope or branch/tag protection rule will permit a later write: denial stops
the process safely. No credentials are committed or configured by this helper.
No Homebrew upload/update is performed.

## Failure, retry and concurrency

- On failure, tags and draft assets already created are left intact. There is
  no automatic rollback, delete, force-push, release recreation or clobber.
- Preserve the entire original dist directory securely until publishing
  finishes. On retry an existing draft skips builds, verifies existing remote
  binary assets and uploads only missing ones. Rebuilt signed/notarized bytes often
  differ; do not overwrite original outputs to resume a draft.
- Mismatches, unexpected assets or a draft targeting a different commit require
  human investigation. Do not delete published releases or tags to work around
  a refusal. A published release cannot be resumed, even with identical bytes.
- A local lock in the Git common directory prevents parallel invocations from
  the same clone/worktrees. An interrupted process normally removes the lock;
  after a crash, investigate before manually removing a stale lock.
- GitHub does not provide an atomic transaction spanning tag pushes, upload
  and publication. Rechecks reduce but cannot eliminate races with other
  machines, direct API clients, or manual edits. Use a **single designated
  publisher**, restricted write access and repository tag/immutable-release
  protection where available. Never publish a draft manually during upload.
  The helper never overwrites any asset, but a concurrent external publisher
  could expose a partially uploaded draft between checks.
- Signing and notarization remain in the existing local macOS scripts; the
  helper's tests do not validate Apple services, hosted runners or binary
  functionality.

## CI, updates and safe tests

[CI](../.github/workflows/ci.yml) uses read-only repository permissions, retains
the existing platform matrices, runs Rust tests and Clippy with
`--release --locked`, and runs the asset checker and offline release tests.
It scans full reachable Git history with the official Gitleaks v8.24.3 CLI,
with redacted output and no organization-only action/license token. A finding
fails CI and needs investigation, not blanket suppression. No scanner proves
that all secrets or provenance issues have been found.

Actions are pinned to full commits verified with `git ls-remote` on 2026-09-05
(annotated tags use their peeled commit). Gitleaks' Go module v8.24.3 resolves
to upstream commit 107a41827bb6698dbbff756b2300537774a1d84c and is verified by
the Go module checksum mechanism. [Dependabot](../.github/dependabot.yml)
checks Actions and Cargo weekly. Rust `stable`, hosted runner images and system
package repositories still move; full reproducibility needs separate review.
Pinned CLI versions installed in shell steps need manual maintenance.

Run `make test-publish-release` for the safe regression suite. It copies the
helper into temporary fixtures and substitutes every `git`, `gh` and `make`
invocation: no network, actual refs, approval files in the checkout, real
builds or release mutations. The suite covers protected versions, dirty tree,
version/branch/upstream/tag mismatches, missing approval, published releases,
network/auth/API failures, positive 404 and hidden drafts, draft creation,
interrupted upload/resume, remote integrity and missing-digest verification.

The implementation is [the publishing helper](../scripts/publish-release.sh),
with [offline tests](../scripts/test-publish-release.sh) and
[Makefile entry points](../Makefile). Never use a real `make publish` as a test.
