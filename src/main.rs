// A tray app: no console window next to it (debug builds keep one for the logs).
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod advice;
mod assets;
mod attribution;
mod cswap;
// The platform modules share one API; the Windows versions live in `win/`.
#[cfg_attr(target_os = "windows", path = "win/dialog.rs")]
mod dialog;
#[cfg_attr(target_os = "windows", path = "win/hotkey.rs")]
mod hotkey;
mod i18n;
#[cfg_attr(target_os = "windows", path = "win/notify.rs")]
mod notify;
mod onboarding;
mod pricing;
mod settings;
mod settings_ui;
mod setup;
#[cfg_attr(target_os = "windows", path = "win/statusbar.rs")]
mod statusbar;
#[cfg_attr(target_os = "windows", path = "win/theme.rs")]
mod theme;
mod ui;
mod update;
mod usage;
#[cfg(target_os = "windows")]
mod win;

use gpui::{
    App, AppContext, Application, AsyncApp, Bounds, Entity, Global, Pixels, Point, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, point, px, size,
};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use i18n::l;
use pricing::PriceBook;
use settings::{Prefs, TitleMode};
use usage::{Period, Rec, Summary};

pub const WINDOW_WIDTH: f32 = 420.0;
pub const WINDOW_HEIGHT: f32 = 780.0;
pub const APP_NAME: &str = "Swapbar";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct AppState {
    pub accounts: Option<cswap::AccountList>,
    pub accounts_error: Option<String>,
    pub accounts_loading: bool,
    pub switching: Option<u32>,
    pub records: Arc<Vec<Rec>>,
    pub prices: Arc<PriceBook>,
    pub summary_today: Option<Summary>,
    pub summary_month: Option<Summary>,
    pub usage_scanning: bool,
    pub usage_scanned_once: bool,
    pub prefs: Prefs,
    pub cswap_config: Option<cswap::ConfigList>,
    pub config_error: Option<String>,
    pub config_busy: bool,
    /// A newer release, once an update check has found one.
    pub update: Option<update::Release>,
    pub update_checking: bool,
    /// Result of the last check the user started from Settings.
    pub update_message: Option<String>,
    /// A newer claude-swap on PyPI, once an update check has found one.
    pub cswap_update: Option<update::CswapUpdate>,
    pub cswap_upgrading: bool,
}

impl AppState {
    fn new() -> Self {
        AppState {
            accounts: None,
            accounts_error: None,
            accounts_loading: false,
            switching: None,
            records: Arc::new(Vec::new()),
            prices: Arc::new(PriceBook::default()),
            summary_today: None,
            summary_month: None,
            usage_scanning: false,
            usage_scanned_once: false,
            prefs: Prefs::load(),
            cswap_config: None,
            config_error: None,
            config_busy: false,
            update: None,
            update_checking: false,
            update_message: None,
            cswap_update: None,
            cswap_upgrading: false,
        }
    }

    pub fn recompute_summary(&mut self) {
        self.summary_today = Some(usage::summarize(&self.records, Period::Today, &self.prices));
        self.summary_month = Some(usage::summarize(&self.records, Period::Month, &self.prices));
    }

    pub fn active_account(&self) -> Option<&cswap::Account> {
        self.accounts.as_ref()?.accounts.iter().find(|a| a.active)
    }
}

pub struct Globals {
    pub state: Entity<AppState>,
    pub status: Rc<statusbar::StatusItem>,
    pub window: Option<WindowHandle<ui::Root>>,
    pub settings_window: Option<WindowHandle<settings_ui::SettingsRoot>>,
    pub setup_window: Option<WindowHandle<onboarding::SetupRoot>>,
    /// Whether the first account list has been checked for a missing setup.
    pub setup_checked: bool,
    /// Last time the status item toggled the popover (debounces repeated clicks).
    pub last_toggle: Option<Instant>,
    /// Last time the popover closed by itself (outside click or losing focus).
    pub last_dismiss: Option<Instant>,
    /// Quota readings from the previous poll, for notifications.
    pub watch: advice::Watch,
}

/// `SWAPBAR_DEBUG` log line prefixed with milliseconds since launch, for click/focus timing.
pub fn debug_log(msg: std::fmt::Arguments) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    if std::env::var("SWAPBAR_DEBUG").is_ok() {
        let ms = START.get_or_init(Instant::now).elapsed().as_millis();
        eprintln!("[{ms:>7} ms] {msg}");
    }
}

