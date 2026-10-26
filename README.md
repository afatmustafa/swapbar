<p align="center">
  <img src="docs/screenshots/hero.png" alt="Swapbar: the menu bar popover in light and dark mode, showing four Claude accounts with plan badges, 5h and Fable quotas, and token cost">
</p>

<p align="center">
  Every Claude account's quota in view, one click to switch.<br>
  A macOS menu bar app for <a href="https://github.com/realiti4/claude-swap"><code>claude-swap</code></a> (cswap).
</p>

<p align="center">
  <a href="https://github.com/afatmustafa/swapbar/releases/latest"><img src="https://img.shields.io/github/v/release/afatmustafa/swapbar" alt="Latest release"></a>
  <a href="https://github.com/afatmustafa/swapbar/releases"><img src="https://img.shields.io/github/downloads/afatmustafa/swapbar/total" alt="Downloads"></a>
</p>

<p align="center">
  <a href="https://github.com/afatmustafa/swapbar/releases/latest"><b>Download</b></a> ·
  <a href="https://swapbar.afat.me">Website</a> ·
  <b>English</b> | <a href="README.tr.md">Türkçe</a>
</p>

<p align="center">
  <img src="docs/screenshots/popover.png" width="864" alt="Swapbar popover in light and dark mode: four accounts with 5h, 7d and Fable quota bars, and a token and cost table">
</p>

Swapbar shows the 5h, 7d and per-model (Fable) quotas of every cswap account at a glance, switches accounts with one click, and summarizes token usage and cost from Claude Code transcripts.

## Features

