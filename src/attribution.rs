//! Which cswap account a transcript record was billed to. Transcripts don't say, so it is
//! worked out from where the file lives:
//!
//! - the default profile (`~/.claude`) follows cswap's switches, so a record belongs to
//!   whichever account `claude-swap.log` says was active at that moment;
//! - a `cswap run` session (`~/.claude-swap-backup/sessions/<num>-<email>/`) is pinned
//!   to its account number;
//! - another profile (`~/.claude-work`, `CLAUDE_CONFIG_DIR`) is logged in on its own,
//!   and counts for the account in its `.claude.json` (the current login, so a profile
//!   that changed accounts within the 30 days is attributed to the newest one).

use chrono::{Local, NaiveDateTime, TimeZone};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Who a record's usage counts for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Owner {
    #[default]
    Unknown,
    /// A cswap account slot, with its email.
    Account(u32, Arc<str>),
    /// Logged in to a profile with an account cswap doesn't manage.
    Email(Arc<str>),
}

impl Owner {
    pub fn label(&self) -> &str {
        match self {
            Owner::Unknown => "unknown account",
            Owner::Account(_, e) | Owner::Email(e) => e,
        }
    }
}

/// Where a projects directory's records come from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The profile cswap switches.
    Default,
    /// A `cswap run` session profile for this account number.
    Session(u32),
    /// A separately logged-in profile, by its login email.
    Profile(Option<String>),
}

/// One `Switched from account X to Y` log line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Switch {
    pub at: i64,
    pub from: u32,
    pub to: u32,
}

/// Parse cswap's log (`2026-10-14 04:04:38,011 - INFO - Switched from account 4 to 3`,
/// local time). Other lines are skipped. The result is sorted by time.
pub fn parse_switch_log(text: &str) -> Vec<Switch> {
    let mut out: Vec<Switch> = text
        .lines()
        .filter_map(|line| {
            let (stamp, rest) = line.split_once(" - ")?;
            let tail = rest.split("Switched from account ").nth(1)?;
            let (from, to) = tail.split_once(" to ")?;
            let to: String = to.chars().take_while(|c| c.is_ascii_digit()).collect();
            let at = NaiveDateTime::parse_from_str(stamp.trim(), "%Y-%m-%d %H:%M:%S,%3f").ok()?;
            Some(Switch {
                at: Local.from_local_datetime(&at).earliest()?.timestamp(),
                from: from.trim().parse().ok()?,
                to: to.parse().ok()?,
            })
        })
        .collect();
    out.sort_by_key(|s| s.at);
    out
}

/// Account active in the default profile at `t`: the target of the last switch before
/// `t`, the origin of the first switch for earlier times, or `current` without history.
pub fn account_at(switches: &[Switch], t: i64, current: Option<u32>) -> Option<u32> {
    let after = switches.partition_point(|s| s.at <= t);
    match after {
        0 => switches.first().map(|s| s.from).or(current),
        n => Some(switches[n - 1].to),
    }
}

