# StewardAuth

[Русский](README.ru.md) · [English](README.md) · **中文**

原生 **macOS Steam 桌面令牌（Steam Desktop Authenticator）**，专为 Apple Silicon 打造 —— 实时 Steam Guard 验证码、交易与市场确认、多账号管理。快速的菜单栏 + Dock 应用。

[**⚡ 快速安装**](#快速安装-homebrew) · [**📦 简单安装**](#简单安装-dmg) · [**💬 Telegram**](https://t.me/StewardAuth)

> **系统要求：** macOS 12 (Monterey) 或更高 · Apple Silicon (M1–M5)

## 功能

- **实时 Steam Guard 验证码** —— 30 秒倒计时，点击复制。
- **交易与市场确认** —— 批准/拒绝，可为每个账号开启后台**自动确认**。
- **多账号** —— 侧栏搜索、分组、右键操作。
- **从零创建新令牌**（登录 → 短信/邮箱验证码 → `.maFile` + 撤销码）。
- **导入**现有的 `.maFile`、ZIP 压缩包或 SDA 文件夹 —— 支持拖放。
- **每账号独立代理**（HTTP / HTTPS / SOCKS5）。
- **可选主密码**加密保险库（Argon2id + AES-256-GCM）。
- **8 种界面语言**，内置**自动更新**。

## 快速安装 (Homebrew)

还没有 Homebrew？先安装它：

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

然后安装 StewardAuth：

```bash
brew tap shinyawave/tap
brew install --cask stewardauth
```

如果 Homebrew (6.0+) 要求信任该 tap，运行一次：`brew trust shinyawave/tap`。

> **首次启动：** StewardAuth 采用 ad-hoc 签名且未经过公证，因此 macOS Gatekeeper 会拦截首次启动。运行一次：
> ```bash
> xattr -cr "/Applications/StewardAuth.app"
> ```
> ……或在“应用程序”中**右键点击** **StewardAuth** → **打开** → **仍要打开**。之后的自动更新无需此步骤。

## 简单安装 (.dmg)

无需终端：

1. 从 [**Releases**](https://github.com/shinyawave/stewardauth/releases) 下载 `StewardAuth_0.1.0_aarch64.dmg`。
2. 打开 `.dmg`，将 **StewardAuth** 拖入**应用程序**。
3. 首次启动：**右键点击** StewardAuth → **打开** → **仍要打开**（一次性 Gatekeeper 步骤；或运行 `xattr -cr "/Applications/StewardAuth.app"`）。

## 账号文件存放位置

默认情况下，你的账号以 `.maFile` 文件形式保存在：

```
~/Library/Application Support/com.stewardauth.app/maFiles/
```

你可以在应用内的 **设置 → maFiles / 数据目录** 中**打开该文件夹或将其迁移**到任意位置（外置磁盘、同步文件夹等）。可选的**主密码**会用 Argon2id + AES-256-GCM 加密保险库清单（账号列表与设置）；而 `.maFile` 本身遵循经典的 Windows SDA 明文存储模式，因此请妥善保管此文件夹并做好备份。

## 社区

问题、更新、反馈 → **[Telegram: @StewardAuth](https://t.me/StewardAuth)**

## 免责声明

StewardAuth 是独立的社区项目，与 Valve 或 Steam 无关，也未获其认可。

## 许可证

[AGPL-3.0-or-later](LICENSE)。
