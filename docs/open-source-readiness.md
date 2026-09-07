# Open-source readiness

This document tracks preparation, not authorization to publish. The repository
must remain private until the unchecked release blockers are resolved.

## Decisions

- Original Moeka code: GPL-3.0-or-later.
- Public source history: a single clean initial commit; no old-history backup
  is requested. Do not copy the current `.git` into the public snapshot.
- Downloads and releases stay in MoekaProject/community; code PRs belong in
  MoekaProject/moeka. Existing v0.1.12 artifacts must not be replaced.
- Only the local maintainer publishing flow creates official releases. Hosted
  workflows build unsigned candidates and do not publish them.

## Completed preparation

- [x] English README, contribution, security, conduct and architecture guides.
- [x] Official GPLv3 text and GPL-3.0-or-later package metadata.
- [x] Removed the tracked personal workspace/session configuration; added empty examples.
- [x] Moved root icons to semantic categories without changing SVG bytes.
- [x] Added static asset-reference validation.
- [x] Removed the CDDL-licensed bundled flamegraph renderer path; retained CPU sampling.
- [x] Replaced force-push/delete-and-recreate publishing with guarded draft uploads.
- [x] Added least-privilege CI and dependency-update configuration.
- [x] Initial Gitleaks history scan (2026-09-05, v8.30.1): no findings in 255 scanned commits.

## Local verification (2026-09-05)

- Rust 1.95.0: `cargo test --release --locked` — 572 passed, 4 ignored.
- `cargo build --release --locked` — passed on macOS.
- `cargo clippy --release --locked --all-targets` — passed with warnings:
  application target 182; test target 198 including 182 duplicates. This is
  the current lint backlog, not a zero-warning claim or a coverage percentage.
  Clean these incrementally with focused behavioral regression tests.
- Windows and Linux x86_64/ARM64 compile checks — all four passed.
- Static asset check — 153 literal references passed; dynamic whiteboard
  prefix still requires manual review when tool names change.
- Offline release safety — 36 isolated fixtures passed; no real publication.
- Resolved Cargo metadata — 936 packages, no inferno or CDDL declaration after
  removing bundled flamegraph rendering. This is not a full license clearance.
- No-Git source snapshot — export, Gitleaks directory scan, asset validation and
  macOS release build passed. The build reused installed tools and Cargo caches;
  it does not establish fresh-machine or cross-platform runtime reproducibility.

These results apply to this preparation tree, not the unmodified public v0.1.12
binaries. Revalidate after subsequent changes and before the final cutover.

Secret scanners do not establish the absence of private document content,
copyright issues, cached GitHub references or sensitive data in release binaries.

## Blockers before public visibility or new binary distribution

- [ ] Confirm ownership/license of original code and externally adapted snippets;
  preserve contributor and third-party attribution despite history compression.
- [ ] Establish provenance and license notices for every icon, brand asset,
  installation image, font, fixture and embedded image. Unknown items must be
  replaced, removed or approved with evidence, not silently relabeled GPL.
- [ ] Decide whether legacy `site/` belongs in the public desktop repository;
  it is independent of MoekaProject/website. Audit its design links and assets.
- [ ] Complete resolved dependency and platform-library license/source review.
- [ ] Provide all required notices and corresponding sources alongside GPL binaries.
- [ ] Review Mac App Store distribution terms separately for compatibility with
  the chosen license; the `mas` feature is not legal clearance.
- [ ] Validate a clean snapshot on all supported platforms, including real GUI
  smoke tests. Cached local builds and numeric layout tests are insufficient.
- [ ] Verify hosted CI succeeds; enable private vulnerability reporting and
  configure branch protection, release access and workflow approvals.
- [ ] Scan the final snapshot and public refs; review sample documents,
  screenshots, binaries and Actions artifacts for personal information.
- [ ] Confirm the final destructive ref replacement and public-visibility scope.

Only after these checks may a maintainer commit `packaging/release-approval.txt`
containing `GPL-3.0-or-later distribution review: approved`. Include the reviewed
revision, evidence, reviewer, date and any scope limitations. Adding the marker
does not itself establish legal compliance. It must be revisited for dependency,
asset, distribution-channel and licensing changes.

## Single-commit cutover

1. Freeze and review the prepared tree while still private.
2. Export only reviewed files into a fresh directory; initialize one root commit.
3. Validate that this commit has no parents and the snapshot has no old refs,
   private config, build cache or credentials.
4. Test and scan from a fresh clone of that candidate.
5. Obtain explicit confirmation before replacing main and deleting old remote
   branches/tags. Account for PR refs, forks, other remotes and GitHub caches;
   a force push does not erase them. Rotate any exposed credentials first.
6. Do not push an old tag or ancestor into community while attaching future
   releases. The publishing script assumes cutover has already been completed.
7. Preserve existing community releases. Use a new release tag for the prepared
   GPL source rather than retargeting v0.1.12 to changed code.
8. Change visibility only after confirming the remaining remote surface is safe.

No history rewrite, remote ref deletion, new release or visibility change is
performed by this preparation work.
