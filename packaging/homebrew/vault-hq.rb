# Homebrew cask for Vault HQ. This is the template: scripts/release.sh fills in the
# version and the DMG's SHA-256 and writes the result to dist/release/vault-hq.rb, which
# is copied to Casks/vault-hq.rb in the Athreyas/homebrew-tap repo (docs/v2/RELEASING.md).
cask "vault-hq" do
  version "@@VERSION@@"
  sha256 "@@SHA256@@"

  url "https://github.com/Athreyas/vault-hq/releases/download/v#{version}/Vault-HQ_#{version}_aarch64.dmg"
  name "Vault HQ"
  desc "Local front end for a markdown vault: tasks, notes, days and local-model search"
  homepage "https://github.com/Athreyas/vault-hq"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :ventura"

  app "Vault HQ.app"
  # `vault-hq mcp` is the MCP stdio server agents launch; put it on PATH.
  binary "#{appdir}/Vault HQ.app/Contents/MacOS/vault-hq"

  uninstall quit: "local.vaulthq.desktop"

  # The user's vaults are never touched: only HQ's own settings, caches and state.
  zap trash: [
    "~/Library/Application Support/local.vaulthq.desktop",
    "~/Library/Caches/local.vaulthq.desktop",
    "~/Library/HTTPStorages/local.vaulthq.desktop",
    "~/Library/LaunchAgents/Vault HQ.plist",
    "~/Library/Preferences/local.vaulthq.desktop.plist",
    "~/Library/Saved Application State/local.vaulthq.desktop.savedState",
    "~/Library/WebKit/local.vaulthq.desktop",
  ]
end
