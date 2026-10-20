# Changelog

All notable changes to Swapbar. The website shows the same notes at
[swapbar.afat.me/changelog](https://swapbar.afat.me/changelog).

## Unreleased

### Added

- **Windows.** Swapbar runs on Windows 10 and 11 as a system tray app, from the same code: the popover, Settings, the setup assistant, account management, notifications, global shortcuts (`Ctrl+Alt+S`, `Ctrl+Alt+Shift+S`) and launch at login. The tray icon shows the active account's 5h percentage in place of the logo, with both windows in its tooltip, and a right-click menu opens Settings or quits. Notifications have no **Switch** button yet; clicking one opens Swapbar. Releases include `Swapbar-windows-x64.zip`.

## 0.3.0

### Added

- **Turkish.** The whole interface (popover, Settings, setup assistant, notifications and dialogs) is in English or Turkish. It follows the macOS language by default; pick one in Settings › General › Language.
- **Setup assistant.** On first run (no claude-swap, or no accounts) a setup window checks for Claude Code and claude-swap and installs claude-swap with `uv` (and `uv` itself with Homebrew or Astral's installer if needed). It then adds your accounts one at a time: Claude Code signs in to each in the browser (`claude auth login`), cswap saves the login, and Claude Code switches back to the account it was using. Open it again from Settings › Accounts.
- **Account management.** Settings › Accounts can add an account, rename it (alias), move it up or down, remove it, and **Sign in again** when its saved login stopped working or belongs to another account. cswap's duplicate-login warning shows on that page.
- **Pace warnings.** When an account is using a window faster than an even pace would, a small marker on its bar shows where it should be by now. If it would run out before the window resets, the row says when: "At this pace, 5h runs out around 14:53, before its 16:38 reset". A notification gives the same warning for the current account once per window, with a **Switch** button when another account has clearly more room. Turn the marker off in Settings › Menu bar, and the notification in Settings › Notifications. The projection comes from cswap.

### Fixed

- Account errors in the popover read as words ("couldn't fetch usage", "saved login belongs to another account") instead of cswap's status codes.
- Accounts whose latest usage fetch failed (for example rate-limited by the usage endpoint) showed only "unavailable". They now show cswap's last good reading, and how old it is, as `cswap watch` does.

## 0.2.0

### Added

- **Quota notifications.** A notification when the current account runs low, with a **Switch** button for the account with the most quota left, and another when a depleted account's quota resets. Turn each on or off, and set the low-quota level, in Settings › Notifications.
- **Switch suggestion.** Once the current account's tightest window reaches the low-quota level (15% left by default), the popover shows which account has the most room and switches to it in one click. That account is marked **suggested** in the list.
- **Global shortcuts.** `⌃⌥S` opens Swapbar from any app, and `⌃⌥⇧S` switches to the account with the most quota left. No Accessibility permission needed; turn them off in Settings › General.
- **Cost per account.** The usage section splits cost by account. The account comes from cswap's switch log for the default profile, from the slot number for `cswap run` sessions, and from the login of each other profile.
- **Update check.** Swapbar looks for a new release on GitHub at launch and every 6 hours and shows a **Download** banner when one is out. Check by hand, or turn it off, in Settings › General.
- **Homebrew.** `brew install --cask afatmustafa/tap/swapbar`.

### Fixed

- The popover could stay on screen after opening Settings and then ignore clicks outside it and on the menu bar icon.
- Amounts that round up to a whole dollar showed as `$0.100` instead of `$1.00`.

## 0.1.1

### Changed

- Releases are signed with a Developer ID and notarized by Apple, so Swapbar opens with a double-click.
- The DMG background uses the Swapbar mark in its menu bar.

### Fixed

- The menu bar popover sometimes didn't close, or reopened right after closing.

## 0.1.0

First release: every cswap account's 5h, 7d and per-model quota in the menu bar, one-click switching, token and cost totals from Claude Code transcripts, light and dark mode, and a settings window for cswap's auto-switch options.
