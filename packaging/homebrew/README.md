# Homebrew Cask

`Casks/moeka.rb` tracks the signed, notarized architecture-specific DMGs in the public release repository.

Install Moeka with:

```sh
brew tap MoekaProject/moeka
brew install --cask moekaproject/moeka/moeka
```

If Homebrew reports an untrusted tap, review
[`MoekaProject/homebrew-moeka`](https://github.com/MoekaProject/homebrew-moeka),
then run `brew trust moekaproject/moeka` and retry.

Upgrade with `brew update` followed by
`brew upgrade --cask --greedy moekaproject/moeka/moeka`. `--greedy` includes
auto-updating casks. This installs `Moeka.app`, not a `moeka` CLI.

## Local publishing

After publishing a stable release, `make publish vX.Y.Z` automatically invokes
the local Homebrew helper. No GitHub Actions workflow updates the tap.
To check or retry the tap update independently:

```sh
make homebrew-check TAG=vX.Y.Z
make publish-homebrew TAG=vX.Y.Z
```

Both commands download the two published DMGs and verify their sizes and SHA-256
digests. Only `publish-homebrew` commits and pushes the public tap. The helper
preserves the existing cask's other settings, including `livecheck`; the local
`moeka.rb` is a reference snapshot, not a template that overwrites the tap.

See [release safety and retries](../../docs/releasing.md#homebrew-distribution).
