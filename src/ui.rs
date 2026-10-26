use gpui::{
    App, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, prelude::*, px, relative, rgb,
};

use crate::advice::{self, Suggestion};
use crate::assets::icon;
use crate::cswap::{Account, Window as UsageWindow};
use crate::i18n::l;
use crate::lf;
use crate::settings::{PercentMode, Prefs};
use crate::update::{CswapUpdate, Release};
use crate::usage::{Summary, format_tokens, format_usd};
use crate::{AppState, Globals};

/// System UI font for labels; a monospaced face only where digits must line up.
const UI_FONT: &str = ".AppleSystemUIFont";
const NUM_FONT: &str = "Menlo";

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Colour palette; `t()` picks the light or dark one for the current appearance.
struct T {
    bg: u32,
    hairline: u32,
    hover: u32,
    text: u32,
    secondary: u32,
    tertiary: u32,
    track: u32,
    cache: u32,
    accent: u32,
    ok: u32,
    warn: u32,
    danger: u32,
}

const LIGHT: T = T {
    bg: 0xfafaf9,
    hairline: 0xe8e7e3,
    hover: 0xf1f0ec,
    text: 0x1d1d1f,
    secondary: 0x6e6e73,
    tertiary: 0xa1a1a6,
    track: 0xe9e8e4,
    cache: 0xc9c8c3,
    accent: 0xff4101,
    ok: 0x34a853,
    warn: 0xe0a100,
    danger: 0xd93025,
};

const DARK: T = T {
    bg: 0x1f1f1e,
    hairline: 0x353533,
    hover: 0x2b2b29,
    text: 0xf2f2f0,
    secondary: 0xa1a1a6,
    tertiary: 0x707074,
    track: 0x363634,
    cache: 0x5f5e5a,
    accent: 0xff5a1f,
    ok: 0x3fbf62,
    warn: 0xf0b429,
    danger: 0xff5f52,
};

fn t() -> &'static T {
    if crate::theme::is_dark() {
        &DARK
    } else {
        &LIGHT
    }
}

fn remaining_color(remaining: f64, prefs: &Prefs) -> Hsla {
    match advice::level(remaining, prefs) {
        advice::Level::Low => c(t().danger),
        advice::Level::Warn => c(t().warn),
        advice::Level::Ok => c(t().ok),
    }
}

pub struct Root {
    state: Entity<AppState>,
    was_active: bool,
    _subs: Vec<Subscription>,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.global::<Globals>().state.clone();
        let mut subs = Vec::new();
        subs.push(cx.observe(&state, |_, _, cx| cx.notify()));
        subs.push(cx.observe_window_appearance(window, |_, _, cx| cx.notify()));
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            crate::debug_log(format_args!("popover active={}", window.is_window_active()));
            if window.is_window_active() {
                this.was_active = true;
            } else if this.was_active {
                cx.defer(crate::dismiss_window);
            }
        }));
        cx.on_release(|_, cx| {
            cx.global_mut::<Globals>().window = None;
            cx.global::<Globals>().status.set_highlighted(false);
            crate::statusbar::watch_outside_clicks(false);
        })
        .detach();
        Root {
            state,
            was_active: false,
            _subs: subs,
        }
    }
}

// ---------- primitives ----------

fn hairline() -> impl IntoElement {
    div().h(px(1.0)).w_full().bg(c(t().hairline))
}

fn num(s: impl Into<SharedString>) -> gpui::Div {
    div()
        .font_family(NUM_FONT)
        .whitespace_nowrap()
        .child(s.into())
}

fn section_header(glyph: &str, title: &str, trailing: Option<String>) -> impl IntoElement {
    let mut row = div()
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .pt_3()
        .pb_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(icon(glyph, 12.0, c(t().secondary)))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(t().secondary))
                        .child(title.to_string()),
                ),
        );
    if let Some(text) = trailing {
        row = row.child(
            div()
                .text_size(px(10.5))
                .text_color(c(t().tertiary))
                .child(text),
        );
    }
    row
}

