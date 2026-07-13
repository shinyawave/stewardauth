# Homebrew Cask for StewardAuth — belongs in the tap repo
# github.com/shinyawave/homebrew-tap at Casks/stewardauth.rb
#
# Install (after the tap repo is public):
#   brew install --cask shinyawave/tap/stewardauth
# or:
#   brew tap shinyawave/tap && brew install --cask stewardauth
#
# NOTE: StewardAuth is ad-hoc signed and NOT notarized (no Apple Developer
# account). Homebrew is phasing out --no-quarantine, so the first launch still
# needs a one-time Gatekeeper bypass — see the caveats block below.

cask "stewardauth" do
  version "0.1.0"
  # The release .dmg is built in CI (tauri-action), so its hash is NOT the same
  # as a local build. Two options:
  #   1) :no_check (current) — simplest; no need to bump the hash every release.
  #   2) Pin it — after publishing the GitHub release, download the asset and run
  #      `shasum -a 256 StewardAuth_0.1.0_aarch64.dmg`, then replace :no_check
  #      with `sha256 "<that hash>"`. You must re-pin on every version bump.
  sha256 :no_check

  url "https://github.com/shinyawave/stewardauth/releases/download/v#{version}/StewardAuth_#{version}_aarch64.dmg"
  name "StewardAuth"
  desc "Native Steam Desktop Authenticator"
  homepage "https://github.com/shinyawave/stewardauth"

  # Detect new versions from the GitHub releases (for `brew outdated` / bump PRs).
  livecheck do
    url :url
    strategy :github_latest
  end

  # StewardAuth ships its own updater (tauri-plugin-updater), so let it update
  # itself rather than Homebrew fighting the in-app updater.
  auto_updates true
  # Apple Silicon only; requires macOS 12 (Monterey) or newer.
  depends_on arch: :arm64
  depends_on macos: :monterey

  app "StewardAuth.app"

  # `brew uninstall --zap` cleanup. Intentionally does NOT touch the maFiles
  # vault (default ~/Desktop/sda or a user-chosen dir) — only the fixed
  # app-data pointer dir + system caches/prefs keyed by the bundle id.
  zap trash: [
    "~/Library/Application Support/com.stewardauth.app",
    "~/Library/Caches/com.stewardauth.app",
    "~/Library/HTTPStorages/com.stewardauth.app",
    "~/Library/Preferences/com.stewardauth.app.plist",
    "~/Library/Saved Application State/com.stewardauth.app.savedState",
    "~/Library/WebKit/com.stewardauth.app",
  ]

  caveats <<~EOS
    StewardAuth is ad-hoc signed and not notarized, so macOS Gatekeeper blocks
    the FIRST launch. This is a one-time step (auto-updates afterwards are fine):

      xattr -cr "/Applications/StewardAuth.app"

    …or right-click StewardAuth in Applications → Open → Open Anyway.
  EOS
end