/// Account number of a `cswap run` session directory (`3-me-example-com`).
pub fn session_account(dir_name: &str) -> Option<u32> {
    let (num, _) = dir_name.split_once('-')?;
    num.parse().ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileConfig {
    oauth_account: Option<OauthAccount>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OauthAccount {
    email_address: Option<String>,
}

/// Login email from a profile's `.claude.json`.
pub fn profile_email(json: &str) -> Option<String> {
    serde_json::from_str::<ProfileConfig>(json)
        .ok()?
        .oauth_account?
        .email_address
        .filter(|e| !e.is_empty())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Sequence {
    #[serde(default)]
    active_account_number: Option<u32>,
    #[serde(default)]
    accounts: HashMap<String, SequenceAccount>,
}

#[derive(Deserialize)]
struct SequenceAccount {
    email: String,
}

/// Everything needed to attribute records, read once per scan.
#[derive(Debug, Default)]
pub struct Attribution {
    switches: Vec<Switch>,
    current: Option<u32>,
    /// slot number → email
    emails: HashMap<u32, Arc<str>>,
}

impl Attribution {
    /// Build from cswap's `sequence.json` and log text (already concatenated).
    pub fn from_parts(sequence_json: &str, log: &str) -> Attribution {
        let seq: Sequence = serde_json::from_str(sequence_json).unwrap_or_default();
        Attribution {
            switches: parse_switch_log(log),
            current: seq.active_account_number,
            emails: seq
                .accounts
                .into_iter()
                .filter_map(|(n, a)| Some((n.parse().ok()?, Arc::from(a.email))))
                .collect(),
        }
    }

    /// Read from `~/.claude-swap-backup`, including rotated logs.
    pub fn load() -> Attribution {
        let dir = backup_dir();
        let sequence = std::fs::read_to_string(dir.join("sequence.json")).unwrap_or_default();
        let mut log = String::new();
        for name in [
            "claude-swap.log.3",
            "claude-swap.log.2",
            "claude-swap.log.1",
            "claude-swap.log",
        ] {
            if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
                log.push_str(&text);
                log.push('\n');
            }
        }
        Attribution::from_parts(&sequence, &log)
    }

    fn account(&self, number: u32) -> Owner {
        match self.emails.get(&number) {
            Some(e) => Owner::Account(number, e.clone()),
            None => Owner::Account(number, Arc::from(format!("account {number}"))),
        }
    }

    pub fn owner(&self, source: &Source, t: i64) -> Owner {
        match source {
            Source::Default => match account_at(&self.switches, t, self.current) {
                Some(n) => self.account(n),
                None => Owner::Unknown,
            },
            Source::Session(n) => self.account(*n),
            Source::Profile(Some(email)) => self
                .emails
                .iter()
                .find(|(_, e)| e.eq_ignore_ascii_case(email))
                .map(|(n, e)| Owner::Account(*n, e.clone()))
                .unwrap_or_else(|| Owner::Email(Arc::from(email.as_str()))),
            Source::Profile(None) => Owner::Unknown,
        }
    }
}

pub fn backup_dir() -> PathBuf {
    crate::setup::home().join(".claude-swap-backup")
}

/// Source of a projects directory, given the profile directory that contains it.
pub fn source_for(profile_dir: &Path, home: &Path) -> Source {
    let default_dirs = [home.join(".claude"), home.join(".config").join("claude")];
    if default_dirs.iter().any(|d| d == profile_dir) {
        return Source::Default;
    }
    if profile_dir.parent() == Some(&backup_dir().join("sessions"))
        && let Some(n) = profile_dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(session_account)
    {
        return Source::Session(n);
    }
    let email = std::fs::read_to_string(profile_dir.join(".claude.json"))
        .ok()
        .and_then(|j| profile_email(&j));
    Source::Profile(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> i64 {
        let at = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        Local
            .from_local_datetime(&at)
            .earliest()
            .unwrap()
            .timestamp()
    }

    const LOG: &str = "\
2026-10-07 15:13:40,963 - INFO - Added account 1: a@x.com (org: 300560c6)
2026-10-14 11:33:37,493 - INFO - Switched from account 3 to 1
2026-10-14 04:04:38,011 - INFO - Switched from account 4 to 3
garbage line
2026-10-16 12:52:06,051 - INFO - Switched from account 1 to 2
";

    #[test]
    fn parses_and_sorts_switch_lines() {
        let s = parse_switch_log(LOG);
        assert_eq!(
            s.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>(),
            vec![(4, 3), (3, 1), (1, 2)]
        );
        assert_eq!(s[0].at, ts("2026-10-14 04:04:38"));
    }

    #[test]
    fn account_at_follows_the_timeline() {
        let s = parse_switch_log(LOG);
        assert_eq!(account_at(&s, ts("2026-10-10 00:00:00"), Some(2)), Some(4));
        assert_eq!(account_at(&s, ts("2026-10-14 05:00:00"), Some(2)), Some(3));
        assert_eq!(account_at(&s, ts("2026-10-14 11:33:38"), Some(2)), Some(1));
        assert_eq!(account_at(&s, ts("2026-10-18 00:00:00"), Some(9)), Some(2));
        assert_eq!(account_at(&[], 0, Some(2)), Some(2));
        assert_eq!(account_at(&[], 0, None), None);
    }

    #[test]
    fn session_and_profile_helpers() {
        assert_eq!(session_account("3-me-example-com"), Some(3));
        assert_eq!(session_account("me-example-com"), None);
        assert_eq!(
            profile_email(r#"{"oauthAccount":{"emailAddress":"me@x.com","accountUuid":"u"}}"#),
            Some("me@x.com".into())
        );
        assert_eq!(
            profile_email(r#"{"oauthAccount":{"emailAddress":""}}"#),
            None
        );
        assert_eq!(profile_email("{}"), None);
        assert_eq!(profile_email("not json"), None);
    }

    #[test]
    fn owner_resolves_each_source() {
        let seq = r#"{"activeAccountNumber":2,"accounts":{"1":{"email":"a@x.com"},"2":{"email":"b@x.com"}}}"#;
        let at = Attribution::from_parts(seq, LOG);
        let t = ts("2026-10-15 00:00:00");
        assert_eq!(
            at.owner(&Source::Default, t),
            Owner::Account(1, "a@x.com".into())
        );
        assert_eq!(
            at.owner(&Source::Session(2), t),
            Owner::Account(2, "b@x.com".into())
        );
        assert_eq!(at.owner(&Source::Session(7), t).label(), "account 7");
        assert_eq!(
            at.owner(&Source::Profile(Some("B@x.com".into())), t),
            Owner::Account(2, "b@x.com".into())
        );
        assert_eq!(
            at.owner(&Source::Profile(Some("c@y.com".into())), t),
            Owner::Email("c@y.com".into())
        );
        assert_eq!(at.owner(&Source::Profile(None), t), Owner::Unknown);
    }

    #[test]
    fn without_cswap_data_default_records_are_unknown() {
        let at = Attribution::from_parts("", "");
        assert_eq!(at.owner(&Source::Default, 0), Owner::Unknown);
    }
}
