//! Bridge to the `cswap` CLI (claude-swap). cswap owns credentials, the
//! Anthropic usage API cadence (rate limited to ~30 req/h/account) and the
//! switch transaction, so we never reimplement any of it here.

use crate::i18n::l;
use crate::lf;
use crate::setup::{self, Tool};
use chrono::{DateTime, Duration, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// Warn this long before a saved login lapses.
pub const LOGIN_WARN_DAYS: i64 = 3;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub pct: f64,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub countdown: Option<String>,
    #[serde(default)]
    pub clock: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub expected_pct: Option<f64>,
    /// cswap's pace check: used more than `expected_pct` at this point of the window.
    #[serde(default)]
    pub ahead_of_pace: Option<bool>,
    /// When the window runs out at the current pace (cswap's linear projection).
    #[serde(default)]
    pub projected_exhaustion_at: Option<String>,
    #[serde(default)]
    pub will_last_to_reset: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub used: f64,
    pub limit: f64,
    pub pct: f64,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default, deserialize_with = "lenient")]
    pub five_hour: Option<Window>,
    #[serde(default, deserialize_with = "lenient")]
    pub seven_day: Option<Window>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub scoped: Vec<Window>,
    #[serde(default, deserialize_with = "lenient")]
    pub spend: Option<Spend>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub number: u32,
    pub email: String,
    #[serde(default)]
    pub organization_name: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub usage_status: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub usage_age_seconds: Option<f64>,
    /// cswap's last successful reading, kept while fresh fetches fail (e.g. HTTP 429).
    #[serde(default, deserialize_with = "lenient")]
    pub last_good_usage: Option<Usage>,
    #[serde(default)]
    pub last_good_age_seconds: Option<f64>,
    /// Why the last usage fetch failed ("http-429", "timeout", …) on an `unavailable`
    /// row (cswap 0.27+).
    #[serde(default, deserialize_with = "lenient")]
    pub usage_error: Option<String>,
    /// When cswap fetches again while it backs off from `usage_error` (ISO-8601).
    #[serde(default, deserialize_with = "lenient")]
    pub usage_retry_at: Option<String>,
    /// When the saved login's refresh token lapses and the account needs a fresh
    /// sign-in (ISO-8601, cswap 0.27+). Absent when Claude Code didn't record it.
    #[serde(default, deserialize_with = "lenient")]
    pub login_expires_at: Option<String>,
    /// `usage` holds `last_good_usage` because the latest fetch failed.
    #[serde(skip)]
    pub stale: bool,
    /// Plan label ("Max 20x", "Premium", …), read from cswap's saved Claude profile.
    #[serde(skip)]
    pub plan: Option<String>,
}

impl Account {
    /// The alias if there is one, else the email.
    pub fn name(&self) -> &str {
        self.alias
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or(&self.email)
    }

    /// The saved login is unusable until the account signs in again.
    pub fn needs_login(&self) -> bool {
        matches!(
            self.usage_status.as_deref(),
            Some("relogin_required" | "foreign_credential" | "no_credentials")
        )
    }

    /// cswap's `usageStatus`, worded for people.
    pub fn status_label(&self) -> Option<String> {
        let st = self.usage_status.as_deref().filter(|s| *s != "ok")?;
        if st == "unavailable" {
            return Some(self.unavailable_label(Utc::now()));
        }
        Some(
            match st {
                "token_expired" => l(
                    "token expired, cswap refreshes it",
                    "token süresi doldu, cswap yeniliyor",
                ),
                "relogin_required" => l(
                    "signed out, sign in again",
                    "oturum kapandı, yeniden giriş yap",
                ),
                "foreign_credential" => l(
                    "saved login belongs to another account",
                    "kayıtlı giriş başka bir hesaba ait",
                ),
                "no_credentials" => l("no saved login", "kayıtlı giriş yok"),
                "keychain_unavailable" => l("Keychain is locked", "Anahtar Zinciri kilitli"),
                "api_key" => l("API key, no plan quota", "API anahtarı, plan kotası yok"),
                other => other,
            }
            .to_string(),
        )
    }

