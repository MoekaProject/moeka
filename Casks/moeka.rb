cask "moeka" do
  arch arm: "arm64", intel: "x86_64"

  # Bootstrap from the last published release; CI replaces this with the first
  # verified release in this repository, without inventing unpublished hashes.
  version "0.1.38"
  sha256 arm:   "580186c0028f7378f07cce05a22815e79b53e554a1b8e29a1e04ad34512dfe15",
         intel: "ce5c32eceeb592a737228455b45b7d2760c4a48bdcdd9d9cba96765c7fa236bc"

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
