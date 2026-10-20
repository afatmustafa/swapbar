//! App preferences persisted at `~/.config/swapbar/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::i18n::{l, os};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TitleMode {
    /// Icon only
    Off,
    /// 5h remaining
    FiveHour,
    /// 5h · 7d remaining
    Both,
}

impl TitleMode {
    pub const ALL: [TitleMode; 3] = [TitleMode::Off, TitleMode::FiveHour, TitleMode::Both];
    pub fn label(self) -> &'static str {
        match self {
            // the Windows tray shows the logo or a number, both icons
            TitleMode::Off => os(l("Icon only", "Yalnızca ikon"), "Logo"),
            TitleMode::FiveHour => "5h",
            TitleMode::Both => "5h · 7d",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PercentMode {
    Remaining,
    Used,
}

impl PercentMode {
    pub const ALL: [PercentMode; 2] = [PercentMode::Remaining, PercentMode::Used];
    pub fn label(self) -> &'static str {
        match self {
            PercentMode::Remaining => l("Remaining", "Kalan"),
            PercentMode::Used => l("Used", "Kullanılan"),
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            PercentMode::Remaining => l("left", "kaldı"),
            PercentMode::Used => l("used", "kullanıldı"),
        }
    }
    /// Value to display for a window reporting `used` percent.
    pub fn value(self, used: f64) -> f64 {
        match self {
            PercentMode::Remaining => (100.0 - used).clamp(0.0, 100.0),
            PercentMode::Used => used.clamp(0.0, 100.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    System,
    English,
    Turkish,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::System, Language::English, Language::Turkish];
    pub fn label(self) -> &'static str {
        match self {
            Language::System => l("System", "Sistem"),
            // each language in its own name
            Language::English => "English",
            Language::Turkish => "Türkçe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];
    pub fn label(self) -> &'static str {
        match self {
            Theme::System => l("System", "Sistem"),
            Theme::Light => l("Light", "Açık"),
            Theme::Dark => l("Dark", "Koyu"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: Theme,
    pub language: Language,
    pub title_mode: TitleMode,
    pub percent_mode: PercentMode,
    pub show_usage: bool,
    pub show_seven_day: bool,
    pub show_model_limits: bool,
    pub show_reset_times: bool,
    /// Mark windows used faster than their pace and say when they would run out.
    pub show_pace: bool,
    /// Account slot numbers hidden from the popover (still switchable via cswap).
    pub hidden_accounts: Vec<u32>,
    pub accounts_refresh_secs: u64,
    pub usage_refresh_secs: u64,
    pub launch_at_login: bool,
    /// Remaining percentage at which a window counts as low (notifications, switch suggestion).
    pub low_quota_pct: u32,
    pub notify_low: bool,
    /// Warn when the current account's pace runs a window out before it resets.
    pub notify_pace: bool,
    pub notify_reset: bool,
    pub check_updates: bool,
    pub hotkeys: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: Theme::System,
            language: Language::System,
            title_mode: TitleMode::FiveHour,
            percent_mode: PercentMode::Remaining,
            show_usage: true,
            show_seven_day: true,
            show_model_limits: true,
            show_reset_times: true,
            show_pace: true,
            hidden_accounts: Vec::new(),
            accounts_refresh_secs: 60,
            usage_refresh_secs: 120,
            launch_at_login: false,
            low_quota_pct: 15,
            notify_low: true,
            notify_pace: true,
            notify_reset: true,
            check_updates: true,
            hotkeys: true,
        }
    }
}

fn path() -> PathBuf {
    crate::usage::data_dir().join("settings.json")
}

impl Prefs {
    pub fn load() -> Prefs {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }

    pub fn is_hidden(&self, number: u32) -> bool {
        self.hidden_accounts.contains(&number)
    }

    pub fn toggle_hidden(&mut self, number: u32) {
        if let Some(i) = self.hidden_accounts.iter().position(|n| *n == number) {
            self.hidden_accounts.remove(i);
        } else {
            self.hidden_accounts.push(number);
        }
    }
}

// ---- Launch at login via a LaunchAgent plist ----------------------------

#[cfg(target_os = "macos")]
const AGENT_LABEL: &str = "com.miova.swapbar";

#[cfg(target_os = "macos")]
fn agent_path() -> PathBuf {
    crate::setup::home()
        .join("Library/LaunchAgents")
        .join(format!("{AGENT_LABEL}.plist"))
}

#[cfg(target_os = "macos")]
pub fn launch_agent_installed() -> bool {
    agent_path().exists()
}

#[cfg(target_os = "macos")]
pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let p = agent_path();
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{AGENT_LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
</dict></plist>
"#,
            exe.display()
        );
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, plist).map_err(|e| e.to_string())?;
    } else if p.exists() {
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- Launch at login via the Run key (Windows) --------------------------

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Whether the Run key starts this copy of Swapbar at sign-in.
#[cfg(target_os = "windows")]
pub fn launch_agent_installed() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    windows_registry::CURRENT_USER
        .open(RUN_KEY)
        .and_then(|k| k.get_string(crate::APP_NAME))
        .is_ok_and(|v| {
            v.trim_matches('"')
                .eq_ignore_ascii_case(&exe.display().to_string())
        })
}

#[cfg(target_os = "windows")]
pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let key = windows_registry::CURRENT_USER
        .create(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_string(crate::APP_NAME, format!("\"{}\"", exe.display()))
            .map_err(|e| e.to_string())
    } else {
        match key.remove_value(crate::APP_NAME) {
            Err(e) if e.code() != windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.to_hresult() => {
                Err(e.to_string())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_mode_values_are_clamped() {
        assert_eq!(PercentMode::Remaining.value(30.0), 70.0);
        assert_eq!(PercentMode::Remaining.value(130.0), 0.0);
        assert_eq!(PercentMode::Used.value(30.0), 30.0);
        assert_eq!(PercentMode::Used.value(-5.0), 0.0);
    }

    #[test]
    fn older_settings_files_get_defaults_for_new_keys() {
        let p: Prefs = serde_json::from_str(r#"{"theme":"Dark","hidden_accounts":[3]}"#).unwrap();
        assert_eq!(p.theme, Theme::Dark);
        assert!(p.is_hidden(3));
        assert_eq!(p.low_quota_pct, 15);
        assert!(p.notify_low && p.notify_pace && p.show_pace && p.check_updates && p.hotkeys);
        assert_eq!(p.accounts_refresh_secs, 60);
    }

    #[test]
    fn toggle_hidden_flips_membership() {
        let mut p = Prefs::default();
        p.toggle_hidden(2);
        assert!(p.is_hidden(2));
        p.toggle_hidden(2);
        assert!(!p.is_hidden(2));
    }
}
