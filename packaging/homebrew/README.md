# Homebrew

Install the signed, notarized macOS application from the [public Tap](https://github.com/MoekaProject/homebrew-moeka):

```sh
brew tap MoekaProject/moeka
brew install --cask moekaproject/moeka/moeka
brew update
brew upgrade --cask --greedy moekaproject/moeka/moeka
```

If Homebrew requests trust, review the Tap before running `brew trust moekaproject/moeka`. The cask installs `Moeka.app`; `--greedy` includes auto-updating casks.

## Maintainer commands

Stable `make publish vX.Y.Z` updates the Tap automatically. For an existing official release:

```sh
make homebrew-check TAG=vX.Y.Z
make publish-homebrew TAG=vX.Y.Z
```

The check is read-only; publication commits and pushes. Both verify the two public DMGs by size and SHA-256. The helper updates only version/digests, preserving livecheck and other settings. Keep publishers serial, reconcile failed pushes, and revalidate concurrent changes. See [release and recovery rules](../../AGENTS.md#release).
