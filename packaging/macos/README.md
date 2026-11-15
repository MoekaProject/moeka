# Signed macOS CI candidates

The existing `release.yml` workflow still creates unsigned candidates without
secrets. `signed-macos.yml` can be run manually for signed candidates or called
by the [official release workflow](../../.github/workflows/publish.yml). Its jobs
never push tags, create a GitHub Release, or modify Homebrew; the official workflow
performs those operations only after all six platform builds pass verification.
No official build or signing step runs locally.

## Environment setup

In **MoekaProject/moeka > Settings > Environments**, create `release-signing`.
Restrict deployment branches to `main` and enable required reviewers when the
repository's GitHub plan supports them. Only trusted maintainers should be able
to edit workflows or dispatch signing jobs. A workflow-level `main` condition is
not a substitute for environment access controls.

Add these environment secrets without committing or logging their contents:

| Secret | Value |
| --- | --- |
| `MACOS_CERTIFICATE_BASE64` | Base64 of a password-protected P12 containing the Developer ID Application certificate and private key |
| `MACOS_CERTIFICATE_PASSWORD` | P12 export password |
| `APPLE_ID` | Apple account belonging to the developer team |
| `APPLE_APP_SPECIFIC_PASSWORD` | App-specific password, not the Apple account login password |

Add these environment variables:

| Variable | Value |
| --- | --- |
| `APPLE_TEAM_ID` | Ten-character Apple Developer Team ID |
| `DEVELOPER_ID_APPLICATION` | Exact `Developer ID Application: Name (TEAM_ID)` identity from that P12 |

The temporary keychain password is generated per job and masked. No API key,
provisioning profile, repository-write token, or local login keychain is needed
for this Developer ID distribution workflow. Base64 is not encryption.

## Running and verifying

1. Commit the intended version and source changes to `main`; do not bump or tag
   automatically in CI.
2. Open **Actions > Signed macOS candidates (no publishing) > Run workflow**.
   Select `main` and enter the exact Cargo version without `v`, e.g. `0.1.38`.
3. Both unsigned app builds finish before the signing jobs run on fresh hosted
   macOS runners. Signing checks the archive hash, app version, CPU architecture,
   certificate identity and notarization credentials. All jobs use the same
   dispatch commit SHA; no dependency cache is restored in the signing job.
4. Download the two `signed-candidate-macos-*` artifacts. Each includes a DMG,
   ZIP, `SHA256SUMS`, and `build-info.txt` containing the source SHA and run ID.
   Verify hashes, app signatures, stapled tickets, Gatekeeper assessment and
   actual app launch on the appropriate macOS architecture before any release.

The existing package script signs and notarizes the **app**, staples its ticket,
then creates the DMG and ZIP. It does not separately sign/notarize the DMG
container. DMG layout uses Finder/AppleScript; verify it on the hosted runner.
No `make publish` command runs in this workflow.

## Failure and cleanup

`ci-macos-signing.sh` deletes the imported P12 immediately after setup. Its
failure handler and the workflow's `always()` cleanup delete the dedicated
temporary keychain. Hosted runners are also discarded after each job.

The `notarization-recovery-*` artifact retains the original **signed app ZIP**
and submission output for 14 days, including failures. This directory never
contains the certificate, private key or passwords. Download it before retention
expires if recovery is needed. A hard cancellation may prevent artifact upload;
preserve any submission ID already printed in the job log.

The workflow refuses signing on a rerun attempt to avoid blindly resubmitting an
archive after a timeout or lost response. Query the original submission with
`notarytool info`/`wait` using its ID. On `Accepted`, extract the original archive
and staple/validate it rather than re-signing or resubmitting it. Resolve unknown
outcomes before requesting a new candidate run. Rerunning only a failed publish
or Homebrew job reuses successful build jobs rather than signing again.
Published-version immutability rules remain unchanged.

For local use, `SIGNING_KEYCHAIN` and `NOTARY_KEYCHAIN` are optional; omitting them
preserves the existing keychain lookup. `NOTARY_OUTPUT_DIR` optionally retains
the submission archive and logs instead of deleting temporary files.

## Offline validation

```sh
make test-publish-release
ruby scripts/test-check-community.rb
ruby scripts/check-community.rb
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.12 -shellcheck= -pyflakes=
git diff --check
```

Tests intercept signing and keychain commands; never test with real publication,
credentials or a developer's keychain.