/// Where an even pace would put the bar now, when the window is used faster than that.
fn pace_marker(w: Option<&UsageWindow>, mode: PercentMode) -> Option<f64> {
    let w = w?;
    if w.ahead_of_pace != Some(true) {
        return None;
    }
    Some(mode.value(w.expected_pct?) / 100.0)
}

/// One window's bar. `amount` ("$13.50 / $50.00") goes before the reset time.
fn quota_row(
    label: &str,
    w: Option<&UsageWindow>,
    prefs: &Prefs,
    pace: bool,
    amount: Option<String>,
) -> impl IntoElement {
    let mode = prefs.percent_mode;
    let marker = pace_marker(w, mode).filter(|_| pace);
    let remaining = w.map(|w| (100.0 - w.pct).clamp(0.0, 100.0));
    let shown = w.map(|w| mode.value(w.pct));
    // beside an amount there is room for the countdown only
    let clock = w
        .and_then(|w| w.clock.as_ref())
        .filter(|_| amount.is_none());
    let reset = if prefs.show_reset_times {
        w.and_then(|w| match (&w.countdown, clock) {
            (Some(cd), Some(clock)) => Some(format!(
                "{} · {}",
                crate::i18n::time(cd),
                crate::i18n::time(clock)
            )),
            (Some(cd), None) => Some(crate::i18n::time(cd)),
            _ => None,
        })
    } else {
        None
    };
    let reset = match (amount, reset) {
        (Some(a), Some(r)) => format!("{a} · {r}"),
        (a, r) => a.or(r).unwrap_or_default(),
    };
    let color = remaining
        .map(|r| remaining_color(r, prefs))
        .unwrap_or(c(t().tertiary));
    // The bar always fills with the shown quantity so it reads the same way as the number.
    let fill = shown.unwrap_or(0.0) / 100.0;
    let bar_color = if mode == PercentMode::Used {
        c(t().secondary)
    } else {
        color
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(18.0))
        .child(
            div()
                .w(px(40.0))
                .text_size(px(11.0))
                .text_color(c(t().secondary))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(label.to_string()),
        )
        .child(
            div()
                .relative()
                .w(px(120.0))
                .h(px(3.0))
                .rounded_full()
                .bg(c(t().track))
                .child(
                    div()
                        .h_full()
                        .w(relative(fill as f32))
                        .rounded_full()
                        .bg(bar_color),
                )
                .when_some(marker, |d, at| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(-3.0))
                            .left(relative(at as f32))
                            .ml(px(-1.0))
                            .w(px(2.0))
                            .h(px(9.0))
                            .rounded_full()
                            .bg(c(t().warn)),
                    )
                }),
        )
        .child(
            num(shown
                .map(|r| format!("{r:>3.0}%"))
                .unwrap_or_else(|| "  —".into()))
            .w(px(36.0))
            .text_size(px(11.0))
            .text_color(color),
        )
        .child(
            div()
                .w(px(30.0))
                .text_size(px(10.0))
                .text_color(c(t().tertiary))
                .child(if shown.is_some() { mode.suffix() } else { "" }),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(10.5))
                .text_color(c(t().tertiary))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(reset),
        )
}

// ---------- banners ----------

fn banner(
    id: &'static str,
    glyph: &str,
    title: String,
    detail: String,
    action: &str,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div().px_3().pt_3().child(
        div()
            .flex()
            .items_center()
            .gap_2p5()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(c(t().hover))
            .border_1()
            .border_color(c(t().hairline))
            .child(icon(glyph, 14.0, c(t().accent)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(t().text))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(c(t().secondary))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .id(id)
                    .flex_shrink_0()
                    .px_2p5()
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .bg(c(t().accent))
                    .cursor_pointer()
                    .hover(|d| d.opacity(0.85))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(0xffffff))
                    .child(action.to_string())
                    .on_click(move |_, _, cx| on_click(cx)),
            ),
    )
}

