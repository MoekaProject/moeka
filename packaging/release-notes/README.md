# Official GitHub Actions releases

All official builds and publication run in
[publish.yml](../../.github/workflows/publish.yml), manually dispatched on `main`.
The source commit must contain matching Cargo versions and
`packaging/release-notes/vX.Y.Z.md` with English Features, Fixes and Improvements.

## Configuration

- `release-signing` environment: the four Apple/P12 secrets and two variables
  documented in the [macOS guide](../macos/README.md).
- The repository's built-in `GITHUB_TOKEN` handles preflight, source tag, Release
  and Cask publication in **MoekaProject/moeka**. Preflight is read-only; publish
  and Homebrew jobs have contents write for their separate operations. No
  `MOEKA_PUBLIC_RELEASE_TOKEN` or cross-repository write access is needed.
  Checkout does not retain credentials.
- Enable environment branch restrictions and approval rules where supported.
  Protect `main` and workflow edits; do not expose signing to pull requests,
  forks or `pull_request_target`.

There is no Windows Authenticode credential in the current configuration.
Windows and Linux packages are unsigned. macOS apps use Developer ID signing
and Apple notarization; macOS DMG containers are not separately signed.

## Dispatch

After committing and pushing the intended source/version:

```sh
make publish TAG=v0.1.38
# Or for an explicitly requested prerelease:
make publish-beta TAG=v0.1.38
```

These commands only dispatch Actions. Use the Actions page to monitor the run;
they do not wait for publication or authorize changing a previously released tag.
Do not push another source commit while the release is running: the publisher
rechecks that remote main still matches the dispatch SHA.

## Pipeline

1. Validate version and absence/draft status of the requested release.
2. Build the four native SSH helpers without credentials, then sign/notarize the
   macOS helpers in isolated jobs before hashing and compressing them. Every
   desktop app package must embed this source-matched bundle.
   Build macOS app bundles without credentials, then sign/notarize them in fresh
   hosted macOS jobs. In parallel, build Windows and Linux natively on x86_64 and
   ARM64 runners. Packages include repository license notices.
3. Record version, source SHA, workflow run ID, filenames, sizes and SHA-256 in
   per-platform manifests. No developer-machine artifacts are accepted.
4. Download artifacts from this run without merging directories. Verify all six
   unique platform groups and exactly eight expected attachments before copying
   them into the publish directory.
5. The existing release helper consumes these artifacts without building,
   signing or overwriting them. It creates one source tag at the build SHA and a
   draft, uploads missing files serially, verifies remote content, then publishes.
6. Stable releases update `Casks/moeka.rb` on this repository's main branch in
   a separate job. Prereleases do not. This Cask-only commit does not move the
   source tag and does not discard concurrent source commits.

   ## Performance validation

   Build jobs share target-specific dependency caches with CI and unsigned
   candidates. Content-addressed sccache also retains compilation of the vendored
   GPUI path dependency, which the dependency cache excludes. A small Rust compiler
   wrapper routes only GPUI through sccache: duplicating every dependency in both
   caches generates excessive cache API traffic. Only this
   repository's main branch writes caches; PRs restore them read-only. Runner
   families, macOS SDKs, target triples and test/production dependency sets remain
   separate. Disposable hosted runners retain only the repository's active Rust
   toolchain: rust-cache otherwise fingerprints unused preinstalled compilers and
   misses compatible caches when runner images carry different stable versions.
   On the three primary CI targets, production restores the dependency
   superset written by the full test job instead of keeping a second large archive.
   Other architectures retain their production caches. Trusted jobs may save
   compiled dependencies after a test failure; PRs never write either cache.
   All production builds retain the release profile and locked
   dependencies. Signing uses fresh cache-free runners, with both architectures
   signing independently in parallel after their unsigned builds complete.

   To benchmark the complete build, signing and artifact-verification pipeline
   without creating a Release, moving a tag or updating the Cask:

   ```sh
   GH_PAGER=cat gh workflow run publish.yml --repo MoekaProject/moeka --ref main \
     -f version=0.1.38 -F candidate_only=true
   ```

   This mode may use an already-published version, but its outputs are **candidates
   only** and must not replace published assets. It still uses Apple credentials,
   creates notarization submissions and retains recovery artifacts; the usual
   notarization retry restrictions apply. Run a new dispatch only after the
   previous run's outcome is known. Compare runs on the same source SHA, including
   cache restore/save time, compiler-cache hit statistics and job durations. The
   publication and Cask jobs are skipped, so report their omitted overhead rather
   than describing a candidate benchmark as a real publication.

   For PR-equivalent timing, dispatch `ci.yml` on the same main SHA with
   `pr_profile=true`. This selects the PR matrix and forces read-only caches.
   Documentation/Cask-only diffs still run metadata, secret and release-safety
   checks; configure the always-reporting `CI result` as the required check.

## Recovery

- Rerun **failed jobs**, not all jobs, after investigating a transient publishing
  failure. Successful build/sign jobs and their immutable artifacts remain the
  source of bytes. The publisher rejects mismatched manifests or existing
  attachments with different bytes.
- Never overwrite published assets or move published tags. If the release already
  became public, do not rerun publication: verify its flags/assets and retry only
  the failed downstream operation.
- If notarization failed, retrieve the recovery artifact and inspect the original
  submission ID before any retry. Signing on a repeated run attempt is refused
  to avoid duplicate submissions after an unknown result.
- After a Cask-only failure, rerun only the failed `homebrew` job. Ensure repository
  rules allow its Cask-only main commit, or use a reviewed PR flow if required.
  Do not rebuild, re-sign or recreate the release.
- If the dispatch SHA changed, artifacts expired, or a run has incomplete
  notarization state, stop for reconciliation rather than mixing local or
  newly-built bytes into an existing draft.

Local release builds are no longer the production path. Existing local signed
artifacts are preserved for diagnosis, not uploaded by this pipeline.

Old community releases and homebrew-moeka history are not changed by this
workflow. Older applications querying the community update feed do not
automatically discover the new destination; redirects or a bridge release need
separate authorization. See the [Homebrew guide](../homebrew/README.md) for the
explicit remote needed when using this repository as a tap.