/// Settings and Setup draw under a transparent title bar beside macOS's traffic lights.
/// Windows would hide its title bar, buttons and all, so it keeps the native one.
pub const TRANSPARENT_TITLEBAR: bool = cfg!(target_os = "macos");

/// Clicks on the status item can make the popover lose focus a moment before the
/// click itself is handled; within this window a toggle must not reopen it.
const TOGGLE_GRACE: Duration = Duration::from_millis(300);

impl Global for Globals {}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--scan") {
        #[cfg(target_os = "windows")]
        win::attach_console();
        let started = std::time::Instant::now();
        let prices = PriceBook::load_with_overlay();
        let recs = usage::collect_records();
        eprintln!("{} records in {:?}", recs.len(), started.elapsed());
        for p in [Period::Today, Period::Week, Period::Month] {
            let s = usage::summarize(&recs, p, &prices);
            println!(
                "{:>7}: {} | {} tokens | {} req | {} unpriced",
                p.label(),
                usage::format_usd(s.total.cost),
                usage::format_tokens(s.total.tokens()),
                s.total.requests,
                s.total.unpriced
            );
            for (o, t) in &s.by_account {
                println!(
                    "         {:<28} {:>10} {:>8} {:>6}",
                    o.label(),
                    usage::format_usd(t.cost),
                    usage::format_tokens(t.tokens()),
                    t.requests
                );
            }
            for (m, t) in &s.by_model {
                println!(
                    "         {m:<28} {:>10} {:>8} {:>6}",
                    usage::format_usd(t.cost),
                    usage::format_tokens(t.tokens()),
                    t.requests
                );
            }
        }
        return;
    }
    #[cfg(target_os = "windows")]
    if !win::claim_single_instance() {
        return;
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            #[cfg(target_os = "windows")]
            win::keep_alive(cx);
            let state = cx.new(|_| AppState::new());
            theme::apply(state.read(cx).prefs.theme);
            i18n::set(state.read(cx).prefs.language);

            // AppKit calls these from inside its own event handling, sometimes while gpui
            // is mid-update. Run them on gpui's next main-thread tick instead of inline,
            // so they neither re-enter the app nor get dropped.
            let status = Rc::new(statusbar::StatusItem::new(
                " …",
                Box::new({
                    let cx = cx.to_async();
                    move || {
                        let app = cx.clone();
                        cx.foreground_executor()
                            .spawn(async move {
                                let _ = app.update(toggle_window);
                            })
                            .detach();
                    }
                }),
            ));
            statusbar::set_outside_click_handler(Box::new({
                let cx = cx.to_async();
                move || {
                    debug_log(format_args!("outside click"));
                    let app = cx.clone();
                    cx.foreground_executor()
                        .spawn(async move {
                            let _ = app.update(dismiss_window);
                        })
                        .detach();
                }
            }));
            #[cfg(target_os = "windows")]
            statusbar::set_menu_handler(Box::new({
                let cx = cx.to_async();
                move |action| {
                    let app = cx.clone();
                    cx.foreground_executor()
                        .spawn(async move {
                            let _ = app.update(|cx| match action {
                                statusbar::MenuAction::Open => open_popover(cx),
                                statusbar::MenuAction::Settings => open_settings(cx),
                                statusbar::MenuAction::Quit => cx.quit(),
                            });
                        })
                        .detach();
                }
            }));

            cx.set_global(Globals {
                state: state.clone(),
                status,
                window: None,
                settings_window: None,
                setup_window: None,
                setup_checked: false,
                last_toggle: None,
                last_dismiss: None,
                watch: advice::Watch::default(),
            });

            notify::init(Box::new({
                let cx = cx.to_async();
                move |action| {
                    let app = cx.clone();
                    cx.foreground_executor()
                        .spawn(async move {
                            let _ = app.update(|cx| match action {
                                notify::Action::Open => open_popover(cx),
                                notify::Action::Switch(n) => switch_account(cx, n),
                            });
                        })
                        .detach();
                }
            }));
            let prefs = state.read(cx).prefs.clone();
            if prefs.notify_low || prefs.notify_pace || prefs.notify_reset || prefs.notify_login {
                notify::request_authorization();
            }
            hotkey::set_handler(Box::new({
                let cx = cx.to_async();
                move |key| {
                    let app = cx.clone();
                    cx.foreground_executor()
                        .spawn(async move {
                            let _ = app.update(|cx| match key {
                                hotkey::Hotkey::Toggle => toggle_window(cx),
                                hotkey::Hotkey::SwitchBest => switch_to_best(cx),
                            });
                        })
                        .detach();
                }
            }));
            hotkey::set_enabled(prefs.hotkeys);

            refresh_accounts(cx);
            refresh_usage(cx);

            if std::env::var("SWAPBAR_SETTINGS").is_ok() {
                open_settings(cx);
            }
            match std::env::var("SWAPBAR_SETUP").as_deref() {
                Ok("add") => onboarding::open(cx, onboarding::Entry::AddAccount),
                Ok(n) if n.parse::<u32>().is_ok() => {
                    onboarding::open(cx, onboarding::Entry::Relogin(n.parse().unwrap()))
                }
                Ok(_) => onboarding::open(cx, onboarding::Entry::Welcome),
                Err(_) => {}
            }
            if std::env::var("SWAPBAR_OPEN").is_ok() {
                // give the status item a layout pass before measuring it
                cx.spawn(async move |cx: &mut AsyncApp| {
                    cx.background_executor()
                        .timer(Duration::from_millis(400))
                        .await;
                    let _ = cx.update(open_popover);
                })
                .detach();
            }

            // periodic refreshes (intervals re-read from prefs each tick)
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    let secs = cx
                        .update(|cx| {
                            cx.global::<Globals>()
                                .state
                                .read(cx)
                                .prefs
                                .accounts_refresh_secs
                        })
                        .unwrap_or(60);
                    cx.background_executor()
                        .timer(Duration::from_secs(secs.max(15)))
                        .await;
                    let _ = cx.update(refresh_accounts);
                }
            })
            .detach();
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    let secs = cx
                        .update(|cx| {
                            cx.global::<Globals>()
                                .state
                                .read(cx)
                                .prefs
                                .usage_refresh_secs
                        })
                        .unwrap_or(120);
                    cx.background_executor()
                        .timer(Duration::from_secs(secs.max(30)))
                        .await;
                    let _ = cx.update(refresh_usage);
                }
            })
            .detach();
            cx.spawn(async move |cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
                loop {
                    let _ = cx.update(|cx| {
                        if cx.global::<Globals>().state.read(cx).prefs.check_updates {
                            check_for_updates(cx, false);
                        }
                    });
                    cx.background_executor().timer(UPDATE_CHECK_INTERVAL).await;
                }
            })
            .detach();
        });
    // macOS never gets here; Windows returns from the message loop on quit
    #[cfg(target_os = "windows")]
    statusbar::remove();
}