    /// "couldn't fetch usage: rate limited · retrying at 14:32", with the cause and retry
    /// time when cswap reports them.
    fn unavailable_label(&self, now: DateTime<Utc>) -> String {
        let mut s = l("couldn't fetch usage", "kullanım alınamadı").to_string();
        if let Some(kind) = self.usage_error.as_deref().filter(|k| !k.is_empty()) {
            s.push_str(": ");
            s.push_str(&usage_error_text(kind));
        }
        if let Some(at) = parse_time(&self.usage_retry_at).filter(|t| *t > now) {
            let clock = crate::i18n::time(&crate::advice::local_clock(at, now));
            s.push_str(&lf!(" · retrying at {clock}", " · yeniden deneme {clock}"));
        }
        s
    }

    /// Time left on the saved login, once it is within `LOGIN_WARN_DAYS` of lapsing.
    /// `None` when the login already needs a sign-in, since the status says so.
    pub fn login_expires_in(&self, now: DateTime<Utc>) -> Option<Duration> {
        if self.needs_login() {
            return None;
        }
        let left = parse_time(&self.login_expires_at)? - now;
        (left <= Duration::days(LOGIN_WARN_DAYS)).then_some(left.max(Duration::zero()))
    }

    /// "login expires in 2 days", while `login_expires_in` warns.
    pub fn login_expiry_label(&self, now: DateTime<Utc>) -> Option<String> {
        let left = self.login_expires_in(now)?;
        Some(if left.is_zero() {
            l("login expired", "girişin süresi doldu").into()
        } else {
            let when = expires_in_text(left);
            lf!("login expires {when}", "giriş {when} sona eriyor")
        })
    }

    /// Show the last good reading when the latest fetch failed, as `cswap watch` does.
    /// `usage_status` keeps the failure, so advice still ignores the account.
    fn fall_back_to_last_good(&mut self) {
        if self.usage.is_none() && self.last_good_usage.is_some() {
            self.usage = self.last_good_usage.take();
            self.usage_age_seconds = self.last_good_age_seconds;
            self.stale = true;
        }
    }

    pub fn org_tag(&self) -> String {
        match &self.organization_name {
            Some(n) if !n.is_empty() => n.clone(),
            _ => l("personal", "kişisel").to_string(),
        }
    }
}

