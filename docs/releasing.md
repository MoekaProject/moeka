# Release safety and local publishing

## Channels and current block

Official GitHub Releases remain in **MoekaProject/community**. The only
supported official release entry point is local `make publish vX.Y.Z` on the
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

**GPL readiness is unresolved until a human completes provenance and
distribution review.** The release helper requires a regular, tracked and
committed file named packaging/release-approval.txt containing this exact line:

> GPL-3.0-or-later distribution review: approved

There is deliberately no environment-variable bypass. This change does not
create the approval file. Do not add it merely to make a build pass. Keep it
absent while the open-source readiness checklist is incomplete; revoke the
approval if dependencies, assets, licensing, or distribution conditions change.
Record reviewer, date, reviewed dependency/asset inventory, and evidence in the
approval file when the review is genuinely complete. The automatic gate checks
the declaration, not the legal merits or continuing validity of that review.

The gate applies only to official publishing, not ordinary `cargo build`,
`make build-release`, or candidate builds. Packaging does require LICENSE and
THIRD_PARTY_NOTICES.md; an optional licenses/ directory is copied when present.
Missing notices fail packaging rather than silently producing incomplete
distributions. The macOS app (including the separate MAS bundle) carries these
in Resources; Windows and Linux packages carry them alongside the executable.
MAS packaging remains a separate existing channel, not an approval of App Store
terms compatibility or permission to distribute an unresolved GPL build.

## Required review: source and licensing

Before creating approval, a human must review at least:

- The GPL-3.0-or-later grant, contributor provenance, modifications and notices.
- All vendored, registry and Git dependencies, optional/platform features,
  fonts, icons, embedded resources, and bundled Linux shared libraries.
- Compatibility and the complete notices/license texts for actual shipped
  binaries, not just the top-level Rust package.
- Corresponding Source obligations, dependency source availability and exact
  versions, build/install scripts, toolchain inputs, and any applicable
  Installation Information obligations.
- The source/history that will be made public, including credentials and
  material for which redistribution rights are missing. Rotate exposed secrets;
  removing their current files does not remove them from Git history.

The helper emits dist/Moeka-vX.Y.Z-source.tar.gz using `git archive` of the
**exact release tag**, with a versioned prefix and deterministic gzip headers.
It contains tracked project source, tracked scripts and vendor contents (subject
to Git archive attributes), not untracked/ignored files, Cargo's downloaded
crate sources, Git dependency checkouts in the Cargo cache, sysroot packages,
or submodule working trees. **A Git archive plus notices is not evidence of
complete GPL Corresponding Source compliance.** Review whether additional
dependency source, offers or other delivery mechanisms are required before
approving distribution. This helper does not assemble a complete dependency
source bundle or claim that it satisfies all GPL obligations.

## Preflight and official release procedure

1. Complete the review above, commit the approval and distribution documents,
   bump the package version, and update/commit the lockfile separately. Run CI
   and review its results. Authenticate local Git and GitHub CLI without
   putting credentials in source or command history.
2. Synchronize `main` separately through the normal review/push process. It
   must track `origin/main`, and local HEAD, the tracking ref and a live
   `git ls-remote origin refs/heads/main` must agree. Publishing never pushes
   `main`. Verify the configured origin, including any push URL, is correct.
3. Run `make publish-check TAG=vX.Y.Z`. This is read-only: no fetch, build,
   signing, ref creation, upload or release edit. It checks the clean tree
   (including untracked files), stable tag syntax/version, main/upstream,
   committed approval/license/notices/lockfile, and local and remote tag
   commits. An existing lightweight or annotated tag at HEAD is accepted.
   Any other commit is refused, never retagged.
4. Preflight authenticates access to MoekaProject/community. Only an actual
   HTTP 404 from the tag endpoint, followed by a successful paginated release
   listing, means absence. The listing also finds hidden drafts. Network,
   authentication, permission, malformed response and non-404 HTTP errors
   fail closed. A published release is refused **before expensive builds**.
5. After review, run `make publish vX.Y.Z` (not for v0.1.12). A fresh release
   checks signing/cross-build prerequisites, then uses the existing signed and
   notarized macOS pipeline plus Windows/Linux cross builds. Keep the default
   Moeka names and dist layout; the official asset allowlist is fixed, not a
   wildcard over potentially stale files. Do not override build commands or
   paths to reuse candidate binaries.
6. The helper repeats preflight after builds, checks unchanged HEAD, creates
   an annotated tag **only if absent**, then prepares the exact-tag archive and
   dist/SHA256SUMS. Only missing tag refs are pushed, without force, to origin
   and MoekaProject/community. **Pushing the tag exposes its reachable source
   history in the destination repository**, not just the source tarball.
   Existing remote tags are checked by peeled commit, not overwritten.
7. The helper creates a **draft before any upload** with the exact HEAD target,
   or resumes that same draft. It uploads eight binary archives (two macOS DMGs
   and six platform ZIPs), the source tarball and SHA256SUMS. SHA256SUMS covers
   all nine payloads, not itself. Unknown/duplicate/incomplete remote assets
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
  finishes. On retry an existing draft skips builds and requires the exact
  original source archive and matching SHA256SUMS. It verifies existing remote
  assets and uploads only missing ones. Rebuilt signed/notarized bytes often
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
- Source archives/checksums establish traceability and transport integrity,
  not a cryptographic signature or proof of reproducible binaries. Signing and
  notarization remain in the existing local macOS scripts; the helper's tests
  do not validate Apple services, hosted runners or binary functionality.

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
