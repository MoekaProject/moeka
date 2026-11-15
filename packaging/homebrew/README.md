# Homebrew

The [Cask](../../Casks/moeka.rb) lives in the public application repository and
installs the signed, notarized macOS application:

```sh
brew tap --custom-remote MoekaProject/moeka https://github.com/MoekaProject/moeka
brew install --cask moekaproject/moeka/moeka
brew update
brew upgrade --cask --greedy moekaproject/moeka/moeka
```

If Homebrew requests trust, review the Tap before running `brew trust moekaproject/moeka`. The cask installs `Moeka.app`; `--greedy` includes auto-updating casks.

## Maintainer commands

The explicit remote is required: Homebrew otherwise assumes a repository named
`homebrew-moeka`. `--custom-remote` also changes an existing tap's remote. The
application source history is downloaded with the tap. Old-repository redirects
are a separate migration task, not performed by the release workflow.

## Maintainer publishing

Stable releases from `publish.yml` update only `Casks/moeka.rb` on `main` in the
same repository after both released DMGs have been downloaded and their size and
SHA-256 verified. The job uses its repository-scoped `GITHUB_TOKEN`; no PAT or
separate Tap repository is needed. The source tag remains on the built commit;
the subsequent Cask-only commit never changes the tag.

For a failed Cask update, rerun only the failed `homebrew` Actions job. Local
`make homebrew-check TAG=vX.Y.Z` is read-only. Preserve concurrent source commits,
reject downgrades and changed checksums for the same version, and never force
push or republish the application to repair a Cask failure.

The initial Cask retains the verified 0.1.37 download in the legacy repository
until the first new stable release succeeds; new versions download from moeka.
See [release and recovery rules](../../AGENTS.md#release).