fn parse_time(s: &Option<String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.as_deref()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// cswap's fetch-failure kind, worded for people.
fn usage_error_text(kind: &str) -> String {
    match kind {
        "http-429" => l("rate limited", "istek sınırı").into(),
        "timeout" => l("timed out", "zaman aşımı").into(),
        "network" => l("no connection", "bağlantı yok").into(),
        "bad-response" => l("unexpected response", "beklenmeyen yanıt").into(),
        k if k.starts_with("http-5") => lf!("server error ({k})", "sunucu hatası ({k})"),
        k => k.to_string(),
    }
}

/// "in 2 days", "in 5 hours", "within an hour".
pub fn expires_in_text(left: Duration) -> String {
    let hours = left.num_hours();
    if hours >= 48 {
        lf!("in {} days", "{} gün içinde", hours / 24)
    } else if hours >= 1 {
        lf!("in {hours} hours", "{hours} saat içinde")
    } else {
        l("within an hour", "bir saat içinde").into()
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountList {
    #[serde(default)]
    pub active_account_number: Option<u32>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub accounts: Vec<Account>,
    /// "Account-1 and Account-2 hold the same credential (…)": one slot's login was
    /// overwritten with another account's.
    #[serde(default, deserialize_with = "lenient_vec")]
    pub duplicate_account_warnings: Vec<String>,
}

/// A field of `cswap list` that doesn't have the expected shape (a `null` pct, a
/// missing email, a new cswap's change) reads as absent instead of failing the
/// whole list, so the other accounts still show.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(from_value_or_log(serde_json::Value::deserialize(d)?))
}

/// `lenient` per item: entries that don't fit are dropped, the rest kept.
fn lenient_vec<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let items = Option::<Vec<serde_json::Value>>::deserialize(d)?.unwrap_or_default();
    Ok(items.into_iter().filter_map(from_value_or_log).collect())
}

fn from_value_or_log<T: DeserializeOwned>(v: serde_json::Value) -> Option<T> {
    if v.is_null() {
        return None;
    }
    serde_json::from_value(v)
        .inspect_err(|e| {
            if std::env::var("SWAPBAR_DEBUG").is_ok() {
                eprintln!("skipping part of `cswap list`: {e}");
            }
        })
        .ok()
}

impl AccountList {
    /// Slot numbers named in the duplicate-credential warnings.
    pub fn duplicate_slots(&self) -> Vec<u32> {
        let mut slots: Vec<u32> = self
            .duplicate_account_warnings
            .iter()
            .flat_map(|w| w.split("Account-").skip(1))
            .filter_map(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}

fn run(args: &[&str]) -> Result<String, String> {
    run_with_input(args, None)
}

/// Run cswap, answering its `[y/N]` prompt with `input` when given (cswap has no `--yes`).
fn run_with_input(args: &[&str], input: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    let bin = setup::find(Tool::Cswap).ok_or(l("cswap isn't installed", "cswap kurulu değil"))?;
    let mut child = setup::command(&bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start cswap: {e}"))?;
    if let Some(mut stdin) = child.stdin.take()
        && let Some(input) = input
    {
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("cswap {}: {e}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        };
        return Err(format!("cswap {}: {}", args.join(" "), strip_ansi(&msg)));
    }
    Ok(stdout)
}

/// cswap colours its messages even when piped.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn list() -> Result<AccountList, String> {
    let raw = run(&["list", "--json"])?;
    let mut list: AccountList = serde_json::from_str(&raw)
        .map_err(|e| format!("couldn't read `cswap list` output: {e}"))?;
    for acc in &mut list.accounts {
        acc.fall_back_to_last_good();
        acc.plan = plan_for(acc.number);
    }
    Ok(list)
}

// ---- Plan labels ---------------------------------------------------------

/// The `oauthAccount` block Claude Code keeps in its config; cswap saves one
/// copy per account as `~/.claude-swap-backup/configs/.claude-config-<n>-<email>.json`.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct OauthAccount {
    #[serde(default)]
    organization_type: Option<String>,
    #[serde(default)]
    organization_rate_limit_tier: Option<String>,
    #[serde(default)]
    user_rate_limit_tier: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SavedConfig {
    #[serde(default)]
    oauth_account: Option<OauthAccount>,
}

fn plan_for(number: u32) -> Option<String> {
    let dir = setup::home().join(".claude-swap-backup").join("configs");
    let prefix = format!(".claude-config-{number}-");
    let path = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".json"))
        })?;
    let cfg: SavedConfig = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    plan_label(&cfg.oauth_account?)
}

fn plan_label(a: &OauthAccount) -> Option<String> {
    let tier = |t: &Option<String>| t.as_deref().unwrap_or("").to_string();
    let max = |t: &str| {
        if t.contains("20x") {
            "Max 20x"
        } else if t.contains("5x") {
            "Max 5x"
        } else {
            "Max"
        }
    };
    let label = match a.organization_type.as_deref()? {
        "claude_max" => max(&tier(&a.organization_rate_limit_tier)),
        "claude_pro" => "Pro",
        // Team and Enterprise seats: a Premium seat carries a per-user Max rate-limit tier.
        "claude_team" | "claude_enterprise" => {
            if tier(&a.user_rate_limit_tier).contains("max") {
                "Premium"
            } else {
                "Standard"
            }
        }
        _ => return None,
    };
    Some(label.to_string())
}

