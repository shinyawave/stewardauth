# StewardAuth

[Русский](README.ru.md) · **English** · [中文](README.zh.md)

Native **macOS Steam Desktop Authenticator** for Apple Silicon — live Steam Guard codes, trade &amp; market confirmations, and multi-account management, in a fast menu-bar + Dock app.

[**⚡ Quick install**](#quick-install-homebrew) · [**📦 Simple install**](#simple-install-dmg) · [**💬 Telegram**](https://t.me/StewardAuth)

> **Requirements:** macOS 12 (Monterey) or newer · Apple Silicon (M1–M5)

## Features

- **Live Steam Guard codes** — 30-second countdown, click to copy.
- **Trade &amp; market confirmations** — approve/deny, with optional background **auto-confirm** per account.
- **Multiple accounts** — sidebar search, groups, right-click actions.
- **Add a new authenticator** from scratch (login → SMS/email code → `.maFile` + revocation code).
- **Import** existing `.maFile`s, ZIP archives, or SDA folders — drag &amp; drop.
- **Per-account proxy** routing (HTTP / HTTPS / SOCKS5).
- **Optional master-password** vault encryption (Argon2id + AES-256-GCM).
- **8 interface languages** and a built-in **auto-updater**.

## Quick install (Homebrew)

Don't have Homebrew yet? Install it first:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

Then install StewardAuth:

```bash
brew tap shinyawave/tap
brew install --cask stewardauth
```

If Homebrew (6.0+) asks you to trust the tap, run once: `brew trust shinyawave/tap`.

> **First launch:** StewardAuth is ad-hoc signed and not notarized, so macOS Gatekeeper blocks the first launch. Run once:
> ```bash
> xattr -cr "/Applications/StewardAuth.app"
> ```
> …or right-click **StewardAuth** in Applications → **Open** → **Open Anyway**. Auto-updates afterwards install without this step.

## Simple install (.dmg)

No terminal required:

1. Download `StewardAuth_0.1.0_aarch64.dmg` from [**Releases**](https://github.com/shinyawave/stewardauth/releases).
2. Open the `.dmg` and drag **StewardAuth** to **Applications**.
3. First launch: **right-click** StewardAuth → **Open** → **Open Anyway** (one-time Gatekeeper step; or run `xattr -cr "/Applications/StewardAuth.app"`).

## Where your accounts are stored

By default, your accounts are saved as `.maFile`s in:

```
~/Library/Application Support/com.stewardauth.app/maFiles/
```

You can **open this folder or move it** anywhere (external disk, synced folder, etc.) from **Settings → maFiles / Data directory** inside the app. An optional **master password** encrypts the vault manifest — the account list &amp; settings — with Argon2id + AES-256-GCM; the `.maFile`s themselves follow the classic Windows SDA plaintext-on-disk model, so keep this folder private and backed up.

## Community

Questions, updates, feedback → **[Telegram: @StewardAuth](https://t.me/StewardAuth)**

## Disclaimer

StewardAuth is an independent, community project. It is not affiliated with or endorsed by Valve or Steam.

## License

[AGPL-3.0-or-later](LICENSE).