pub fn refresh_accounts(cx: &mut App) {
    let state = cx.global::<Globals>().state.clone();
    let already = state.read(cx).accounts_loading;
    if already {
        return;
    }
    state.update(cx, |s, cx| {
        s.accounts_loading = true;
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            .spawn(async { cswap::list() })
            .await;
        let _ = state.update(cx, |s, cx| {
            s.accounts_loading = false;
            match result {
                Ok(list) => {
                    s.accounts = Some(list);
                    s.accounts_error = None;
                }
                Err(e) => s.accounts_error = Some(e),
            }
            cx.notify();
        });
        let _ = cx.update(|cx| {
            update_title(cx);
            notify_quota_changes(cx);
            offer_setup_once(cx);
        });
    })
    .detach();
}

/// After the first account list: open the setup window when cswap is missing or has
/// no accounts, since the menu would have nothing to show.
fn offer_setup_once(cx: &mut App) {
    let g = cx.global_mut::<Globals>();
    if g.setup_checked {
        return;
    }
    g.setup_checked = true;
    let state = g.state.clone();
    let state = state.read(cx);
    let missing = match &state.accounts {
        Some(list) => list.accounts.is_empty(),
        None => setup::find(setup::Tool::Cswap).is_none(),
    };
    if missing && cx.global::<Globals>().setup_window.is_none() {
        onboarding::open(cx, onboarding::Entry::Welcome);
    }
}

/// Run an account command in the background, then reload the list; failures show in
/// the accounts error line.
fn account_command(cx: &mut App, f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    let state = cx.global::<Globals>().state.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx.background_executor().spawn(async move { f() }).await;
        let _ = state.update(cx, |s, cx| {
            s.accounts_error = result.err();
            cx.notify();
        });
        let _ = cx.update(refresh_accounts);
    })
    .detach();
}

