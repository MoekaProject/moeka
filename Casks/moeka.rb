cask "moeka" do
  arch arm: "arm64", intel: "x86_64"

  # Bootstrap from the last published release; CI replaces this with the first
  # verified release in this repository, without inventing unpublished hashes.
  version "0.1.39"
  sha256 arm:   "0861a084b8f22d6fca78d9970319c1cfb36b156680991b46389010cf6b01fe41",
         intel: "3c0e7d66dc2579c872c9a13648e69672ad57c9bc9b79e921cb7447e32bef8d67"

  release_repository = version.to_s == "0.1.37" ? "community" : "moeka"
  url "https://github.com/MoekaProject/#{release_repository}/releases/download/v#{version}/Moeka-macos-#{arch}.dmg"
  name "Moeka"
  desc "Markdown editor"
  homepage "https://moeka.app/"

  livecheck do
    url "https://github.com/MoekaProject/moeka"
    strategy :github_releases
  end

  auto_updates true
  depends_on macos: :monterey

  app "Moeka.app"
end