fn suggestion_banner(s: &Suggestion) -> impl IntoElement {
    let number = s.number;
    banner(
        "suggest-switch",
        "swap",
        lf!(
            "Current account has {:.0}% left",
            "Aktif hesapta %{:.0} kaldı",
            s.current_headroom
        ),
        lf!(
            "{} has {:.0}% left",
            "{} hesabında %{:.0} kaldı",
            s.email,
            s.headroom
        ),
        l("Switch", "Geç"),
        move |cx| crate::switch_account(cx, number),
    )
}

fn update_banner(r: &Release) -> impl IntoElement {
    let url = r.url.clone();
    banner(
        "update",
        "rocket",
        lf!(
            "{} {} is available",
            "{} {} çıktı",
            crate::APP_NAME,
            r.version
        ),
        lf!("You have {}", "Sizdeki sürüm {}", crate::APP_VERSION),
        l("Download", "İndir"),
        move |cx| cx.open_url(&url),
    )
}

fn cswap_update_banner(u: &CswapUpdate, upgrading: bool) -> impl IntoElement {
    banner(
        "cswap-update",
        "refresh",
        lf!(
            "claude-swap {} is available",
            "claude-swap {} çıktı",
            u.latest
        ),
        lf!("You have {}", "Sizdeki sürüm {}", u.installed),
        if upgrading {
            l("Updating…", "Güncelleniyor…")
        } else {
            l("Update", "Güncelle")
        },
        crate::upgrade_cswap,
    )
}

// ---------- accounts ----------