pub fn remove_account(cx: &mut App, number: u32) {
    let Some(acc) = account(cx, number) else {
        return;
    };
    let alert = dialog::Alert::confirm(
        lf!("Remove {}?", "{} kaldırılsın mı?", acc.email),
        lf!(
            "cswap deletes the login it saved for account {number}. Claude Code itself stays signed in to whatever account it uses now. You can add the account again later.",
            "cswap, {number} numaralı hesap için sakladığı girişi siler. Claude Code şu an hangi hesaptaysa onda kalır. Hesabı daha sonra yeniden ekleyebilirsin."
        ),
        l("Remove", "Kaldır"),
    )
    .destructive();
    dialog::ask(cx, alert, move |answer, cx| {
        if answer.is_some() {
            account_command(cx, move || cswap::remove(number));
        }
    });
}

pub fn rename_account(cx: &mut App, number: u32) {
    let Some(acc) = account(cx, number) else {
        return;
    };
    let alert = dialog::Alert::confirm(
        lf!("Name for {}", "{} için ad", acc.email),
        l(
            "A short alias shows in place of the email. cswap also accepts it in commands, e.g. `cswap switch work`. Leave empty to remove it.",
            "Kısa bir takma ad e-postanın yerine görünür. cswap komutlarda da kabul eder, örneğin `cswap switch is`. Kaldırmak için boş bırak.",
        ),
        l("Save", "Kaydet"),
    )
    .with_input(acc.alias.clone().unwrap_or_default(), l("work, personal…", "iş, kişisel…"));
    dialog::ask(cx, alert, move |answer, cx| {
        let Some(alias) = answer else { return };
        let alias = alias.trim().to_string();
        account_command(cx, move || {
            cswap::set_alias(number, (!alias.is_empty()).then_some(alias.as_str()))
        });
    });
}

/// Swap an account with its neighbour in slot order (`up` = towards slot 1).
pub fn move_account(cx: &mut App, number: u32, up: bool) {
    let state = cx.global::<Globals>().state.read(cx);
    let Some(list) = &state.accounts else { return };
    let mut slots: Vec<u32> = list.accounts.iter().map(|a| a.number).collect();
    slots.sort_unstable();
    let Some(i) = slots.iter().position(|&n| n == number) else {
        return;
    };
    let other = if up {
        i.checked_sub(1).map(|j| slots[j])
    } else {
        slots.get(i + 1).copied()
    };
    if let Some(other) = other {
        account_command(cx, move || cswap::swap_slots(number, other));
    }
}

fn account(cx: &App, number: u32) -> Option<cswap::Account> {
    let state = cx.global::<Globals>().state.read(cx);
    state
        .accounts
        .as_ref()?
        .accounts
        .iter()
        .find(|a| a.number == number)
        .cloned()
}