- **Setup assistant.** On first run Swapbar checks for Claude Code and claude-swap, installs claude-swap if it's missing (with `uv`, installing `uv` too if needed), and adds your accounts one by one: each signs in through `claude auth login` in the browser, cswap saves the login, and Claude Code switches back to the account it was using.
- **All accounts, one glance.** Each account shows its plan (Max 20x, Max 5x, Pro, Team Premium/Standard) and gets 5h, 7d and per-model (Fable) bars with time to reset, plus its spend limit when it has one (e.g. "$13.50 / $50.00"). For seats billed by usage alone, such as some Enterprise seats, the spend limit is the quota. A bar turns red at the low-quota level (15% left by default) and yellow 25 points above it.
- **One-click switching.** Click an inactive account to run `cswap switch`. The active account is marked **current**, and accounts left out of auto-rotation show as **paused**.
- **Pace warnings.** A marker on a bar shows where an even pace would be when an account is using a window too fast, and the row says when the window would run out if that comes before its reset. A notification warns about the current account once per window.
- **Switch suggestion.** When the current account's tightest window drops to the low-quota level (15% left by default), a banner names the account with the most room and switches to it in one click. That account is also marked **suggested** in the list.
- **Notifications.** A notification when the current account runs low, with a **Switch** button for the best other account, another when a depleted account's quota resets, and a warning 3 days before an account's saved login expires (claude-swap 0.27+), with **Sign in again** on its row.
- **Why usage is missing.** When cswap can't fetch an account's usage, the row says why and when it tries again: "couldn't fetch usage: rate limited · retrying at 14:32" (claude-swap 0.27+).
- **Global shortcuts.** `⌃⌥S` opens Swapbar from any app; `⌃⌥⇧S` switches to the account with the most quota left. No Accessibility permission needed.
- **Update check.** Swapbar checks GitHub for a new release at launch and every 6 hours and shows a **Download** banner when one is out. It checks PyPI for a new claude-swap too, and **Update** runs `cswap upgrade`.
- **Quota in the menu bar.** The Swapbar icon is followed by the active account's quota, as `5h` or `5h · 7d`, shown as remaining or used.
- **Token and cost summary.** Today and 30-day totals for cost, tokens, requests, output and cache reads, cost per account (worked out from cswap's switch log, `cswap run` sessions and each profile's login) and per model, and a bar splitting input, cache read, cache write and output.
- **English and Turkish.** Follows the macOS language, or pick one in Settings › General.
- **Light and dark.** Follows the macOS appearance, or set it to Light or Dark in Settings.
- **Small and native.** Rust and [gpui](https://crates.io/crates/gpui) (Zed's UI framework), a universal binary for Apple Silicon and Intel, about 4 MB.

## Settings

Open **Settings…** from the popover. Preferences are stored in `~/.config/swapbar/settings.json`.

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/settings-general-dark.png" alt="General settings in dark mode"></td>
    <td width="50%"><img src="docs/screenshots/settings-menubar.png" alt="Menu Bar settings"></td>
  </tr>
  <tr>
    <td><b>General:</b> theme (System / Light / Dark), language (System / English / Türkçe), launch at login (LaunchAgent), refresh intervals, show or hide the usage section.</td>
    <td><b>Menu Bar:</b> percentage mode (remaining / used), title next to the icon (icon only / 5h / 5h · 7d), 7-day window, per-model limits, reset times.</td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/settings-accounts.png" alt="Accounts settings"></td>
    <td><img src="docs/screenshots/settings-auto.png" alt="Auto-switch settings"></td>
  </tr>
  <tr>
    <td><b>Accounts:</b> add an account, rename (<code>cswap alias</code>), reorder (<code>cswap swap</code>), sign a broken login in again, remove; show or hide each account in the menu, include it in auto-rotation (<code>cswap enable/disable</code>).</td>
    <td><b>Auto-switch:</b> cswap's <code>autoswitch.*</code> settings (<code>cswap config set</code>), reset to defaults. These only take effect while <code>cswap auto</code> is running.</td>
  </tr>
</table>

## Installation

### Homebrew

```sh
brew install --cask afatmustafa/tap/swapbar
```

The cask installs Swapbar into Applications; `brew upgrade --cask swapbar` updates it. On first launch the setup assistant installs [`claude-swap`](https://github.com/realiti4/claude-swap) if needed and adds your accounts.

### Download

1. Download `Swapbar.dmg` from the [latest release](https://github.com/afatmustafa/swapbar/releases/latest) and drag Swapbar to Applications.
2. Open Swapbar. Its icon appears in the menu bar, and the setup assistant opens if claude-swap or your accounts are missing. It needs [Claude Code](https://claude.com/claude-code) to sign in to accounts.

Swapbar looks for `cswap` at `~/.local/bin/cswap`, `/opt/homebrew/bin/cswap` and `/usr/local/bin/cswap`. Set `CSWAP_BIN` to use another path (`CLAUDE_BIN` and `UV_BIN` work the same way for `claude` and `uv`).

Releases from 0.1.1 on are signed with a Developer ID and notarized by Apple, so Swapbar opens with a double-click. Version 0.1.0 is not: macOS blocks its first launch, and you allow it with **Open Anyway** under System Settings → Privacy & Security.

To launch at login, turn on **Launch at login** in Settings.

### Build from source

```sh
./scripts/bundle.sh                    # release build, produces target/Swapbar.app
./scripts/bundle.sh --universal        # arm64 + x86_64
./scripts/dmg.sh                       # target/Swapbar.dmg from the bundle
cp -R target/Swapbar.app /Applications/
```

Requirements:

- Rust toolchain
- Xcode. gpui is built with the `runtime_shaders` feature, so the Metal toolchain is not needed.

## How it works

| Data | Source |
|---|---|
| Accounts, 5h/7d/Fable quotas | `cswap list --json`, every 60 seconds. cswap calls the Anthropic usage API itself. That endpoint allows roughly 30 requests per hour per account, so cswap manages the polling cadence. |
| Account switching | `cswap switch <n> --json` |
| Tokens and cost | `~/.claude/projects/**/*.jsonl`, plus `CLAUDE_CONFIG_DIR`, `~/.claude-*` profiles and cswap session directories. Entries are deduplicated by `message.id:requestId` and rescanned incrementally every 2 minutes. |
| Prices | Built-in table with a [models.dev](https://models.dev/api.json) overlay (cached for 24 hours). 1h cache write = 2× input, 5m cache write = 1.25× input. |

Caches live in `~/.config/swapbar/` (`usage-cache.json`, `model-rates.json`).

Cost figures are estimates at API list prices, not your subscription bill.

Swapbar does not talk to Anthropic directly. Quotas come from cswap, and usage is read from transcripts on your Mac.

## Development

```sh
cargo run                                          # click the Swapbar icon in the menu bar
SWAPBAR_OPEN=1 cargo run                           # open the popover on launch
SWAPBAR_SETTINGS=1 SWAPBAR_PAGE=about cargo run    # open settings on a given page
SWAPBAR_DEBUG=1 cargo run                          # geometry and scan logs
cargo run -- --scan                                # headless: print the token and cost summary
./scripts/icon.sh                                  # rebuild assets/AppIcon.icns from AppIcon.svg
```

`SWAPBAR_PAGE` accepts `menubar`, `accounts`, `auto` and `about`; anything else opens General.

The popover and settings screenshots use demo accounts and generated transcripts. Usage and quota figures in the header image are illustrative.

### Files

- `src/main.rs`: app state, timers, window open/close, menu bar title
- `src/statusbar.rs`: NSStatusItem (cocoa), click callback, screen geometry
- `src/cswap.rs`: bridge to cswap (`list`, `switch`, `add`, `remove`, `alias`, `swap`)
- `src/setup.rs`: finding `claude`/`cswap`/`uv`/`brew`, installing claude-swap, `claude auth login`
- `src/onboarding.rs`: setup assistant window
- `src/i18n.rs`: English/Turkish strings (`l()`, `lf!`) and cswap time strings in Turkish
- `src/dialog.rs`: native NSAlert confirmations and text prompts
- `src/advice.rs`: switch suggestion, pace, notification events
- `src/usage.rs`: JSONL scanner, cache, period summaries
- `src/pricing.rs`: model prices
- `src/assets.rs`: embedded SVG icons (AssetSource)
- `src/theme.rs`: light/dark appearance
- `src/ui.rs`: popover UI
- `src/settings.rs`: preferences model, LaunchAgent
- `src/settings_ui.rs`: settings window and About page

### Website

The landing page lives in its own repository, `swapbar-website`.

## Credits

Icons are [Hugeicons](https://hugeicons.com/?via=afat) stroke-rounded (MIT, in `assets/icons/`). Account data and switching come from [claude-swap](https://github.com/realiti4/claude-swap).

Made by [Mustafa Afat](https://afat.me).