fn account_row(
    acc: &Account,
    prefs: &Prefs,
    switching: Option<u32>,
    suggested: bool,
    is_last: bool,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let number = acc.number;
    let active = acc.active;
    let usage = acc.usage.as_ref();
    let five = usage.and_then(|u| u.five_hour.as_ref());
    let seven = usage.and_then(|u| u.seven_day.as_ref());
    let scoped: Vec<&UsageWindow> = usage.map(|u| u.scoped.iter().collect()).unwrap_or_default();
    let spend = usage.and_then(|u| u.spend.as_ref());

    let status: (String, u32) = if switching == Some(number) {
        (l("switching…", "geçiliyor…").into(), t().secondary)
    } else if active {
        (l("current", "aktif").into(), t().accent)
    } else if acc.disabled {
        (l("paused", "duraklatıldı").into(), t().tertiary)
    } else if suggested {
        (l("suggested", "önerilen").into(), t().ok)
    } else {
        (String::new(), t().tertiary)
    };

    let title = div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(7.0))
                .rounded_full()
                .bg(if active {
                    c(t().accent)
                } else {
                    c(t().hairline)
                })
                .flex_shrink_0(),
        )
        .child(
            div()
                .text_size(px(12.5))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(c(t().text))
                .whitespace_nowrap()
                .child(acc.email.clone()),
        )
        .when_some(acc.plan.clone(), |d, plan| {
            d.child(
                div()
                    .flex_shrink_0()
                    .px(px(5.0))
                    .rounded(px(4.0))
                    .bg(c(t().hover))
                    .text_size(px(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(c(t().secondary))
                    .whitespace_nowrap()
                    .child(plan),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.0))
                .text_color(c(t().tertiary))
                .child(acc.org_tag()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(11.0))
                .text_color(c(status.1))
                .whitespace_nowrap()
                .child(status.0),
        );

    // a stale reading's pace is hours old
    let pace = prefs.show_pace && !acc.stale;
    let mut windows: Vec<(&str, Option<&UsageWindow>)> = Vec::new();
    // a seat billed by usage has no 5h window to show as missing
    if five.is_some() || spend.is_none() {
        windows.push(("5h", five));
    }
    if seven.is_some() && prefs.show_seven_day {
        windows.push(("7d", seven));
    }
    if prefs.show_model_limits {
        windows.extend(
            scoped
                .iter()
                .map(|s| (s.name.as_deref().unwrap_or("model"), Some(*s))),
        );
    }
    let mut quotas = div().flex().flex_col().gap_0p5().pl(px(15.0));
    for (label, w) in &windows {
        quotas = quotas.child(quota_row(label, *w, prefs, pace, None));
    }
    if let Some(s) = spend {
        let w = s.window();
        quotas = quotas.child(quota_row(
            s.currency(),
            Some(&w),
            prefs,
            false,
            Some(s.amounts()),
        ));
    }
    if pace {
        let now = chrono::Utc::now();
        for (label, w) in &windows {
            let Some(out) = w.and_then(|w| advice::runs_out(w, now)) else {
                continue;
            };
            let at = crate::i18n::time(&advice::local_clock(out, now));
            let text = match w.and_then(|w| w.clock.as_deref()).map(crate::i18n::time) {
                Some(r) => lf!(
                    "At this pace, {label} runs out around {at}, before its {r} reset",
                    "Bu tempoyla {label}, {r} sıfırlanmasından önce {at} civarında biter"
                ),
                None => lf!(
                    "At this pace, {label} runs out around {at}",
                    "Bu tempoyla {label} {at} civarında biter"
                ),
            };
            quotas = quotas.child(
                div()
                    .text_size(px(10.5))
                    .text_color(c(t().warn))
                    .child(text),
            );
        }
    }
    if let Some(label) = acc.status_label() {
        quotas = quotas.child(
            div()
                .pl(px(0.0))
                .text_size(px(10.5))
                .text_color(c(t().warn))
                .child(match acc.usage_age_seconds.filter(|_| acc.stale) {
                    Some(age) => lf!(
                        "{label} · showing reading from {}",
                        "{label} · {} okuma gösteriliyor",
                        ago(age)
                    ),
                    None => label,
                }),
        );
    }
    if let Some(label) = acc.login_expiry_label(chrono::Utc::now()) {
        quotas = quotas.child(
            div()
                .flex()
                .gap_1()
                .text_size(px(10.5))
                .child(div().text_color(c(t().warn)).child(format!("{label} ·")))
                .child(
                    div()
                        .id(SharedString::from(format!("relogin-{number}")))
                        .text_color(c(t().accent))
                        .cursor_pointer()
                        .hover(|d| d.opacity(0.8))
                        .child(l("Sign in again", "Yeniden giriş yap"))
                        .on_click(move |_, _, cx| {
                            // not a click on the row, which would switch to the account
                            cx.stop_propagation();
                            crate::onboarding::open(cx, crate::onboarding::Entry::Relogin(number));
                        }),
                ),
        );
    }

    div()
        .id(SharedString::from(format!("acct-{number}")))
        .flex()
        .flex_col()
        .gap_1p5()
        .px_4()
        .py_2p5()
        .when(!is_last, |d| d.border_b_1().border_color(c(t().hairline)))
        .when(!active, |d| {
            d.cursor_pointer().hover(|d| d.bg(c(t().hover)))
        })
        .child(title)
        .child(quotas)
        .on_click(cx.listener(move |_, _, _, cx| {
            if !active {
                crate::switch_account(cx, number);
            }
        }))
}

fn ago(seconds: f64) -> String {
    if seconds < 3600.0 {
        lf!("{}m ago", "{} dk önceki", (seconds / 60.0).max(1.0) as u64)
    } else {
        lf!("{}h ago", "{} sa önceki", (seconds / 3600.0) as u64)
    }
}

fn setup_button(label: &'static str, entry: crate::onboarding::Entry) -> impl IntoElement {
    div().px_4().pb_2().child(
        div()
            .id("setup")
            .flex()
            .justify_center()
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(c(t().accent))
            .cursor_pointer()
            .hover(|d| d.opacity(0.88))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c(0xffffff))
            .child(label)
            .on_click(move |_, _, cx| crate::onboarding::open(cx, entry)),
    )
}

