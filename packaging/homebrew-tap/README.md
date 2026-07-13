# StewardAuth — Homebrew tap setup

This folder is a **staging copy** of the Homebrew tap. The cask itself must live in
a **separate repository** named `homebrew-tap` under your GitHub account, because
Homebrew derives the tap name from the repo name (`homebrew-<name>`).

Target layout in the tap repo `github.com/shinyawave/homebrew-tap`:

```
homebrew-tap/
└── Casks/
    └── stewardauth.rb
```

## One-time setup (you run these — nothing is pushed automatically)

```bash
# 1. Create the tap repo on GitHub (must be named exactly "homebrew-tap", public).
#    Then, in a scratch dir:
git clone https://github.com/shinyawave/homebrew-tap.git
cd homebrew-tap
mkdir -p Casks
cp <path-to-stewardauth-repo>/packaging/homebrew-tap/Casks/stewardauth.rb Casks/
git add Casks/stewardauth.rb
git commit -m "Add StewardAuth cask"
git push
```

> **Status:** the tap repo `github.com/shinyawave/homebrew-tap` is already created and
> the cask is pushed (public). It becomes *installable* only once the `stewardauth`
> repo is public with a published release carrying the `.dmg` asset.

## What users then run

```bash
brew tap shinyawave/tap
brew install --cask stewardauth
```

Homebrew 6.0+ treats third-party taps as untrusted; if it refuses to load the cask,
the user runs once:

```bash
brew trust shinyawave/tap
```

First launch needs a one-time Gatekeeper bypass (the cask prints this as a caveat):

```bash
xattr -cr "/Applications/StewardAuth.app"
```

…or right-click StewardAuth in Applications → **Open** → **Open Anyway**.

## Prerequisites (same gates as the auto-updater)

- The `stewardauth` repo must be **public** with a **published** (non-draft) release
  that carries the `StewardAuth_<version>_aarch64.dmg` asset the cask URL points to.
- The cask `version` must match the release tag (`v<version>`).

## Keeping it in sync

- On each new version: bump `version` in `Casks/stewardauth.rb` and push to the tap repo.
- The app also self-updates via its built-in updater (`auto_updates true`), so most
  users get new versions without `brew upgrade`. The cask is mainly for first install.
- `sha256 :no_check` is used so you don't have to re-hash every release. To pin instead,
  download the published `.dmg`, run `shasum -a 256 <dmg>`, and replace `:no_check`
  with `sha256 "<hash>"` (must be re-pinned on every version bump).

## Validate the cask before publishing (optional)

```bash
# From a checkout of the tap repo:
brew audit --cask --new Casks/stewardauth.rb   # style/consistency checks
brew style Casks/stewardauth.rb                 # rubocop formatting
```

Note: `brew audit` will warn that the app is unsigned/un-notarized — expected and
acceptable for a personal tap (official homebrew-cask no longer accepts such apps).