/// Post notifications for quota changes since the previous poll.
fn notify_quota_changes(cx: &mut App) {
    let state = cx.global::<Globals>().state.read(cx);
    let Some(list) = state.accounts.clone() else {
        return;
    };
    let prefs = state.prefs.clone();
    let events = cx.global_mut::<Globals>().watch.update(&list, &prefs);
    for e in events {
        match e {
            advice::Event::Low {
                email,
                window,
                left,
                suggestion,
            } => {
                let title = lf!(
                    "{window} quota low: {left:.0}% left",
                    "{window} kotası azaldı: %{left:.0} kaldı"
                );
                let (body, target) = match suggestion {
                    Some(s) => (
                        lf!(
                            "{email} is running out. {} has {:.0}% left.",
                            "{email} tükenmek üzere. {} hesabında %{:.0} kaldı.",
                            s.email,
                            s.headroom
                        ),
                        Some(s.number),
                    ),
                    None => (
                        lf!("{email} is running out.", "{email} tükenmek üzere."),
                        None,
                    ),
                };
                notify::post(&format!("low-{window}"), &title, &body, target);
            }
            advice::Event::Pace {
                email,
                window,
                left,
                runs_out,
                resets,
                suggestion,
            } => {
                let at = i18n::time(&advice::local_clock(runs_out, chrono::Utc::now()));
                let title = lf!(
                    "{window} quota won't last: runs out around {at}",
                    "{window} kotası yetmeyecek: {at} civarında biter"
                );
                let mut body = match resets.as_deref().map(i18n::time) {
                    Some(r) => lf!(
                        "{email} has {left:.0}% left, but at this pace it runs out before its {r} reset.",
                        "{email} hesabında %{left:.0} kaldı, ama bu tempoyla {r} sıfırlanmasından önce biter."
                    ),
                    None => lf!(
                        "{email} has {left:.0}% left, but at this pace it runs out.",
                        "{email} hesabında %{left:.0} kaldı, ama bu tempoyla biter."
                    ),
                };
                if let Some(s) = &suggestion {
                    body.push_str(&lf!(
                        " {} has {:.0}% left.",
                        " {} hesabında %{:.0} kaldı.",
                        s.email,
                        s.headroom
                    ));
                }
                notify::post(
                    &format!("pace-{window}"),
                    &title,
                    &body,
                    suggestion.map(|s| s.number),
                );
            }
            advice::Event::Reset {
                number,
                email,
                window,
                left,
            } => {
                let active = list.active_account_number == Some(number);
                notify::post(
                    &format!("reset-{number}-{window}"),
                    l("Quota available again", "Kota yeniden kullanılabilir"),
                    &lf!(
                        "{email}: the {window} window reset, {left:.0}% left.",
                        "{email}: {window} penceresi sıfırlandı, %{left:.0} kaldı."
                    ),
                    (!active).then_some(number),
                );
            }
            advice::Event::LoginExpiring {
                number,
                email,
                left,
            } => {
                let (title, body) = if left.is_zero() {
                    (
                        lf!("{email} is signed out", "{email} oturumu kapandı"),
                        l(
                            "Its saved login has expired. Open Swapbar › Settings › Accounts and choose Sign in again.",
                            "Kayıtlı girişinin süresi doldu. Swapbar › Ayarlar › Hesaplar'da Yeniden giriş yap'ı seç.",
                        )
                        .to_string(),
                    )
                } else {
                    let when = cswap::expires_in_text(left);
                    (
                        lf!(
                            "Sign in to {email} again soon",
                            "{email} için yakında yeniden giriş gerekecek"
                        ),
                        lf!(
                            "Its saved login expires {when}. Sign in again from the popover or Settings › Accounts so it keeps working.",
                            "Kayıtlı girişi {when} sona eriyor. Hesap çalışmaya devam etsin diye popover'dan ya da Ayarlar › Hesaplar'dan yeniden giriş yap."
                        ),
                    )
                };
                notify::post(&format!("login-{number}"), &title, &body, None);
            }
        }
    }
}

/// Switch to the account with the most quota left (the global shortcut).
pub fn switch_to_best(cx: &mut App) {
    let state = cx.global::<Globals>().state.read(cx);
    let best = state
        .accounts
        .as_ref()
        .and_then(|l| advice::best_other(l, &state.prefs))
        .map(|(a, _)| a.number);
    match best {
        Some(number) => switch_account(cx, number),
        // nothing to switch to: show the accounts instead
        None => open_popover(cx),
    }
}

const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Look for a newer release. `manual` checks report their outcome in Settings.
pub fn check_for_updates(cx: &mut App, manual: bool) {
    let state = cx.global::<Globals>().state.clone();
    if state.read(cx).update_checking {
        return;
    }
    state.update(cx, |s, cx| {
        s.update_checking = true;
        if manual {
            s.update_message = None;
        }
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let (result, cswap_result) = cx
            .background_executor()
            .spawn(async { (update::check(APP_VERSION), check_cswap()) })
            .await;
        let _ = state.update(cx, |s, cx| {
            s.update_checking = false;
            match cswap_result {
                Ok(found) => s.cswap_update = found,
                Err(e) => debug_log(format_args!("claude-swap update check failed: {e}")),
            }
            match result {
                Ok(found) => {
                    if manual {
                        s.update_message = Some(match &found {
                            Some(r) => {
                                lf!("Version {} is available.", "{} sürümü çıktı.", r.version)
                            }
                            None => lf!(
                                "{APP_NAME} {APP_VERSION} is the latest version.",
                                "{APP_NAME} {APP_VERSION} en güncel sürüm."
                            ),
                        });
                    }
                    s.update = found;
                }
                Err(e) => {
                    debug_log(format_args!("update check failed: {e}"));
                    if manual {
                        s.update_message = Some(lf!(
                            "Could not check for updates: {e}",
                            "Güncellemeler kontrol edilemedi: {e}"
                        ));
                    }
                }
            }
            cx.notify();
        });
    })
    .detach();
}

/// A newer claude-swap than the installed one. Nothing to offer without cswap.
fn check_cswap() -> Result<Option<update::CswapUpdate>, String> {
    let Some(installed) = setup::find(setup::Tool::Cswap).and_then(|p| setup::cswap_version(&p))
    else {
        return Ok(None);
    };
    update::check_cswap(&installed)
}