fn accounts_section(
    state: &AppState,
    suggested: Option<u32>,
    cx: &mut Context<Root>,
) -> impl IntoElement {
    let fresh = state
        .accounts
        .as_ref()
        .and_then(|l| {
            l.accounts
                .iter()
                .filter_map(|a| a.usage_age_seconds)
                .reduce(f64::min)
        })
        .map(|s| {
            if s < 90.0 {
                l("updated just now", "az önce güncellendi").to_string()
            } else {
                lf!("updated {}", "{} güncellendi", ago(s))
            }
        });
    let prefs = &state.prefs;
    let title = match prefs.percent_mode {
        PercentMode::Remaining => l("Accounts · quota remaining", "Hesaplar · kalan kota"),
        PercentMode::Used => l("Accounts · quota used", "Hesaplar · kullanılan kota"),
    };
    let mut col = div()
        .flex()
        .flex_col()
        .child(section_header("users", title, fresh));
    match (&state.accounts, &state.accounts_error) {
        (Some(list), _) => {
            let visible: Vec<&Account> = list
                .accounts
                .iter()
                .filter(|a| !prefs.is_hidden(a.number))
                .collect();
            let n = visible.len();
            if n == 0 {
                col = col.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_size(px(12.0))
                        .text_color(c(t().secondary))
                        .child(if list.accounts.is_empty() {
                            l("No accounts yet.", "Henüz hesap yok.")
                        } else {
                            l(
                                "All accounts are hidden. Show them in Settings › Accounts.",
                                "Tüm hesaplar gizli. Ayarlar › Hesaplar'dan gösterebilirsin.",
                            )
                        }),
                );
                if list.accounts.is_empty() {
                    col = col.child(setup_button(
                        l("Add an account", "Hesap ekle"),
                        crate::onboarding::Entry::AddAccount,
                    ));
                }
            }
            for (i, acc) in visible.iter().enumerate() {
                col = col.child(account_row(
                    acc,
                    prefs,
                    state.switching,
                    suggested == Some(acc.number),
                    i + 1 == n,
                    cx,
                ));
            }
        }
        (None, Some(err)) => {
            col = col.child(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(11.5))
                    .text_color(c(t().danger))
                    .child(err.clone()),
            );
            if crate::setup::find(crate::setup::Tool::Cswap).is_none() {
                col = col.child(setup_button(
                    l("Set up Swapbar", "Swapbar'ı kur"),
                    crate::onboarding::Entry::Welcome,
                ));
            }
        }
        (None, None) => {
            col = col.child(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(12.0))
                    .text_color(c(t().tertiary))
                    .child(l("Loading…", "Yükleniyor…")),
            )
        }
    }
    if let (Some(_), Some(err)) = (&state.accounts, &state.accounts_error) {
        col = col.child(
            div()
                .px_4()
                .py_2()
                .text_size(px(11.0))
                .text_color(c(t().danger))
                .child(err.clone()),
        );
    }
    col
}

// ---------- usage ----------

const COL_W: f32 = 92.0;

fn usage_line(
    label: impl Into<SharedString>,
    today: String,
    month: String,
    bold: bool,
    muted: bool,
) -> impl IntoElement {
    let color = if muted { t().secondary } else { t().text };
    let weight = if bold {
        FontWeight::SEMIBOLD
    } else {
        FontWeight::NORMAL
    };
    div()
        .flex()
        .items_center()
        .h(px(20.0))
        .px_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(11.5))
                .text_color(c(color))
                .child(label.into()),
        )
        .child(
            num(today)
                .w(px(COL_W))
                .flex()
                .justify_end()
                .text_size(px(11.5))
                .font_weight(weight)
                .text_color(c(color)),
        )
        .child(
            num(month)
                .w(px(COL_W))
                .flex()
                .justify_end()
                .text_size(px(11.5))
                .font_weight(weight)
                .text_color(c(color)),
        )
}

