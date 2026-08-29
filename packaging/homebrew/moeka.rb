cask "moeka" do
  arch arm: "arm64", intel: "x86_64"

  version "0.1.10"
  sha256 arm:   "1d7039985a4e376412ea4080e8df894c4064fc45f553007954d33f36a99b2ffd",
         intel: "33100f78fd5da41b02b39e7f2cf2bf9ce4745382ce9e8266b5b02008fae7743e"

  url "https://github.com/MoekaProject/community/releases/download/v#{version}/Moeka-macos-#{arch}.dmg"
  name "Moeka"
  desc "Markdown editor"
  homepage "https://moeka.app/"

  auto_updates true
  depends_on macos: :monterey

  app "Moeka.app"
end