/// Run `cswap upgrade`, then reload the account list with the new cswap.
pub fn upgrade_cswap(cx: &mut App) {
    let state = cx.global::<Globals>().state.clone();
    if state.read(cx).cswap_upgrading {
        return;
    }
    state.update(cx, |s, cx| {
        s.cswap_upgrading = true;
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            // the upgrade worked even if PyPI can't be asked again right now
            .spawn(async { cswap::upgrade().map(|()| check_cswap().ok().flatten()) })
            .await;
        let ok = result.is_ok();
        let _ = state.update(cx, |s, cx| {
            s.cswap_upgrading = false;
            match result {
                Ok(still_newer) => s.cswap_update = still_newer,
                Err(e) => {
                    s.accounts_error = Some(lf!(
                        "Could not update claude-swap: {e}",
                        "claude-swap güncellenemedi: {e}"
                    ))
                }
            }
            cx.notify();
        });
        // a reload would clear the error before anyone read it
        if ok {
            let _ = cx.update(refresh_accounts);
        }
    })
    .detach();
}

pub fn switch_account(cx: &mut App, number: u32) {
    let state = cx.global::<Globals>().state.clone();
    if state.read(cx).switching.is_some() {
        return;
    }
    state.update(cx, |s, cx| {
        s.switching = Some(number);
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            .spawn(async move { cswap::switch(number) })
            .await;
        let ok = result.is_ok();
        let _ = state.update(cx, |s, cx| {
            s.switching = None;
            if let Err(e) = result {
                s.accounts_error = Some(e);
            }
            cx.notify();
        });
        let _ = cx.update(|cx| {
            // a switch from a shortcut or a notification has no popover to show it
            if ok && cx.global::<Globals>().window.is_none() {
                let target = state
                    .read(cx)
                    .accounts
                    .as_ref()
                    .and_then(|l| l.accounts.iter().find(|a| a.number == number))
                    .map(|a| (a.email.clone(), advice::headroom(a)));
                if let Some((email, left)) = target {
                    let body = match left {
                        Some(h) => lf!("{h:.0}% quota left.", "%{h:.0} kota kaldı."),
                        None => l("Quota not known yet.", "Kota henüz bilinmiyor.").to_string(),
                    };
                    notify::post(
                        "switched",
                        &lf!("Switched to {email}", "{email} hesabına geçildi"),
                        &body,
                        None,
                    );
                }
            }
            refresh_accounts(cx);
        });
    })
    .detach();
}

pub fn refresh_usage(cx: &mut App) {
    let state = cx.global::<Globals>().state.clone();
    if state.read(cx).usage_scanning {
        return;
    }
    state.update(cx, |s, cx| {
        s.usage_scanning = true;
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let (prices, recs) = cx
            .background_executor()
            .spawn(async {
                let prices = PriceBook::load_with_overlay();
                let recs = usage::collect_records();
                (prices, recs)
            })
            .await;
        let _ = state.update(cx, |s, cx| {
            s.prices = Arc::new(prices);
            s.records = Arc::new(recs);
            s.usage_scanning = false;
            s.usage_scanned_once = true;
            s.recompute_summary();
            cx.notify();
        });
    })
    .detach();
}

pub fn update_title(cx: &mut App) {
    let g = cx.global::<Globals>();
    let state = g.state.read(cx);
    let mode = state.prefs.percent_mode;
    let title = match (state.prefs.title_mode, state.active_account()) {
        (TitleMode::Off, _) | (_, None) => String::new(),
        (tm, Some(acc)) => {
            let u = acc.usage.as_ref();
            let h5 = u
                .and_then(|u| u.five_hour.as_ref())
                .map(|w| format!("{:.0}%", mode.value(w.pct)));
            let d7 = u
                .and_then(|u| u.seven_day.as_ref())
                .map(|w| format!("{:.0}%", mode.value(w.pct)));
            match (tm, h5, d7) {
                (TitleMode::Both, Some(a), Some(b)) => format!(" {a}·{b}"),
                (_, Some(a), _) => format!(" {a}"),
                _ => String::new(),
            }
        }
    };
    g.status.set_title(&title);
    // the tray icon fits one number; its tooltip has the full reading
    #[cfg(target_os = "windows")]
    {
        let tip = match state.active_account() {
            None => APP_NAME.to_string(),
            Some(acc) => {
                let u = acc.usage.as_ref();
                let parts: Vec<String> = [
                    ("5h", u.and_then(|u| u.five_hour.as_ref())),
                    ("7d", u.and_then(|u| u.seven_day.as_ref())),
                ]
                .into_iter()
                .filter_map(|(name, w)| {
                    let v = mode.value(w?.pct);
                    Some(lf!("{name} {v:.0}%", "{name} %{v:.0}"))
                })
                .collect();
                if parts.is_empty() {
                    format!("{APP_NAME} · {}", acc.name())
                } else {
                    format!(
                        "{APP_NAME} · {}\n{} {}",
                        acc.name(),
                        parts.join(" · "),
                        mode.suffix()
                    )
                }
            }
        };
        g.status.set_tooltip(&tip);
    }
}