fn stacked_bar(summary: &Summary) -> impl IntoElement {
    let tot = &summary.total;
    let total = tot.tokens() as f64;
    let segs = [
        (tot.input, t().text),
        (tot.cache_read, t().cache),
        (tot.cache_create, t().warn),
        (tot.output, t().accent),
    ];
    let mut bar = div()
        .flex()
        .h(px(4.0))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(c(t().track));
    if total > 0.0 {
        for (v, color) in segs {
            if v > 0 {
                bar = bar.child(
                    div()
                        .h_full()
                        .w(relative((v as f64 / total) as f32))
                        .bg(c(color)),
                );
            }
        }
    }
    let pct = |v: u64| {
        if total > 0.0 {
            v as f64 * 100.0 / total
        } else {
            0.0
        }
    };
    let legend = div()
        .flex()
        .gap_3()
        .text_size(px(10.0))
        .text_color(c(t().tertiary))
        .child(lf!("input {:.0}%", "girdi %{:.0}", pct(tot.input)))
        .child(lf!(
            "cache read {:.0}%",
            "önbellek okuma %{:.0}",
            pct(tot.cache_read)
        ))
        .child(lf!(
            "cache write {:.0}%",
            "önbellek yazma %{:.0}",
            pct(tot.cache_create)
        ))
        .child(lf!("output {:.0}%", "çıktı %{:.0}", pct(tot.output)));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_4()
        .pt_1()
        .pb_2()
        .child(bar)
        .child(legend)
}

fn usage_section(state: &AppState) -> impl IntoElement {
    let status = if state.usage_scanning {
        Some(l("scanning transcripts…", "transkriptler taranıyor…").to_string())
    } else if state.usage_scanned_once {
        Some(l("estimated at API prices", "API fiyatlarıyla tahmini").to_string())
    } else {
        None
    };
    let mut col =
        div()
            .flex()
            .flex_col()
            .child(section_header("coins", l("Usage", "Kullanım"), status));

    let (Some(today), Some(month)) = (&state.summary_today, &state.summary_month) else {
        return col.child(
            div()
                .px_4()
                .py_2()
                .text_size(px(12.0))
                .text_color(c(t().tertiary))
                .child(l(
                    "Scanning Claude Code transcripts…",
                    "Claude Code transkriptleri taranıyor…",
                )),
        );
    };

    // column headers
    col = col.child(
        div()
            .flex()
            .items_center()
            .h(px(18.0))
            .px_4()
            .child(div().flex_1())
            .child(
                num(l("today", "bugün"))
                    .w(px(COL_W))
                    .flex()
                    .justify_end()
                    .text_size(px(10.5))
                    .text_color(c(t().tertiary)),
            )
            .child(
                num(l("30 days", "30 gün"))
                    .w(px(COL_W))
                    .flex()
                    .justify_end()
                    .text_size(px(10.5))
                    .text_color(c(t().tertiary)),
            ),
    );
    col = col
        .child(usage_line(
            l("Cost", "Maliyet"),
            format_usd(today.total.cost),
            format_usd(month.total.cost),
            true,
            false,
        ))
        .child(usage_line(
            l("Tokens", "Token"),
            format_tokens(today.total.tokens()),
            format_tokens(month.total.tokens()),
            false,
            false,
        ))
        .child(usage_line(
            l("Requests", "İstek"),
            group(today.total.requests),
            group(month.total.requests),
            false,
            false,
        ))
        .child(usage_line(
            l("Output tokens", "Çıktı token"),
            format_tokens(today.total.output),
            format_tokens(month.total.output),
            false,
            true,
        ))
        .child(usage_line(
            l("Cache read", "Önbellek okuma"),
            format_tokens(today.total.cache_read),
            format_tokens(month.total.cache_read),
            false,
            true,
        ));

    col = col.child(div().px_4().py_1p5().child(hairline()));

    // per-account costs, ordered by 30-day cost; only worth a block with two or more
    if month.by_account.len() > 1 {
        for (owner, tot) in &month.by_account {
            let today_cost = today
                .by_account
                .iter()
                .find(|(o, _)| o == owner)
                .map(|(_, t)| t.cost)
                .unwrap_or(0.0);
            col = col.child(usage_line(
                owner.label().to_string(),
                if today_cost > 0.0 {
                    format_usd(today_cost)
                } else {
                    "–".into()
                },
                format_usd(tot.cost),
                false,
                false,
            ));
        }
        col = col.child(div().px_4().py_1p5().child(hairline()));
    }

    // per-model costs, ordered by 30-day cost
    for (name, tot) in month.by_model.iter().take(6) {
        let short = name.strip_prefix("claude-").unwrap_or(name).to_string();
        let today_cost = today
            .by_model
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.cost)
            .unwrap_or(0.0);
        let unpriced = tot.unpriced >= tot.requests && tot.cost == 0.0;
        col = col.child(usage_line(
            short,
            if today_cost > 0.0 {
                format_usd(today_cost)
            } else {
                "–".into()
            },
            if unpriced {
                l("unpriced", "fiyatsız").into()
            } else {
                format_usd(tot.cost)
            },
            false,
            false,
        ));
    }

    col.child(stacked_bar(month))
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