pub fn switch(number: u32) -> Result<(), String> {
    run(&["switch", &number.to_string(), "--json"]).map(|_| ())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    #[serde(default)]
    pub value: serde_json::Value,
    #[serde(default)]
    pub is_set: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigList {
    #[serde(default)]
    pub settings: Vec<ConfigEntry>,
}

pub fn config_list() -> Result<ConfigList, String> {
    let raw = run(&["config", "--json"])?;
    serde_json::from_str(&raw).map_err(|e| format!("couldn't read `cswap config` output: {e}"))
}

pub fn config_set(key: &str, value: &str) -> Result<(), String> {
    run(&["config", "set", key, value]).map(|_| ())
}

pub fn config_unset(key: &str) -> Result<(), String> {
    run(&["config", "unset", key]).map(|_| ())
}

pub fn set_enabled(number: u32, enabled: bool) -> Result<(), String> {
    run(&[
        if enabled { "enable" } else { "disable" },
        &number.to_string(),
    ])
    .map(|_| ())
}

/// Save the account Claude Code is signed in to: a new slot, or fresh credentials for
/// the slot that already holds it.
pub fn add_current(alias: Option<&str>) -> Result<(), String> {
    let mut args = vec!["add"];
    if let Some(alias) = alias {
        args.extend(["--alias", alias]);
    }
    run(&args).map(|_| ())
}

pub fn remove(number: u32) -> Result<(), String> {
    run_with_input(&["remove", &number.to_string()], Some("y\n")).map(|_| ())
}

pub fn set_alias(number: u32, alias: Option<&str>) -> Result<(), String> {
    let n = number.to_string();
    match alias {
        Some(a) => run(&["alias", &n, a]),
        None => run(&["alias", &n, "--unset"]),
    }
    .map(|_| ())
}

/// Update claude-swap with the tool that installed it (`uv tool` or `pipx`).
pub fn upgrade() -> Result<(), String> {
    if cfg!(target_os = "windows") {
        // There `cswap upgrade` only prints the uv command: a running cswap.exe can't be
        // replaced. Swapbar isn't cswap, so it can run that command itself.
        return upgrade_with_uv();
    }
    run(&["upgrade"]).map(|_| ())
}

/// `uv tool upgrade claude-swap`, the command `cswap upgrade` runs for uv installs.
fn upgrade_with_uv() -> Result<(), String> {
    const LABEL: &str = "uv tool upgrade claude-swap";
    let uv = setup::find(Tool::Uv).ok_or(lf!(
        "uv isn't installed. Update claude-swap the way you installed it, e.g. `{LABEL}`.",
        "uv kurulu değil. claude-swap'ı kurduğun yolla güncelle, örneğin `{LABEL}`."
    ))?;
    let out = setup::command(&uv)
        .args(["tool", "upgrade", "claude-swap"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{LABEL}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(format!("{LABEL}: {}", strip_ansi(&msg)))
}

/// Exchange two accounts' slot numbers.
pub fn swap_slots(a: u32, b: u32) -> Result<(), String> {
    run(&["swap", &a.to_string(), &b.to_string()]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth(org_type: &str, org_tier: &str, user_tier: &str) -> OauthAccount {
        OauthAccount {
            organization_type: Some(org_type.into()),
            organization_rate_limit_tier: Some(org_tier.into()),
            user_rate_limit_tier: Some(user_tier.into()),
        }
    }

    #[test]
    fn plan_labels() {
        let label = |a: OauthAccount| plan_label(&a);
        assert_eq!(
            label(oauth("claude_max", "default_claude_max_20x", "")).as_deref(),
            Some("Max 20x")
        );
        assert_eq!(
            label(oauth("claude_max", "default_claude_max_5x", "")).as_deref(),
            Some("Max 5x")
        );
        assert_eq!(label(oauth("claude_max", "", "")).as_deref(), Some("Max"));
        assert_eq!(label(oauth("claude_pro", "", "")).as_deref(), Some("Pro"));
        assert_eq!(
            label(oauth("claude_team", "", "default_claude_max_5x")).as_deref(),
            Some("Premium")
        );
        assert_eq!(
            label(oauth("claude_enterprise", "", "")).as_deref(),
            Some("Standard")
        );
        assert_eq!(label(oauth("api", "", "")), None);
        assert_eq!(plan_label(&OauthAccount::default()), None);
    }

    #[test]
    fn saved_config_reads_the_oauth_block() {
        let cfg: SavedConfig = serde_json::from_str(
            r#"{"numStartups":3,"oauthAccount":{"organizationType":"claude_pro","emailAddress":"a@x.com"}}"#,
        )
        .unwrap();
        assert_eq!(
            plan_label(&cfg.oauth_account.unwrap()).as_deref(),
            Some("Pro")
        );
    }

    // Trimmed from real `cswap list --json` output (cswap 0.26).
    const LIST: &str = r#"{
      "activeAccountNumber": 2,
      "accounts": [
        {"number": 1, "email": "a@x.com", "organizationName": "a@x.com's Organization",
         "isOrganization": true, "active": false, "usageStatus": "ok",
         "usage": {
           "fiveHour": {"pct": 3.0, "resetsAt": "2026-10-18T19:30:00.241009+00:00", "countdown": "7m", "clock": "22:30"},
           "sevenDay": {"pct": 61.0, "countdown": "2d 7h", "clock": "Oct 21 06:00", "expectedPct": 66.8, "aheadOfPace": false},
           "scoped": [{"pct": 0.0, "countdown": "2d 7h", "name": "Fable"}]
         },
         "usageFetchedAt": "2026-10-18T19:18:21Z", "usageAgeSeconds": 224.4},
        {"number": 2, "email": "b@x.com", "organizationName": "", "active": true, "disabled": true,
         "usage": {"fiveHour": {"pct": 2.0}, "spend": {"used": 1.5, "limit": 10, "pct": 15, "currency": "USD"}}},
        {"number": 3, "email": "c@x.com", "usageStatus": "token expired"},
        {"number": 4, "email": "d@x.com", "usageStatus": "unavailable", "usage": null,
         "lastGoodUsage": {"fiveHour": {"pct": 0.0}, "sevenDay": {"pct": 63.0, "countdown": "1d 14h"}},
         "lastGoodFetchedAt": "2026-10-19T07:47:16Z", "lastGoodAgeSeconds": 16875.8}
      ]
    }"#;

    #[test]
    fn parses_list_json() {
        let list: AccountList = serde_json::from_str(LIST).unwrap();
        assert_eq!(list.active_account_number, Some(2));
        assert_eq!(list.accounts.len(), 4);

        let a = &list.accounts[0];
        let u = a.usage.as_ref().unwrap();
        assert_eq!(u.five_hour.as_ref().unwrap().pct, 3.0);
        assert_eq!(
            u.five_hour.as_ref().unwrap().clock.as_deref(),
            Some("22:30")
        );
        assert_eq!(u.seven_day.as_ref().unwrap().expected_pct, Some(66.8));
        assert_eq!(u.scoped[0].name.as_deref(), Some("Fable"));
        assert_eq!(a.usage_age_seconds, Some(224.4));
        assert_eq!(a.org_tag(), "a@x.com's Organization");
        assert!(
            a.plan.is_none(),
            "plan is filled in by list(), not by serde"
        );

        let b = &list.accounts[1];
        assert!(b.active && b.disabled);
        assert_eq!(b.org_tag(), "personal");
        assert_eq!(
            b.usage.as_ref().unwrap().spend.as_ref().unwrap().limit,
            10.0
        );

        let c = &list.accounts[2];
        assert!(c.usage.is_none());
        assert_eq!(c.usage_status.as_deref(), Some("token expired"));
    }

    #[test]
    fn a_malformed_account_does_not_hide_the_others() {
        let list: AccountList = serde_json::from_str(
            r#"{"activeAccountNumber": 4, "duplicateAccountWarnings": null, "accounts": [
                {"number": 1, "email": "a@x.com"},
                {"number": 2},
                {"email": "c@x.com"},
                {"number": 4, "email": "d@x.com", "active": true,
                 "usage": {"fiveHour": {"pct": null}, "sevenDay": {"pct": 40.0}}}
            ]}"#,
        )
        .unwrap();
        let numbers: Vec<u32> = list.accounts.iter().map(|a| a.number).collect();
        assert_eq!(numbers, [1, 4]);
        assert!(list.duplicate_account_warnings.is_empty());
        let u = list.accounts[1].usage.as_ref().unwrap();
        assert!(u.five_hour.is_none(), "a window without pct is dropped");
        assert_eq!(u.seven_day.as_ref().unwrap().pct, 40.0);
    }

    #[test]
    fn a_malformed_usage_block_keeps_the_account() {
        let list: AccountList = serde_json::from_str(
            r#"{"accounts": [
                {"number": 1, "email": "a@x.com", "usage": "n/a", "lastGoodUsage": 7},
                {"number": 2, "email": "b@x.com", "usage": {
                    "fiveHour": {"pct": "3"},
                    "spend": {"used": null, "limit": 10, "pct": 0},
                    "scoped": [{"pct": null, "name": "Opus"}, {"pct": 1.0, "name": "Fable"}]
                }}
            ]}"#,
        )
        .unwrap();
        assert_eq!(list.accounts.len(), 2);
        let a = &list.accounts[0];
        assert!(a.usage.is_none() && a.last_good_usage.is_none());
        let u = list.accounts[1].usage.as_ref().unwrap();
        assert!(u.five_hour.is_none() && u.spend.is_none());
        assert_eq!(u.scoped.len(), 1);
        assert_eq!(u.scoped[0].name.as_deref(), Some("Fable"));
    }

    #[test]
    fn parses_config_json() {
        let cfg: ConfigList = serde_json::from_str(
            r#"{"settings":[{"key":"autoswitch.threshold","value":90,"isSet":true},{"key":"autoswitch.model","value":null}]}"#,
        )
        .unwrap();
        assert_eq!(cfg.settings.len(), 2);
        assert!(cfg.settings[0].is_set);
        assert_eq!(cfg.settings[0].value.as_f64(), Some(90.0));
        assert!(!cfg.settings[1].is_set);
    }

    #[test]
    fn failed_fetch_falls_back_to_last_good_usage() {
        let mut list: AccountList = serde_json::from_str(LIST).unwrap();
        for acc in &mut list.accounts {
            acc.fall_back_to_last_good();
        }
        let d = &list.accounts[3];
        assert!(d.stale);
        assert_eq!(d.usage_status.as_deref(), Some("unavailable"));
        assert_eq!(
            d.usage.as_ref().unwrap().seven_day.as_ref().unwrap().pct,
            63.0
        );
        assert_eq!(d.usage_age_seconds, Some(16875.8));
        assert!(!list.accounts[0].stale, "a fresh reading is left alone");
        assert!(list.accounts[2].usage.is_none(), "nothing to fall back to");
    }

    #[test]
    fn duplicate_slots_come_from_the_warnings() {
        let list: AccountList = serde_json::from_str(
            r#"{"accounts": [], "duplicateAccountWarnings": [
                "Account-1 and Account-2 hold the same credential (b@x.com) — one slot's backup was overwritten.",
                "Account-2 and Account-12 both authenticate as b@x.com — remove or re-login one of them."
            ]}"#,
        )
        .unwrap();
        assert_eq!(list.duplicate_slots(), [1, 2, 12]);
    }

    #[test]
    fn strips_colour_codes() {
        assert_eq!(
            strip_ansi("\u{1b}[33mSlot 2\u{1b}[0m taken"),
            "Slot 2 taken"
        );
    }

    #[test]
    fn status_labels_and_relogin() {
        let mut a = Account {
            usage_status: Some("foreign_credential".into()),
            ..Default::default()
        };
        assert!(a.needs_login());
        assert_eq!(
            a.status_label().as_deref(),
            Some("saved login belongs to another account")
        );
        a.usage_status = Some("unavailable".into());
        assert!(!a.needs_login());
        a.usage_status = Some("ok".into());
        assert_eq!(a.status_label(), None);
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn reads_the_cswap_027_row_fields() {
        let list: AccountList = serde_json::from_str(
            r#"{"accounts": [{"number": 1, "email": "a@x.com", "usageStatus": "unavailable",
                "usage": null, "usageError": "http-429", "usageRetryAt": "2026-10-21T14:32:00Z",
                "loginExpiresAt": "2026-10-23T09:00:00Z"}]}"#,
        )
        .unwrap();
        let a = &list.accounts[0];
        assert_eq!(a.usage_error.as_deref(), Some("http-429"));
        assert_eq!(a.usage_retry_at.as_deref(), Some("2026-10-21T14:32:00Z"));
        assert_eq!(a.login_expires_at.as_deref(), Some("2026-10-23T09:00:00Z"));
    }

    #[test]
    fn an_unavailable_row_says_why_and_when_it_retries() {
        let now = at("2026-10-21T14:00:00Z");
        let mut a = Account {
            usage_status: Some("unavailable".into()),
            ..Default::default()
        };
        assert_eq!(a.unavailable_label(now), "couldn't fetch usage");
        a.usage_error = Some("http-429".into());
        assert_eq!(
            a.unavailable_label(now),
            "couldn't fetch usage: rate limited"
        );
        let retry = at("2026-10-21T14:32:00Z");
        a.usage_retry_at = Some("2026-10-21T14:32:00Z".into());
        assert_eq!(
            a.unavailable_label(now),
            format!(
                "couldn't fetch usage: rate limited · retrying at {}",
                crate::advice::local_clock(retry, now)
            )
        );
        // a retry time already behind us says nothing
        assert_eq!(
            a.unavailable_label(at("2026-10-21T15:00:00Z")),
            "couldn't fetch usage: rate limited"
        );
        a.usage_error = Some("http-503".into());
        a.usage_retry_at = None;
        assert_eq!(
            a.unavailable_label(now),
            "couldn't fetch usage: server error (http-503)"
        );
    }

    #[test]
    fn warns_about_a_login_only_near_its_expiry() {
        let now = at("2026-10-21T12:00:00Z");
        let mut a = Account {
            usage_status: Some("ok".into()),
            login_expires_at: Some("2026-10-30T12:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(a.login_expiry_label(now), None, "9 days out");
        a.login_expires_at = Some("2026-10-23T18:00:00Z".into());
        assert_eq!(
            a.login_expiry_label(now).as_deref(),
            Some("login expires in 2 days")
        );
        a.login_expires_at = Some("2026-10-22T05:00:00Z".into());
        assert_eq!(
            a.login_expiry_label(now).as_deref(),
            Some("login expires in 17 hours")
        );
        a.login_expires_at = Some("2026-10-21T12:20:00Z".into());
        assert_eq!(
            a.login_expiry_label(now).as_deref(),
            Some("login expires within an hour")
        );
        a.login_expires_at = Some("2026-10-20T12:00:00Z".into());
        assert_eq!(a.login_expiry_label(now).as_deref(), Some("login expired"));
        // once cswap reports the login as dead, the status line covers it
        a.usage_status = Some("relogin_required".into());
        assert_eq!(a.login_expiry_label(now), None);
        a.usage_status = Some("ok".into());
        a.login_expires_at = None;
        assert_eq!(a.login_expiry_label(now), None);
    }
}
