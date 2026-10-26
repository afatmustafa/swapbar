//! Quota reasoning shared by the popover, the notifications and the hotkeys: how much
//! room an account has left, which account to switch to, and which quota changes are
//! worth a notification. Pure functions over cswap's account list, so they're testable.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Local, Utc};

use crate::cswap::{Account, AccountList, Window};
use crate::settings::Prefs;

/// A target must have at least this many more points of headroom than the current
/// account before we suggest it (the same idea as cswap's hysteresis).
const MIN_GAIN: f64 = 10.0;

/// A window counts as reset when its used percentage falls by at least this much
/// between two polls.
const RESET_DROP: f64 = 30.0;

/// A window turns from green to yellow this many points above the low-quota level.
const WARN_MARGIN: f64 = 25.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Low,
}

/// Colour band for a window with `remaining` percent left: low at the low-quota level,
/// so the bars agree with the suggestion and the notification.
pub fn level(remaining: f64, prefs: &Prefs) -> Level {
    let low = prefs.low_quota_pct as f64;
    if remaining <= low {
        Level::Low
    } else if remaining <= low + WARN_MARGIN {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// The spend limit's name in notifications and the watch, beside "5h" and "7d".
pub const SPEND: &str = "USD";

/// Percentage left in the account's tightest window (5h or 7d), or in its spend limit when
/// that is its only limit; `None` without usage data.
pub fn headroom(acc: &Account) -> Option<f64> {
    if acc.usage_status.as_deref().is_some_and(|s| s != "ok") {
        return None;
    }
    let u = acc.usage.as_ref()?;
    let spend = u.spend_only().map(|s| s.window());
    [u.five_hour.as_ref(), u.seven_day.as_ref(), spend.as_ref()]
        .into_iter()
        .flatten()
        .map(|w| (100.0 - w.pct).clamp(0.0, 100.0))
        .reduce(f64::min)
}

/// The inactive account with the most headroom that is shown in the menu and not paused.
pub fn best_other<'a>(list: &'a AccountList, prefs: &Prefs) -> Option<(&'a Account, f64)> {
    list.accounts
        .iter()
        .filter(|a| !a.active && !a.disabled && !prefs.is_hidden(a.number))
        .filter_map(|a| headroom(a).map(|h| (a, h)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// A pace warning re-fires only for a window whose reset moved by more than this.
const NEW_WINDOW_MINUTES: i64 = 30;

fn parse_time(s: &Option<String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.as_deref()?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// When `w` runs out at the current pace, if that comes before its reset.
pub fn runs_out(w: &Window, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if w.will_last_to_reset != Some(false) || w.pct >= 100.0 {
        return None;
    }
    let out = parse_time(&w.projected_exhaustion_at)?;
    let before_reset = parse_time(&w.resets_at).is_none_or(|reset| out < reset);
    (out > now && before_reset).then_some(out)
}

/// "17:40" today, "Oct 22 09:04" otherwise, in local time like cswap's reset clocks.
pub fn local_clock(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (t, now) = (t.with_timezone(&Local), now.with_timezone(&Local));
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M").to_string()
    } else {
        t.format("%b %-d %H:%M").to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub number: u32,
    pub email: String,
    pub headroom: f64,
    pub current_headroom: f64,
}

/// Suggest a switch once the current account is at or below `low_pct` and another one
/// has clearly more room.
pub fn suggestion(list: &AccountList, prefs: &Prefs) -> Option<Suggestion> {
    let current = headroom(list.accounts.iter().find(|a| a.active)?)?;
    if current > prefs.low_quota_pct as f64 {
        return None;
    }
    switch_target(list, prefs, current)
}

/// The best other account, if it has clearly more room than `current`.
fn switch_target(list: &AccountList, prefs: &Prefs, current: f64) -> Option<Suggestion> {
    let (acc, h) = best_other(list, prefs)?;
    (h >= current + MIN_GAIN).then(|| Suggestion {
        number: acc.number,
        email: acc.email.clone(),
        headroom: h,
        current_headroom: current,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The current account's `window` dropped to `left` percent.
    Low {
        email: String,
        window: &'static str,
        left: f64,
        suggestion: Option<Suggestion>,
    },
    /// At its current pace the current account's `window` runs out before it resets.
    Pace {
        email: String,
        window: &'static str,
        left: f64,
        runs_out: DateTime<Utc>,
        /// cswap's local reset clock ("22:00", "Oct 21 06:00").
        resets: Option<String>,
        suggestion: Option<Suggestion>,
    },
    /// An account whose `window` had run low has quota again.
    Reset {
        number: u32,
        email: String,
        window: &'static str,
        left: f64,
    },
    /// An account's saved login lapses in `left` (zero: already lapsed).
    LoginExpiring {
        number: u32,
        email: String,
        left: Duration,
    },
}

/// Remembers the last used percentage per account and window, so each threshold
/// crossing produces one event rather than one per poll.
#[derive(Debug, Default)]
pub struct Watch {
    last: HashMap<(u32, &'static str), f64>,
    /// Windows already warned about, with their reset time in minutes (0 if unknown).
    paced: HashMap<(u32, &'static str), i64>,
    /// Accounts already warned about an expiring login, with that `loginExpiresAt`.
    login_warned: HashMap<u32, String>,
    primed: bool,
}

impl Watch {
    /// Compare a fresh account list with the previous one. The first list only sets the
    /// baseline: quota that was already low at launch is not news.
    pub fn update(&mut self, list: &AccountList, prefs: &Prefs) -> Vec<Event> {
        self.update_at(list, prefs, Utc::now())
    }

    fn update_at(&mut self, list: &AccountList, prefs: &Prefs, now: DateTime<Utc>) -> Vec<Event> {
        let low_used = 100.0 - prefs.low_quota_pct as f64;
        let mut events = Vec::new();
        let mut next = HashMap::new();
        for acc in &list.accounts {
            self.check_login(acc, now, prefs, &mut events);
            if acc.usage_status.as_deref().is_some_and(|s| s != "ok") {
                // keep the old reading so a failed poll doesn't look like a reset later
                for key in self.last.keys().filter(|k| k.0 == acc.number) {
                    next.insert(*key, self.last[key]);
                }
                continue;
            }
            let Some(u) = acc.usage.as_ref() else {
                continue;
            };
            let spend = u.spend_only().map(|s| s.window());
            for (window, w) in [
                ("5h", u.five_hour.as_ref()),
                ("7d", u.seven_day.as_ref()),
                (SPEND, spend.as_ref()),
            ] {
                let Some(w) = w else { continue };
                let key = (acc.number, window);
                next.insert(key, w.pct);
                let left = (100.0 - w.pct).clamp(0.0, 100.0);
                if acc.active && left > prefs.low_quota_pct as f64 {
                    self.check_pace(acc, window, w, left, now, prefs, list, &mut events);
                }
                let Some(&prev) = self.last.get(&key) else {
                    continue;
                };
                if acc.active && prefs.notify_low && prev < low_used && w.pct >= low_used {
                    events.push(Event::Low {
                        email: acc.email.clone(),
                        window,
                        left,
                        suggestion: suggestion(list, prefs),
                    });
                }
                if prefs.notify_reset
                    && !acc.disabled
                    && !prefs.is_hidden(acc.number)
                    && prev >= low_used
                    && w.pct <= prev - RESET_DROP
                {
                    events.push(Event::Reset {
                        number: acc.number,
                        email: acc.email.clone(),
                        window,
                        left,
                    });
                }
            }
        }
        self.last = next;
        self.primed = true;
        events
    }

    /// One `LoginExpiring` event per login: a fresh sign-in brings a new expiry date.
    /// Unlike quota, a login that is already about to lapse at launch is worth saying.
    fn check_login(
        &mut self,
        acc: &Account,
        now: DateTime<Utc>,
        prefs: &Prefs,
        events: &mut Vec<Event>,
    ) {
        let Some(left) = acc.login_expires_in(now) else {
            return;
        };
        let expiry = acc.login_expires_at.clone().unwrap_or_default();
        if self.login_warned.get(&acc.number) == Some(&expiry) {
            return;
        }
        self.login_warned.insert(acc.number, expiry);
        if prefs.notify_login && !prefs.is_hidden(acc.number) {
            events.push(Event::LoginExpiring {
                number: acc.number,
                email: acc.email.clone(),
                left,
            });
        }
    }

    /// One `Pace` event per window cycle; while priming, only remember the window.
    #[allow(clippy::too_many_arguments)]
    fn check_pace(
        &mut self,
        acc: &Account,
        window: &'static str,
        w: &Window,
        left: f64,
        now: DateTime<Utc>,
        prefs: &Prefs,
        list: &AccountList,
        events: &mut Vec<Event>,
    ) {
        let Some(out) = runs_out(w, now) else {
            return;
        };
        let reset = parse_time(&w.resets_at).map_or(0, |t| t.timestamp() / 60);
        let key = (acc.number, window);
        if self
            .paced
            .get(&key)
            .is_some_and(|&seen| (seen - reset).abs() <= NEW_WINDOW_MINUTES)
        {
            return;
        }
        self.paced.insert(key, reset);
        if self.primed && prefs.notify_pace {
            events.push(Event::Pace {
                email: acc.email.clone(),
                window,
                left,
                runs_out: out,
                resets: w.clock.clone(),
                suggestion: switch_target(list, prefs, headroom(acc).unwrap_or(left)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cswap::{Usage, Window};

    fn acc(number: u32, active: bool, five: f64, seven: Option<f64>) -> Account {
        Account {
            number,
            email: format!("a{number}@example.com"),
            active,
            usage_status: Some("ok".into()),
            usage: Some(Usage {
                five_hour: Some(Window {
                    pct: five,
                    ..Default::default()
                }),
                seven_day: seven.map(|pct| Window {
                    pct,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn list(accounts: Vec<Account>) -> AccountList {
        AccountList {
            active_account_number: accounts.iter().find(|a| a.active).map(|a| a.number),
            accounts,
            ..Default::default()
        }
    }

    fn prefs() -> Prefs {
        Prefs {
            low_quota_pct: 15,
            ..Default::default()
        }
    }

    #[test]
    fn default_level_bands_stay_at_15_and_40() {
        let p = prefs();
        assert_eq!(level(41.0, &p), Level::Ok);
        assert_eq!(level(40.0, &p), Level::Warn);
        assert_eq!(level(15.5, &p), Level::Warn);
        assert_eq!(level(15.0, &p), Level::Low);
        assert_eq!(level(0.0, &p), Level::Low);
    }

    #[test]
    fn level_bands_follow_the_low_quota_setting() {
        let p = Prefs {
            low_quota_pct: 30,
            ..prefs()
        };
        assert_eq!(level(30.0, &p), Level::Low);
        assert_eq!(level(31.0, &p), Level::Warn);
        assert_eq!(level(55.0, &p), Level::Warn);
        assert_eq!(level(56.0, &p), Level::Ok);
    }

    #[test]
    fn headroom_uses_the_tightest_window() {
        assert_eq!(headroom(&acc(1, true, 20.0, Some(70.0))), Some(30.0));
        assert_eq!(headroom(&acc(1, true, 80.0, None)), Some(20.0));
        let mut broken = acc(1, true, 0.0, None);
        broken.usage_status = Some("rate limited".into());
        assert_eq!(headroom(&broken), None);
    }

    /// A seat billed by usage: only a spend limit, `used` percent of it spent.
    fn spend_only(number: u32, active: bool, used: f64) -> Account {
        Account {
            number,
            email: format!("a{number}@example.com"),
            active,
            usage_status: Some("ok".into()),
            usage: Some(Usage {
                spend: Some(crate::cswap::Spend {
                    used: used / 2.0,
                    limit: 50.0,
                    pct: used,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_spend_limit_counts_only_without_windows() {
        assert_eq!(headroom(&spend_only(1, true, 70.0)), Some(30.0));
        // with a plan, spend is pay-as-you-go past the windows and doesn't lower the room
        let mut plan = acc(1, true, 20.0, None);
        plan.usage.as_mut().unwrap().spend = spend_only(1, true, 95.0).usage.unwrap().spend;
        assert_eq!(headroom(&plan), Some(80.0));
        // and a spend-only account can be suggested and switched to
        let l = list(vec![acc(1, true, 90.0, None), spend_only(2, false, 10.0)]);
        assert_eq!(suggestion(&l, &prefs()).map(|s| s.number), Some(2));
    }

    #[test]
    fn low_and_reset_cover_the_spend_limit() {
        let p = prefs();
        let mut w = Watch::default();
        w.update(&list(vec![spend_only(1, true, 70.0)]), &p);
        let events = w.update(&list(vec![spend_only(1, true, 90.0)]), &p);
        assert!(
            matches!(events.as_slice(), [Event::Low { window: SPEND, .. }]),
            "{events:?}"
        );
        // a new month brings the limit back
        let events = w.update(&list(vec![spend_only(1, true, 0.0)]), &p);
        assert!(
            matches!(events.as_slice(), [Event::Reset { window: SPEND, .. }]),
            "{events:?}"
        );
    }

    #[test]
    fn best_other_skips_active_paused_and_hidden() {
        let mut paused = acc(2, false, 0.0, None);
        paused.disabled = true;
        let l = list(vec![
            acc(1, true, 0.0, None),
            paused,
            acc(3, false, 50.0, None),
            acc(4, false, 10.0, None),
        ]);
        let mut p = prefs();
        assert_eq!(best_other(&l, &p).map(|(a, _)| a.number), Some(4));
        p.hidden_accounts.push(4);
        assert_eq!(best_other(&l, &p).map(|(a, _)| a.number), Some(3));
    }

    #[test]
    fn suggestion_only_when_current_is_low_and_target_is_clearly_better() {
        let p = prefs();
        assert_eq!(
            suggestion(
                &list(vec![acc(1, true, 50.0, None), acc(2, false, 0.0, None)]),
                &p
            ),
            None
        );
        let s = suggestion(
            &list(vec![acc(1, true, 90.0, None), acc(2, false, 20.0, None)]),
            &p,
        )
        .unwrap();
        assert_eq!((s.number, s.headroom, s.current_headroom), (2, 80.0, 10.0));
        // 15% left vs 18% left is not worth a switch
        assert_eq!(
            suggestion(
                &list(vec![acc(1, true, 85.0, None), acc(2, false, 82.0, None)]),
                &p
            ),
            None
        );
    }

    #[test]
    fn first_list_is_only_a_baseline() {
        let mut w = Watch::default();
        let events = w.update(&list(vec![acc(1, true, 95.0, None)]), &prefs());
        assert!(events.is_empty());
    }

    #[test]
    fn low_fires_once_per_crossing() {
        let p = prefs();
        let mut w = Watch::default();
        w.update(
            &list(vec![acc(1, true, 70.0, None), acc(2, false, 0.0, None)]),
            &p,
        );
        let events = w.update(
            &list(vec![acc(1, true, 88.0, None), acc(2, false, 0.0, None)]),
            &p,
        );
        assert_eq!(events.len(), 1);
        let Event::Low {
            window,
            left,
            suggestion,
            ..
        } = &events[0]
        else {
            panic!("expected Low, got {events:?}")
        };
        assert_eq!((*window, *left), ("5h", 12.0));
        assert_eq!(suggestion.as_ref().map(|s| s.number), Some(2));
        assert!(
            w.update(
                &list(vec![acc(1, true, 92.0, None), acc(2, false, 0.0, None)]),
                &p
            )
            .is_empty()
        );
    }

    #[test]
    fn low_ignores_inactive_accounts_and_respects_the_pref() {
        let mut p = prefs();
        let mut w = Watch::default();
        w.update(
            &list(vec![acc(1, true, 0.0, None), acc(2, false, 70.0, None)]),
            &p,
        );
        assert!(
            w.update(
                &list(vec![acc(1, true, 0.0, None), acc(2, false, 90.0, None)]),
                &p
            )
            .is_empty()
        );

        p.notify_low = false;
        let mut w = Watch::default();
        w.update(&list(vec![acc(1, true, 70.0, None)]), &p);
        assert!(
            w.update(&list(vec![acc(1, true, 90.0, None)]), &p)
                .is_empty()
        );
    }

    #[test]
    fn reset_fires_when_a_low_window_recovers() {
        let p = prefs();
        let mut w = Watch::default();
        w.update(
            &list(vec![
                acc(1, true, 0.0, None),
                acc(2, false, 100.0, Some(40.0)),
            ]),
            &p,
        );
        let events = w.update(
            &list(vec![
                acc(1, true, 0.0, None),
                acc(2, false, 0.0, Some(40.0)),
            ]),
            &p,
        );
        assert_eq!(
            events,
            vec![Event::Reset {
                number: 2,
                email: "a2@example.com".into(),
                window: "5h",
                left: 100.0
            }]
        );
    }

    #[test]
    fn a_failed_poll_does_not_count_as_a_reset() {
        let p = prefs();
        let mut w = Watch::default();
        w.update(&list(vec![acc(1, false, 100.0, None)]), &p);
        let mut failed = acc(1, false, 0.0, None);
        failed.usage_status = Some("error".into());
        failed.usage = None;
        assert!(w.update(&list(vec![failed]), &p).is_empty());
        // still at 100% when it comes back: no reset
        assert!(
            w.update(&list(vec![acc(1, false, 100.0, None)]), &p)
                .is_empty()
        );
        // a real reset afterwards is still reported
        assert_eq!(w.update(&list(vec![acc(1, false, 5.0, None)]), &p).len(), 1);
    }

    #[test]
    fn reset_skips_paused_accounts() {
        let p = prefs();
        let mut w = Watch::default();
        let mut a = acc(2, false, 100.0, None);
        a.disabled = true;
        w.update(&list(vec![a.clone()]), &p);
        a.usage.as_mut().unwrap().five_hour.as_mut().unwrap().pct = 0.0;
        assert!(w.update(&list(vec![a]), &p).is_empty());
    }

    fn paced(number: u32, active: bool, used: f64, out: &str) -> Account {
        let mut a = acc(number, active, used, None);
        let w = a.usage.as_mut().unwrap().five_hour.as_mut().unwrap();
        w.will_last_to_reset = Some(false);
        w.projected_exhaustion_at = Some(out.into());
        w.resets_at = Some("2026-10-20T15:00:00+00:00".into());
        w.clock = Some("18:00".into());
        a
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn runs_out_needs_a_future_projection_before_the_reset() {
        let now = at("2026-10-20T12:00:00Z");
        let a = paced(1, true, 40.0, "2026-10-20T13:30:00Z");
        let w = a.usage.as_ref().unwrap().five_hour.clone().unwrap();
        assert_eq!(runs_out(&w, now), Some(at("2026-10-20T13:30:00Z")));
        let mut lasts = w.clone();
        lasts.will_last_to_reset = Some(true);
        assert_eq!(runs_out(&lasts, now), None);
        let mut after_reset = w.clone();
        after_reset.projected_exhaustion_at = Some("2026-10-20T16:00:00Z".into());
        assert_eq!(runs_out(&after_reset, now), None);
        assert_eq!(
            runs_out(&w, at("2026-10-20T14:00:00Z")),
            None,
            "already past"
        );
        let mut empty = w;
        empty.pct = 100.0;
        assert_eq!(runs_out(&empty, now), None);
    }

    #[test]
    fn pace_fires_once_per_window_for_the_current_account() {
        let p = prefs();
        let now = at("2026-10-20T12:00:00Z");
        let mut w = Watch::default();
        let calm = list(vec![acc(1, true, 30.0, None), acc(2, false, 0.0, None)]);
        assert!(w.update_at(&calm, &p, now).is_empty());

        let hot = list(vec![
            paced(1, true, 40.0, "2026-10-20T13:30:00Z"),
            paced(2, false, 40.0, "2026-10-20T13:30:00Z"),
        ]);
        let events = w.update_at(&hot, &p, now);
        assert_eq!(events.len(), 1, "only the current account: {events:?}");
        match &events[0] {
            Event::Pace {
                email,
                window,
                left,
                runs_out,
                resets,
                suggestion,
            } => {
                assert_eq!(
                    (email.as_str(), *window, *left),
                    ("a1@example.com", "5h", 60.0)
                );
                assert_eq!(*runs_out, at("2026-10-20T13:30:00Z"));
                assert_eq!(resets.as_deref(), Some("18:00"));
                assert!(suggestion.is_none(), "account 2 is no better");
            }
            e => panic!("unexpected {e:?}"),
        }
        assert!(
            w.update_at(&hot, &p, now).is_empty(),
            "same window, no repeat"
        );

        let mut next = paced(1, true, 40.0, "2026-10-20T18:30:00Z");
        let nw = next.usage.as_mut().unwrap().five_hour.as_mut().unwrap();
        nw.resets_at = Some("2026-10-20T20:00:00+00:00".into());
        assert_eq!(
            w.update_at(&list(vec![next]), &p, now).len(),
            1,
            "a new window warns again"
        );
    }

    #[test]
    fn pace_is_quiet_at_launch_when_low_or_switched_off() {
        let now = at("2026-10-20T12:00:00Z");
        let hot = list(vec![paced(1, true, 40.0, "2026-10-20T13:30:00Z")]);
        let mut w = Watch::default();
        assert!(
            w.update_at(&hot, &prefs(), now).is_empty(),
            "already pacing at launch"
        );

        let mut w = Watch::default();
        w.update_at(&list(vec![acc(1, true, 0.0, None)]), &prefs(), now);
        let low = list(vec![paced(1, true, 90.0, "2026-10-20T13:30:00Z")]);
        assert!(
            !w.update_at(&low, &prefs(), now)
                .iter()
                .any(|e| matches!(e, Event::Pace { .. })),
            "the low warning covers it"
        );

        let mut w = Watch::default();
        w.update_at(&list(vec![acc(1, true, 0.0, None)]), &prefs(), now);
        let off = Prefs {
            notify_pace: false,
            ..prefs()
        };
        assert!(w.update_at(&hot, &off, now).is_empty());
    }

    fn expiring(number: u32, at: &str) -> Account {
        Account {
            login_expires_at: Some(at.into()),
            ..acc(number, false, 10.0, None)
        }
    }

    fn login_events(events: &[Event]) -> Vec<(u32, i64)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::LoginExpiring { number, left, .. } => Some((*number, left.num_hours())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn warns_once_per_expiring_login() {
        let now = at("2026-10-21T12:00:00Z");
        let mut w = Watch::default();
        let soon = list(vec![
            expiring(1, "2026-10-23T12:00:00Z"),
            expiring(2, "2026-11-30T12:00:00Z"),
            acc(3, true, 10.0, None),
        ]);
        assert_eq!(
            login_events(&w.update_at(&soon, &prefs(), now)),
            [(1, 48)],
            "already close at launch is worth saying; far off is not"
        );
        assert!(login_events(&w.update_at(&soon, &prefs(), now)).is_empty());

        // signing in again moves the date out; a later expiry warns again in its turn
        let renewed = list(vec![expiring(1, "2026-11-20T12:00:00Z")]);
        assert!(login_events(&w.update_at(&renewed, &prefs(), now)).is_empty());
        let later = at("2026-11-19T12:00:00Z");
        assert_eq!(
            login_events(&w.update_at(&renewed, &prefs(), later)),
            [(1, 24)]
        );
    }

    #[test]
    fn login_warnings_follow_the_settings() {
        let now = at("2026-10-21T12:00:00Z");
        let soon = list(vec![expiring(1, "2026-10-22T12:00:00Z")]);
        let off = Prefs {
            notify_login: false,
            ..prefs()
        };
        assert!(login_events(&Watch::default().update_at(&soon, &off, now)).is_empty());
        let hidden = Prefs {
            hidden_accounts: vec![1],
            ..prefs()
        };
        assert!(login_events(&Watch::default().update_at(&soon, &hidden, now)).is_empty());
    }
}