// ---------- footer ----------

fn footer_item(
    id: &'static str,
    glyph: &str,
    label: &str,
    danger: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(11.5))
        .text_color(c(t().secondary))
        .hover(move |d| {
            d.bg(c(t().hover))
                .text_color(c(if danger { t().danger } else { t().text }))
        })
        .child(icon(glyph, 13.0, c(t().secondary)))
        .child(label.to_string())
        .on_click(move |_, w, cx| on_click(w, cx))
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync(cx);
        let s = self.state.read(cx).clone();
        let busy = s.accounts_loading || s.usage_scanning || s.switching.is_some();

        let show_usage = s.prefs.show_usage;
        let suggestion = s
            .accounts
            .as_ref()
            .and_then(|l| advice::suggestion(l, &s.prefs))
            .filter(|_| s.switching.is_none());
        let body = div()
            .id("body")
            .flex_1()
            .overflow_y_scroll()
            .when_some(s.update.as_ref(), |d, r| d.child(update_banner(r)))
            .when_some(s.cswap_update.as_ref(), |d, u| {
                d.child(cswap_update_banner(u, s.cswap_upgrading))
            })
            .when_some(suggestion.as_ref(), |d, sg| d.child(suggestion_banner(sg)))
            .child(accounts_section(
                &s,
                suggestion.as_ref().map(|sg| sg.number),
                cx,
            ))
            .when(show_usage, |d| {
                d.child(div().px_4().py_1().child(hairline()))
                    .child(usage_section(&s))
            });

        let footer = div()
            .flex()
            .items_center()
            .px_2()
            .py_1p5()
            .border_t_1()
            .border_color(c(t().hairline))
            .child(footer_item(
                "refresh",
                "refresh",
                if busy {
                    l("Refreshing…", "Yenileniyor…")
                } else {
                    l("Refresh", "Yenile")
                },
                false,
                |_, cx| {
                    crate::refresh_accounts(cx);
                    crate::refresh_usage(cx);
                },
            ))
            .child(div().flex_1())
            .child(footer_item(
                "settings",
                "settings-small",
                l("Settings…", "Ayarlar…"),
                false,
                |_, cx| crate::open_settings(cx),
            ))
            .child(footer_item(
                "quit",
                "power",
                l("Quit", "Çık"),
                true,
                |_, cx| cx.quit(),
            ));

        div()
            .flex()
            .flex_col()
            .size_full()
            .rounded_lg()
            .border_1()
            .border_color(c(t().hairline))
            .bg(c(t().bg))
            .font_family(UI_FONT)
            .text_size(px(12.0))
            .text_color(c(t().text))
            .child(body)
            .child(footer)
    }
}
