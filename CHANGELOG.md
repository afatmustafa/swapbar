# Changelog

All notable changes to Swapbar. The website shows the same notes at
[swapbar.afat.me/changelog](https://swapbar.afat.me/changelog).

## Unreleased

### Added

- **Spend limits.** Accounts with a monthly spend limit (Anthropic's extra usage) get a **USD** row with what's spent, the limit and when it resets: "$13.50 / $50.00 · 5d 7h". Seats billed by usage alone, such as some Enterprise seats, have no 5h or 7d window; for them the spend limit is the quota. It colours the bar, decides the switch suggestion and the shortcut's pick, sends the low-quota and reset notifications, and shows in the menu bar title. Beside a plan's 5h and 7d windows it stays informational, as in cswap.

## 0.4.1

### Fixed

- The pace warning under an account row was cut off with an ellipsis when it didn't fit, which in Turkish hid the time the quota runs out. It wraps onto a second line now.

Thanks to [@hozdemir](https://github.com/hozdemir) for the fix.

## 0.4.0

### Added

- **Login expiry warning.** Three days before an account's saved login expires, its row says "login expires in 2 days · Sign in again", and the link opens the sign-in flow. Settings › Accounts shows the same line, and a notification warns once per login. Turn the notification off in Settings › Notifications. Needs claude-swap 0.27 or later.
- **Why usage is missing.** When cswap can't fetch an account's usage, the row now says why and when it tries again: "couldn't fetch usage: rate limited · retrying at 14:32". Needs claude-swap 0.27 or later; older versions still show "couldn't fetch usage".
- **claude-swap updates.** The update check also looks for a newer claude-swap on PyPI. When there is one, the popover and Settings › General offer **Update**, which runs `cswap upgrade`. Pre-releases are never offered.

### Changed

- **Bar colours follow the low-quota level.** A bar turns red at the level set in Settings › Notifications › Low quota at, and yellow 25 points above it. Before, the bars turned red at 15% and yellow at 40% whatever the setting, so a bar could stay green while the banner and the notification already called the account low. The default 15% keeps the old 15 / 40 split.

### Fixed

- One malformed entry in `cswap list --json`, such as a `null` percentage in one window, hid every account behind "couldn't read `cswap list` output". Only the part that doesn't fit is left out now, and the other accounts still show.
- The token and cost summary could miscount. One invalid byte in a transcript left that whole file out and had it re-read on every refresh. Transcript lines over 512 KB, such as a reply that writes a large file, were skipped. In time zones where a DST change skips or repeats midnight, Today, 7 days and 30 days all counted the full 31 days.
- At launch, notification permission is now also requested when only the pace warning is on.
- "unknown account" in the cost list is translated.

Thanks to [@musabustun](https://github.com/musabustun) for the colour, cost summary and `cswap list` fixes.

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