/// Mutate prefs, persist, and refresh everything that depends on them.
pub fn update_prefs(cx: &mut App, f: impl FnOnce(&mut Prefs)) {
    let state = cx.global::<Globals>().state.clone();
    state.update(cx, |s, cx| {
        f(&mut s.prefs);
        i18n::set(s.prefs.language);
        s.prefs.save();
        cx.notify();
    });
    let prefs = state.read(cx).prefs.clone();
    theme::apply(prefs.theme);
    hotkey::set_enabled(prefs.hotkeys);
    update_title(cx);
}

pub fn refresh_config(cx: &mut App) {
    let state = cx.global::<Globals>().state.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            .spawn(async { cswap::config_list() })
            .await;
        let _ = state.update(cx, |s, cx| {
            match result {
                Ok(list) => {
                    s.cswap_config = Some(list);
                    s.config_error = None;
                }
                Err(e) => s.config_error = Some(e),
            }
            cx.notify();
        });
    })
    .detach();
}

/// `value = None` resets the key to its cswap default.
pub fn set_cswap_config(cx: &mut App, key: String, value: Option<String>) {
    let state = cx.global::<Globals>().state.clone();
    state.update(cx, |s, cx| {
        s.config_busy = true;
        cx.notify();
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            .spawn(async move {
                match value {
                    Some(v) => cswap::config_set(&key, &v),
                    None => cswap::config_unset(&key),
                }
            })
            .await;
        let _ = state.update(cx, |s, cx| {
            s.config_busy = false;
            if let Err(e) = result {
                s.config_error = Some(e);
            }
            cx.notify();
        });
        let _ = cx.update(refresh_config);
    })
    .detach();
}

pub fn set_account_enabled(cx: &mut App, number: u32, enabled: bool) {
    let state = cx.global::<Globals>().state.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = cx
            .background_executor()
            .spawn(async move { cswap::set_enabled(number, enabled) })
            .await;
        let _ = state.update(cx, |s, cx| {
            if let Err(e) = result {
                s.accounts_error = Some(e);
            }
            cx.notify();
        });
        let _ = cx.update(refresh_accounts);
    })
    .detach();
}

pub fn open_settings(cx: &mut App) {
    if let Some(h) = cx.global::<Globals>().settings_window {
        if h.update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            cx.activate(true);
            return;
        }
        cx.global_mut::<Globals>().settings_window = None;
    }
    close_window(cx);
    refresh_config(cx);
    let bounds = Bounds::centered(None, size(px(760.0), px(540.0)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(lf!("{APP_NAME} Settings", "{APP_NAME} Ayarları").into()),
                appears_transparent: TRANSPARENT_TITLEBAR,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            is_resizable: true,
            is_minimizable: true,
            window_min_size: Some(size(px(640.0), px(440.0))),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| settings_ui::SettingsRoot::new(window, cx)),
    );
    match handle {
        Ok(h) => {
            cx.global_mut::<Globals>().settings_window = Some(h);
            cx.activate(true);
        }
        Err(e) => eprintln!("ayarlar penceresi açılamadı: {e}"),
    }
}

pub fn close_window(cx: &mut App) {
    debug_log(format_args!("close_window"));
    statusbar::watch_outside_clicks(false);
    let handle = cx.global_mut::<Globals>().window.take();
    if let Some(handle) = handle {
        // Called from one of the popover's own handlers (Settings…), the window is mid-update
        // and gpui refuses the nested update; remove it once that update has finished,
        // otherwise it stays on screen with nothing left that can close it.
        if handle
            .update(cx, |_, window, _| window.remove_window())
            .is_err()
        {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
    }
    cx.global::<Globals>().status.set_highlighted(false);
}

/// Close the popover because the user clicked or switched elsewhere.
pub fn dismiss_window(cx: &mut App) {
    if cx.global::<Globals>().window.is_none() {
        return;
    }
    // Pressing our own status item can take focus from the popover before its click
    // action runs; leave the close to that action so a slow click doesn't reopen it.
    if statusbar::mouse_over_button() {
        debug_log(format_args!("dismiss skipped: mouse over status item"));
        return;
    }
    cx.global_mut::<Globals>().last_dismiss = Some(Instant::now());
    close_window(cx);
}

/// Open the popover if it isn't showing.
pub fn open_popover(cx: &mut App) {
    if cx.global::<Globals>().window.is_none() {
        toggle_window(cx);
    }
}

pub fn toggle_window(cx: &mut App) {
    let now = Instant::now();
    let g = cx.global::<Globals>();
    let recent = |t: Option<Instant>| t.is_some_and(|t| now.duration_since(t) < TOGGLE_GRACE);
    let (repeated, just_dismissed) = (recent(g.last_toggle), recent(g.last_dismiss));
    debug_log(format_args!(
        "toggle_window: open={} repeated={repeated} just_dismissed={just_dismissed}",
        g.window.is_some()
    ));
    if repeated {
        return;
    }
    cx.global_mut::<Globals>().last_toggle = Some(now);
    if cx.global::<Globals>().window.is_some() {
        close_window(cx);
        return;
    }
    // The click that hit the status item also took focus from the open popover,
    // which already closed it; this click meant "close", not "open again".
    if just_dismissed {
        return;
    }
    let status = cx.global::<Globals>().status.clone();
    let (display_id, origin, height) = popover_placement(cx, &status);

    let bounds = Bounds {
        origin,
        size: size(px(WINDOW_WIDTH), px(height)),
    };
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            display_id,
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: gpui::WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| ui::Root::new(window, cx)),
    );
    match handle {
        Ok(h) => {
            cx.global_mut::<Globals>().window = Some(h);
            #[cfg(target_os = "windows")]
            let _ = h.update(cx, |_, window, _| statusbar::raise_popover(window));
            status.set_highlighted(true);
            statusbar::watch_outside_clicks(true);
            cx.activate(true);
            refresh_accounts(cx);
        }
        Err(e) => eprintln!("pencere açılamadı: {e}"),
    }
}

/// The popover's display, origin and height: just below the menu bar item.
#[cfg(target_os = "macos")]
fn popover_placement(
    cx: &App,
    status: &statusbar::StatusItem,
) -> (Option<gpui::DisplayId>, Point<Pixels>, f32) {
    let geo = status.geometry();
    if std::env::var("SWAPBAR_DEBUG").is_ok() {
        eprintln!(
            "geometry: {geo:?}; displays: {:?}",
            cx.displays()
                .iter()
                .map(|d| (d.id(), d.bounds()))
                .collect::<Vec<_>>()
        );
    }

    let height = geo
        .map(|g| (g.screen_height - g.bottom_y - 24.0).min(WINDOW_HEIGHT as f64) as f32)
        .unwrap_or(WINDOW_HEIGHT);
    let (display_id, origin) = match geo {
        Some(g) => {
            let display = cx
                .displays()
                .into_iter()
                .find(|d| format!("{:?}", d.id()) == format!("DisplayId({})", g.screen_display_id));
            let x = (g.x + g.width / 2.0 - WINDOW_WIDTH as f64 / 2.0)
                .clamp(8.0, (g.screen_width - WINDOW_WIDTH as f64 - 8.0).max(8.0));
            (
                display.map(|d| d.id()),
                point(px(x as f32), px(g.bottom_y as f32 + 4.0)),
            )
        }
        None => (None, point(px(200.0), px(30.0))),
    };
    (display_id, origin, height)
}

/// The popover's display, origin and height: beside the tray icon, on the taskbar's side.
#[cfg(target_os = "windows")]
fn popover_placement(
    cx: &App,
    status: &statusbar::StatusItem,
) -> (Option<gpui::DisplayId>, Point<Pixels>, f32) {
    let bounds = status.popover_bounds(WINDOW_WIDTH, WINDOW_HEIGHT);
    let display = cx
        .displays()
        .into_iter()
        .find(|d| d.bounds().contains(&bounds.center()));
    debug_log(format_args!(
        "popover bounds {bounds:?} on {:?}",
        display.as_ref().map(|d| d.id())
    ));
    (
        display.map(|d| d.id()),
        bounds.origin,
        f32::from(bounds.size.height),
    )
}
