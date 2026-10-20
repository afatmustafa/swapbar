//! Settings window: sidebar + grouped rows, in the style of macOS System Settings.

use gpui::{
    App, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, prelude::*, px, rgb,
};

use crate::assets::icon;
use crate::cswap::ConfigList;
use crate::i18n::{l, os};
use crate::lf;
use crate::settings::{Language, PercentMode, Prefs, Theme, TitleMode};
use crate::{APP_NAME, APP_VERSION, AppState, Globals, hotkey, notify};

#[cfg(target_os = "macos")]
pub(crate) const UI_FONT: &str = ".AppleSystemUIFont";
#[cfg(target_os = "macos")]
pub(crate) const MONO_FONT: &str = "Menlo";
#[cfg(target_os = "windows")]
pub(crate) const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "windows")]
pub(crate) const MONO_FONT: &str = "Consolas";

pub(crate) fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Colour palette; `t()` picks the light or dark one for the current appearance.
pub(crate) struct T {
    pub(crate) sidebar: u32,
    pub(crate) content: u32,
    pub(crate) group: u32,
    pub(crate) hairline: u32,
    pub(crate) hover: u32,
    pub(crate) selected: u32,
    pub(crate) text: u32,
    pub(crate) secondary: u32,
    pub(crate) tertiary: u32,
    pub(crate) accent: u32,
    pub(crate) switch_on: u32,
    pub(crate) switch_off: u32,
    pub(crate) danger: u32,
    pub(crate) ok: u32,
}

const LIGHT: T = T {
    sidebar: 0xf2f1ee,
    content: 0xfbfbfa,
    group: 0xffffff,
    hairline: 0xe6e5e1,
    hover: 0xe8e7e3,
    selected: 0xdcdbd6,
    text: 0x1d1d1f,
    secondary: 0x6e6e73,
    tertiary: 0xa1a1a6,
    accent: 0xff4101,
    switch_on: 0x3478f6,
    switch_off: 0xd1d0cc,
    danger: 0xd93025,
    ok: 0x248a3d,
};

const DARK: T = T {
    sidebar: 0x1c1c1b,
    content: 0x232322,
    group: 0x2c2c2a,
    hairline: 0x3a3a38,
    hover: 0x353533,
    selected: 0x41413e,
    text: 0xf2f2f0,
    secondary: 0xa1a1a6,
    tertiary: 0x707074,
    accent: 0xff5a1f,
    switch_on: 0x0a84ff,
    switch_off: 0x4a4a48,
    danger: 0xff5f52,
    ok: 0x30d158,
};

pub(crate) fn t() -> &'static T {
    if crate::theme::is_dark() {
        &DARK
    } else {
        &LIGHT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    MenuBar,
    Notifications,
    Accounts,
    AutoSwitch,
    About,
}

impl Page {
    const ALL: [Page; 6] = [
        Page::General,
        Page::MenuBar,
        Page::Notifications,
        Page::Accounts,
        Page::AutoSwitch,
        Page::About,
    ];
    fn label(self) -> &'static str {
        match self {
            Page::General => l("General", "Genel"),
            Page::MenuBar => os(
                l("Menu Bar", "Menü Çubuğu"),
                l("System Tray", "Sistem Tepsisi"),
            ),
            Page::Notifications => l("Notifications", "Bildirimler"),
            Page::Accounts => l("Accounts", "Hesaplar"),
            Page::AutoSwitch => l("Auto-switch", "Otomatik geçiş"),
            Page::About => l("About", "Hakkında"),
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Page::General => "settings",
            Page::MenuBar => "laptop",
            Page::Notifications => "bell",
            Page::Accounts => "users",
            Page::AutoSwitch => "swap",
            Page::About => "info",
        }
    }
}

pub struct SettingsRoot {
    state: Entity<AppState>,
    page: Page,
    _subs: Vec<Subscription>,
}

impl SettingsRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.global::<Globals>().state.clone();
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        cx.on_release(|_, cx| {
            cx.global_mut::<Globals>().settings_window = None;
        })
        .detach();
        let page = match std::env::var("SWAPBAR_PAGE").as_deref() {
            Ok("menubar") => Page::MenuBar,
            Ok("notifications") => Page::Notifications,
            Ok("accounts") => Page::Accounts,
            Ok("auto") => Page::AutoSwitch,
            Ok("about") => Page::About,
            _ => Page::General,
        };
        SettingsRoot {
            state,
            page,
            _subs: subs,
        }
    }
}

// ---------- controls ----------

pub(crate) fn toggle(
    id: impl Into<SharedString>,
    on: bool,
    on_change: impl Fn(bool, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .w(px(38.0))
        .h(px(22.0))
        .rounded_full()
        .bg(if on {
            c(t().switch_on)
        } else {
            c(t().switch_off)
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .px(px(2.0))
        .when(on, |d| d.justify_end())
        .child(
            div()
                .size(px(18.0))
                .rounded_full()
                .bg(c(0xffffff))
                .shadow_sm(),
        )
        .on_click(move |_, _, cx| on_change(!on, cx))
}

fn segmented<V: Copy + PartialEq + 'static>(
    id: &str,
    options: &[V],
    selected: V,
    label: fn(V) -> &'static str,
    on_select: impl Fn(V, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let mut row = div()
        .flex()
        .p(px(2.0))
        .rounded_md()
        .bg(c(t().hover))
        .gap(px(2.0));
    for (i, opt) in options.iter().copied().enumerate() {
        let on_select = on_select.clone();
        let is_sel = opt == selected;
        row = row.child(
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .px_2p5()
                .py(px(3.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(c(t().text))
                .when(is_sel, |d| {
                    d.bg(c(t().group))
                        .shadow_sm()
                        .font_weight(FontWeight::MEDIUM)
                })
                .child(label(opt).to_string())
                .on_click(move |_, _, cx| on_select(opt, cx)),
        );
    }
    row
}

fn stepper(
    id: &str,
    value: String,
    on_step: impl Fn(i64, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let dec = on_step.clone();
    let inc = on_step;
    let btn = |id: String, glyph: &str| {
        div()
            .id(SharedString::from(id))
            .w(px(24.0))
            .h(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(13.0))
            .text_color(c(t().secondary))
            .hover(|d| d.bg(c(t().hover)).text_color(c(t().text)))
            .child(glyph.to_string())
    };
    div()
        .flex()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(c(t().hairline))
        .bg(c(t().group))
        .overflow_hidden()
        .child(btn(format!("{id}-dec"), "−").on_click(move |_, _, cx| dec(-1, cx)))
        .child(
            div()
                .min_w(px(64.0))
                .px_2()
                .flex()
                .justify_center()
                .font_family(MONO_FONT)
                .text_size(px(11.5))
                .text_color(c(t().text))
                .child(value),
        )
        .child(btn(format!("{id}-inc"), "+").on_click(move |_, _, cx| inc(1, cx)))
}

pub(crate) fn button(
    id: &str,
    label: &str,
    danger: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .px_3()
        .py(px(4.0))
        .rounded_md()
        .border_1()
        .border_color(c(t().hairline))
        .bg(c(t().group))
        .cursor_pointer()
        .text_size(px(11.5))
        .text_color(c(if danger { t().danger } else { t().text }))
        .hover(|d| d.bg(c(t().hover)))
        .child(label.to_string())
        .on_click(move |_, _, cx| on_click(cx))
}

/// The filled accent button for a window's main action.
pub(crate) fn primary_button(
    id: &str,
    label: &str,
    enabled: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .px_4()
        .py(px(5.0))
        .rounded_md()
        .bg(c(t().accent))
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(0xffffff))
        .when(!enabled, |d| d.opacity(0.45))
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|d| d.opacity(0.88))
                .on_click(move |_, _, cx| on_click(cx))
        })
        .child(label.to_string())
}

fn link(id: &str, glyph: &str, label: &str, url: &'static str) -> impl IntoElement {
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .h(px(36.0))
        .cursor_pointer()
        .hover(|d| d.bg(c(t().hover)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .child(icon(glyph, 15.0, c(t().secondary)))
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(c(t().text))
                        .child(label.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div().text_size(px(11.0)).text_color(c(t().tertiary)).child(
                        url.trim_start_matches("https://")
                            .trim_end_matches('/')
                            .to_string(),
                    ),
                )
                .child(icon("external", 12.0, c(t().tertiary))),
        )
        .on_click(move |_, _, cx| cx.open_url(url))
}

// ---------- layout ----------

pub(crate) fn group(children: Vec<gpui::AnyElement>) -> impl IntoElement {
    let n = children.len();
    let mut g = div()
        .flex()
        .flex_col()
        .rounded_lg()
        .bg(c(t().group))
        .border_1()
        .border_color(c(t().hairline))
        .overflow_hidden();
    for (i, ch) in children.into_iter().enumerate() {
        g = g.child(
            div()
                .when(i + 1 < n, |d| d.border_b_1().border_color(c(t().hairline)))
                .child(ch),
        );
    }
    g
}

pub(crate) fn row(
    label: &str,
    description: Option<&str>,
    control: impl IntoElement,
) -> gpui::AnyElement {
    let mut text = div()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text_size(px(12.5))
                .text_color(c(t().text))
                .child(label.to_string()),
        );
    if let Some(d) = description {
        text = text.child(
            div()
                .text_size(px(10.5))
                .text_color(c(t().secondary))
                .child(d.to_string()),
        );
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_3()
        .py_2p5()
        .min_h(px(44.0))
        .child(text)
        .child(control)
        .into_any_element()
}

pub(crate) fn heading(label: &str) -> impl IntoElement {
    div()
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(t().text))
        .mt_3()
        .mb_1()
        .child(label.to_string())
}

// ---------- cswap config helpers ----------

fn cfg<'a>(list: Option<&'a ConfigList>, key: &str) -> Option<&'a serde_json::Value> {
    list?
        .settings
        .iter()
        .find(|e| e.key == key)
        .map(|e| &e.value)
}
fn cfg_num(list: Option<&ConfigList>, key: &str, default: f64) -> f64 {
    cfg(list, key).and_then(|v| v.as_f64()).unwrap_or(default)
}
fn cfg_str(list: Option<&ConfigList>, key: &str) -> Option<String> {
    cfg(list, key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
fn cfg_bool(list: Option<&ConfigList>, key: &str) -> bool {
    cfg(list, key).and_then(|v| v.as_bool()).unwrap_or(false)
}

// A flat argument list reads better at the call sites than a one-off options struct.
#[expect(clippy::too_many_arguments)]
fn num_setting(
    id: &'static str,
    key: &'static str,
    list: Option<&ConfigList>,
    default: f64,
    step: f64,
    min: f64,
    max: f64,
    unit: &str,
) -> impl IntoElement {
    let cur = cfg_num(list, key, default);
    let shown = if unit.is_empty() {
        format!("{cur:.0}")
    } else {
        format!("{cur:.0}{unit}")
    };
    stepper(id, shown, move |dir, cx| {
        let next = (cur + dir as f64 * step).clamp(min, max);
        crate::set_cswap_config(cx, key.to_string(), Some(format!("{next:.0}")));
    })
}

// ---------- pages ----------

fn page_general(s: &AppState) -> impl IntoElement {
    let p = &s.prefs;
    let launch = crate::settings::launch_agent_installed();
    div()
        .flex()
        .flex_col()
        .child(heading(l("Appearance", "Görünüm")))
        .child(group(vec![
            row(
                l("Theme", "Tema"),
                Some(os(
                    l("System follows the macOS light/dark setting.", "Sistem, macOS'un açık/koyu ayarını izler."),
                    l("System follows the Windows light/dark setting.", "Sistem, Windows'un açık/koyu ayarını izler."),
                )),
                segmented("theme", &Theme::ALL, p.theme, Theme::label, |t, cx| crate::update_prefs(cx, |p| p.theme = t)),
            ),
            row(
                l("Language", "Dil"),
                Some(os(
                    l("System follows the macOS language: Turkish if it's first, otherwise English.", "Sistem, macOS dilini izler: ilk dil Türkçeyse Türkçe, değilse İngilizce."),
                    l("System follows the Windows display language: Turkish if it's Turkish, otherwise English.", "Sistem, Windows görüntüleme dilini izler: Türkçeyse Türkçe, değilse İngilizce."),
                )),
                segmented("language", &Language::ALL, p.language, Language::label, |lang, cx| crate::update_prefs(cx, |p| p.language = lang)),
            ),
        ]))
        .child(heading(l("Startup", "Başlangıç")))
        .child(group(vec![row(
            l("Launch at login", "Girişte başlat"),
            Some(os(
                l("Installs a LaunchAgent that starts the app when you sign in.", "Mac'e giriş yaptığında uygulamayı başlatan bir LaunchAgent kurar."),
                l("Starts Swapbar when you sign in to Windows, from the Run key in the registry.", "Windows'a giriş yaptığında Swapbar'ı kayıt defterindeki Run anahtarıyla başlatır."),
            )),
            toggle("launch", launch, |on, cx| {
                if let Err(e) = crate::settings::set_launch_at_login(on) {
                    eprintln!("launch agent: {e}");
                }
                crate::update_prefs(cx, |p| p.launch_at_login = on);
            }),
        )]))
        .child(heading(l("Refresh", "Yenileme")))
        .child(group(vec![
            row(
                l("Account quotas", "Hesap kotaları"),
                Some(l("How often to ask cswap for fresh limits. cswap itself paces the Anthropic API.", "cswap'a ne sıklıkla güncel limit sorulacağı. Anthropic API'sinin temposunu cswap kendisi ayarlar.")),
                stepper("acc-refresh", format!("{}s", p.accounts_refresh_secs), |d, cx| {
                    crate::update_prefs(cx, |p| p.accounts_refresh_secs = (p.accounts_refresh_secs as i64 + d * 15).clamp(15, 600) as u64)
                }),
            ),
            row(
                l("Token & cost scan", "Token ve maliyet taraması"),
                Some(l("How often to re-scan Claude Code transcripts. Scans are incremental.", "Claude Code transkriptlerinin ne sıklıkla yeniden taranacağı. Taramalar artımlıdır.")),
                stepper("usage-refresh", format!("{}s", p.usage_refresh_secs), |d, cx| {
                    crate::update_prefs(cx, |p| p.usage_refresh_secs = (p.usage_refresh_secs as i64 + d * 30).clamp(30, 1800) as u64)
                }),
            ),
        ]))
        .child(heading(l("Sections", "Bölümler")))
        .child(group(vec![row(
            l("Show token & cost usage", "Token ve maliyet kullanımını göster"),
            Some(l("Today / 30-day totals under the account list.", "Hesap listesinin altında bugün ve 30 günlük toplamlar.")),
            toggle("show-usage", p.show_usage, |on, cx| crate::update_prefs(cx, |p| p.show_usage = on)),
        )]))
        .child(heading(l("Keyboard shortcuts", "Klavye kısayolları")))
        .child(group(vec![
            row(
                l("Global shortcuts", "Global kısayollar"),
                Some(l("Work from any app. Turn off if another app needs these keys.", "Her uygulamada çalışır. Bu tuşlara başka bir uygulama ihtiyaç duyuyorsa kapat.")),
                toggle("hotkeys", p.hotkeys, |on, cx| crate::update_prefs(cx, |p| p.hotkeys = on)),
            ),
            row(l("Open Swapbar", "Swapbar'ı aç"), None, shortcut(hotkey::TOGGLE_LABEL, p.hotkeys)),
            row(
                l("Switch to the account with the most quota left", "En çok kotası kalan hesaba geç"),
                None,
                shortcut(hotkey::SWITCH_BEST_LABEL, p.hotkeys),
            ),
        ]))
        .child(heading(l("Updates", "Güncellemeler")))
        .child(group(vec![
            row(
                l("Check for updates automatically", "Güncellemeleri otomatik kontrol et"),
                Some(l("Looks for a new release on GitHub at launch and every 6 hours.", "Açılışta ve 6 saatte bir GitHub'da yeni sürüm arar.")),
                toggle("check-updates", p.check_updates, |on, cx| {
                    crate::update_prefs(cx, |p| p.check_updates = on);
                    if on {
                        crate::check_for_updates(cx, false);
                    }
                }),
            ),
            update_row(s),
        ]))
}

pub(crate) fn shortcut(keys: &str, enabled: bool) -> impl IntoElement {
    div()
        .px_2()
        .py(px(2.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(c(t().hairline))
        .bg(c(t().content))
        .text_size(px(12.0))
        .text_color(c(if enabled { t().text } else { t().tertiary }))
        .child(keys.to_string())
}

fn update_row(s: &AppState) -> gpui::AnyElement {
    let status = if s.update_checking {
        l("Checking…", "Kontrol ediliyor…").to_string()
    } else if let Some(m) = &s.update_message {
        m.clone()
    } else {
        lf!(
            "You have version {APP_VERSION}.",
            "Sizdeki sürüm: {APP_VERSION}."
        )
    };
    let control: gpui::AnyElement = match &s.update {
        Some(r) => {
            let url = r.url.clone();
            button(
                "download-update",
                &lf!("Download {}", "{} sürümünü indir", r.version),
                false,
                move |cx| cx.open_url(&url),
            )
            .into_any_element()
        }
        None => button(
            "check-update",
            l("Check now", "Şimdi kontrol et"),
            false,
            |cx| crate::check_for_updates(cx, true),
        )
        .into_any_element(),
    };
    row(l("Latest version", "Son sürüm"), Some(&status), control)
}

fn page_notifications(s: &AppState) -> impl IntoElement {
    let p = &s.prefs;
    let mut col = div().flex().flex_col();
    if !notify::available() {
        col = col.child(
            div()
                .text_size(px(11.0))
                .text_color(c(t().danger))
                .mt_3()
                .child(l("Notifications need Swapbar to run from its app bundle (Swapbar.app).", "Bildirimler için Swapbar'ın uygulama paketinden (Swapbar.app) çalışması gerekir.")),
        );
    }
    col.child(heading(l("Quota", "Kota")))
        .child(group(vec![
            row(
                l("Low quota at", "Düşük kota eşiği"),
                Some(l("A window counts as low at this much left. Swapbar then suggests the account with the most room.", "Bir pencere bu kadar kalınca düşük sayılır. Swapbar o zaman en çok yeri olan hesabı önerir.")),
                stepper("low-pct", format!("{}%", p.low_quota_pct), |d, cx| {
                    crate::update_prefs(cx, |p| p.low_quota_pct = (p.low_quota_pct as i64 + d * 5).clamp(5, 50) as u32)
                }),
            ),
            row(
                l("When the current account runs low", "Aktif hesabın kotası azalınca"),
                Some(os(
                    l("Once per 5h or 7d window, with a Switch button for the best other account.", "5h veya 7d penceresi başına bir kez, en uygun diğer hesap için Geç butonuyla."),
                    l("Once per 5h or 7d window. Click it to see the best other account.", "5h veya 7d penceresi başına bir kez. Tıklayınca en uygun diğer hesabı gösterir."),
                )),
                toggle("notify-low", p.notify_low, |on, cx| {
                    crate::update_prefs(cx, |p| p.notify_low = on);
                    if on {
                        notify::request_authorization();
                    }
                }),
            ),
            row(
                l("When the current account won't last", "Aktif hesabın kotası yetmeyecekse"),
                Some(l("When the 5h or 7d window would run out before it resets at your current pace. Once per window.", "Mevcut tempoyla 5h veya 7d penceresi sıfırlanmadan bitecekse. Pencere başına bir kez.")),
                toggle("notify-pace", p.notify_pace, |on, cx| {
                    crate::update_prefs(cx, |p| p.notify_pace = on);
                    if on {
                        notify::request_authorization();
                    }
                }),
            ),
            row(
                l("When an account's quota resets", "Bir hesabın kotası sıfırlanınca"),
                Some(l("For accounts that had run low. Paused and hidden accounts are skipped.", "Kotası azalmış hesaplar için. Duraklatılmış ve gizli hesaplar atlanır.")),
                toggle("notify-reset", p.notify_reset, |on, cx| {
                    crate::update_prefs(cx, |p| p.notify_reset = on);
                    if on {
                        notify::request_authorization();
                    }
                }),
            ),
        ]))
        .child(heading(l("System", "Sistem")))
        .child(group(vec![
            row(
                l("Send a test notification", "Deneme bildirimi gönder"),
                Some(os(
                    l("If nothing appears, allow Swapbar in System Settings › Notifications.", "Bir şey görünmezse Sistem Ayarları › Bildirimler'de Swapbar'a izin ver."),
                    l("If nothing appears, check Settings › System › Notifications and Do not disturb.", "Bir şey görünmezse Ayarlar › Sistem › Bildirimler'i ve Rahatsız etme'yi kontrol et."),
                )),
                div()
                    .flex()
                    .gap_2()
                    .child(button("test-notify", l("Send test", "Dene"), false, |_| {
                        notify::request_authorization();
                        notify::post("test", l("Swapbar notifications are on", "Swapbar bildirimleri açık"), l("You'll hear from Swapbar when quota runs low or resets.", "Kota azalınca ya da sıfırlanınca Swapbar haber verecek."), None);
                    }))
                    .child(button(
                        "open-notify-settings",
                        os(l("Open System Settings", "Sistem Ayarları'nı aç"), l("Open notification settings", "Bildirim ayarlarını aç")),
                        false,
                        |_| notify::open_system_settings(),
                    )),
            ),
        ]))
}

fn page_menubar(s: &AppState) -> impl IntoElement {
    let p: &Prefs = &s.prefs;
    div()
        .flex()
        .flex_col()
        .child(heading(os(l("Menu bar", "Menü çubuğu"), l("Tray icon", "Tepsi ikonu"))))
        .child(group(vec![
            row(
                l("Percentage", "Yüzde"),
                Some(os(
                    l("Show how much quota is left, or how much has been used. Applies to the menu bar and the account list.", "Ne kadar kota kaldığını ya da ne kadar kullanıldığını göster. Menü çubuğu ve hesap listesi için geçerli."),
                    l("Show how much quota is left, or how much has been used. Applies to the tray icon and the account list.", "Ne kadar kota kaldığını ya da ne kadar kullanıldığını göster. Tepsi ikonu ve hesap listesi için geçerli."),
                )),
                segmented("pct-mode", &PercentMode::ALL, p.percent_mode, PercentMode::label, |m, cx| crate::update_prefs(cx, |p| p.percent_mode = m)),
            ),
            title_mode_row(p),
        ]))
        .child(heading(l("Account rows", "Hesap satırları")))
        .child(group(vec![
            row(l("Show 7-day window", "7 günlük pencereyi göster"), None, toggle("show-7d", p.show_seven_day, |on, cx| crate::update_prefs(cx, |p| p.show_seven_day = on))),
            row(l("Show per-model limits", "Model bazlı limitleri göster"), Some(l("Weekly model-scoped windows such as Fable.", "Fable gibi haftalık model bazlı pencereler.")), toggle("show-scoped", p.show_model_limits, |on, cx| crate::update_prefs(cx, |p| p.show_model_limits = on))),
            row(l("Show reset times", "Sıfırlanma zamanlarını göster"), None, toggle("show-reset", p.show_reset_times, |on, cx| crate::update_prefs(cx, |p| p.show_reset_times = on))),
            row(l("Show pace", "Tempoyu göster"), Some(l("Mark windows used faster than an even pace, and say when they would run out.", "Eşit tempodan hızlı harcanan pencereleri işaretle ve ne zaman biteceklerini söyle.")), toggle("show-pace", p.show_pace, |on, cx| crate::update_prefs(cx, |p| p.show_pace = on))),
        ]))
}

/// What shows beside the menu bar icon. A tray icon only has room for one number, so on
/// Windows the 5h figure replaces the logo and the tooltip carries both windows.
fn title_mode_row(p: &Prefs) -> gpui::AnyElement {
    let set = |m: TitleMode, cx: &mut App| crate::update_prefs(cx, |p| p.title_mode = m);
    if cfg!(target_os = "windows") {
        let current = match p.title_mode {
            TitleMode::Both => TitleMode::FiveHour,
            m => m,
        };
        row(
            l("Number in the icon", "İkondaki sayı"),
            Some(l(
                "The current account's 5h quota in place of the logo. Hover the icon for both windows.",
                "Logonun yerine aktif hesabın 5h kotası. İki pencere için ikonun üzerine gel.",
            )),
            segmented(
                "title-mode",
                &[TitleMode::Off, TitleMode::FiveHour],
                current,
                TitleMode::label,
                set,
            ),
        )
    } else {
        row(
            l("Title next to the icon", "İkonun yanındaki başlık"),
            Some(l(
                "Which windows of the current account to show in the menu bar.",
                "Aktif hesabın hangi pencerelerinin menü çubuğunda gösterileceği.",
            )),
            segmented(
                "title-mode",
                &TitleMode::ALL,
                p.title_mode,
                TitleMode::label,
                set,
            ),
        )
    }
}

/// A small text button under an account row.
fn action(
    id: String,
    label: &str,
    color: u32,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id))
        .px_1p5()
        .py(px(1.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(c(color))
        .hover(|d| d.bg(c(t().hover)))
        .child(label.to_string())
        .on_click(move |_, _, cx| on_click(cx))
}

fn labelled_toggle(label: &str, toggle: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(c(t().secondary))
                .child(label.to_string()),
        )
        .child(toggle)
}

fn account_settings_row(
    acc: &crate::cswap::Account,
    s: &AppState,
    first: bool,
    last: bool,
    broken: bool,
) -> gpui::AnyElement {
    let n = acc.number;
    let hidden = s.prefs.is_hidden(n);
    let enabled = !acc.disabled;
    let mut desc = Vec::new();
    if acc.alias.as_deref().is_some_and(|a| !a.is_empty()) {
        desc.push(acc.email.clone());
    }
    desc.push(acc.org_tag());
    if let Some(p) = &acc.plan {
        desc.push(p.clone());
    }
    if acc.active {
        desc.push(l("current", "aktif").into());
    }
    let problem = if acc.needs_login() {
        acc.status_label()
    } else if broken {
        Some(
            l(
                "Shares a saved login with another slot",
                "Kayıtlı girişi başka bir slotla aynı",
            )
            .to_string(),
        )
    } else {
        None
    };
    let relogin_color = if problem.is_some() {
        t().accent
    } else {
        t().secondary
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(c(t().text))
                                .child(format!("{n}  {}", acc.name())),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(c(t().secondary))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(desc.join(" · ")),
                        )
                        .when_some(problem, |d, p| {
                            d.child(div().text_size(px(10.5)).text_color(c(t().danger)).child(p))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(labelled_toggle(
                            l("Show in menu", "Menüde göster"),
                            toggle(format!("hide-{n}"), !hidden, move |on, cx| {
                                crate::update_prefs(cx, |p| {
                                    if on != !p.is_hidden(n) {
                                        p.toggle_hidden(n);
                                    }
                                })
                            }),
                        ))
                        .child(labelled_toggle(
                            l("Auto-rotation", "Otomatik rotasyon"),
                            toggle(format!("enable-{n}"), enabled, move |on, cx| {
                                crate::set_account_enabled(cx, n, on)
                            }),
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .ml(px(-6.0))
                .child(action(
                    format!("rename-{n}"),
                    l("Rename", "Yeniden adlandır"),
                    t().secondary,
                    move |cx| crate::rename_account(cx, n),
                ))
                .child(action(
                    format!("relogin-{n}"),
                    l("Sign in again", "Yeniden giriş yap"),
                    relogin_color,
                    move |cx| crate::onboarding::open(cx, crate::onboarding::Entry::Relogin(n)),
                ))
                .when(!first, |d| {
                    d.child(action(
                        format!("up-{n}"),
                        l("Move up", "Yukarı taşı"),
                        t().secondary,
                        move |cx| crate::move_account(cx, n, true),
                    ))
                })
                .when(!last, |d| {
                    d.child(action(
                        format!("down-{n}"),
                        l("Move down", "Aşağı taşı"),
                        t().secondary,
                        move |cx| crate::move_account(cx, n, false),
                    ))
                })
                .child(div().flex_1())
                .child(action(
                    format!("remove-{n}"),
                    l("Remove", "Kaldır"),
                    t().danger,
                    move |cx| crate::remove_account(cx, n),
                )),
        )
        .into_any_element()
}

/// cswap's duplicate-login warning, without its CLI advice and in the interface language.
fn duplicate_warning(w: &str) -> String {
    let slots: Vec<String> = w
        .split("Account-")
        .skip(1)
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect())
        .collect();
    let email = w
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(e, _)| e);
    match (slots.as_slice(), email) {
        ([a, b], Some(e)) => lf!(
            "Accounts {a} and {b} hold the same saved login ({e}). Use “Sign in again” on the account whose login is wrong.",
            "{a} ve {b} numaralı hesaplar aynı kayıtlı girişi taşıyor ({e}). Girişi yanlış olan hesapta “Yeniden giriş yap”ı kullan."
        ),
        _ => {
            let text = w.split(" — ").next().unwrap_or(w);
            lf!(
                "{text}. Use “Sign in again” on the account whose login is wrong.",
                "{text}. Girişi yanlış olan hesapta “Yeniden giriş yap”ı kullan."
            )
        }
    }
}

fn page_accounts(s: &AppState) -> impl IntoElement {
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    match &s.accounts {
        Some(list) if !list.accounts.is_empty() => {
            let broken = list.duplicate_slots();
            warnings = list.duplicate_account_warnings.clone();
            let count = list.accounts.len();
            for (i, acc) in list.accounts.iter().enumerate() {
                rows.push(account_settings_row(
                    acc,
                    s,
                    i == 0,
                    i + 1 == count,
                    broken.contains(&acc.number),
                ));
            }
        }
        _ => rows.push(row(
            l("No accounts yet", "Henüz hesap yok"),
            Some(l(
                "Add the accounts you use with Claude Code below.",
                "Claude Code ile kullandığın hesapları aşağıdan ekle.",
            )),
            div(),
        )),
    }
    let mut col = div()
        .flex()
        .flex_col()
        .child(heading(l("Managed accounts", "Yönetilen hesaplar")))
        .child(div().text_size(px(11.0)).text_color(c(t().secondary)).mb_2().child(l("“Show in menu” only hides a row in this app. “Auto-rotation” runs `cswap enable/disable`, which keeps the account out of cswap's automatic switching.", "“Menüde göster” yalnızca bu uygulamadaki satırı gizler. “Otomatik rotasyon” `cswap enable/disable` çalıştırır; kapalıysa hesap cswap'ın otomatik geçişlerine katılmaz.")));
    for w in warnings {
        // drop cswap's CLI advice; "Sign in again" on the row does it
        let text = duplicate_warning(&w);
        col = col.child(
            div()
                .mb_2()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(c(t().hairline))
                .bg(c(t().group))
                .text_size(px(11.0))
                .text_color(c(t().danger))
                .child(text),
        );
    }
    if let Some(e) = &s.accounts_error {
        col = col.child(
            div()
                .mb_2()
                .text_size(px(11.0))
                .text_color(c(t().danger))
                .child(e.clone()),
        );
    }
    col.child(group(rows))
        .child(heading(l("Add an account", "Hesap ekle")))
        .child(group(vec![
            row(
                l("Sign in to another Claude account", "Başka bir Claude hesabına giriş yap"),
                Some(l("Opens claude.ai in your browser. cswap saves the login, and Claude Code switches back to the account it uses now.", "Tarayıcında claude.ai'yi açar. cswap girişi kaydeder, Claude Code da şu an kullandığı hesaba geri döner.")),
                primary_button("add-account", l("Add account…", "Hesap ekle…"), true, |cx| {
                    crate::onboarding::open(cx, crate::onboarding::Entry::AddAccount)
                }),
            ),
            row(
                l("Setup assistant", "Kurulum asistanı"),
                Some(l("Check Claude Code and claude-swap, and walk through adding accounts again.", "Claude Code ve claude-swap'ı kontrol et, hesap ekleme adımlarını yeniden izle.")),
                button("run-setup", l("Open", "Aç"), false, |cx| crate::onboarding::open(cx, crate::onboarding::Entry::Welcome)),
            ),
        ]))
}

fn page_autoswitch(s: &AppState) -> impl IntoElement {
    let list = s.cswap_config.as_ref();
    let strategy = cfg_str(list, "autoswitch.strategy").unwrap_or_else(|| "best".into());
    let model = cfg_str(list, "autoswitch.model").unwrap_or_default();
    let strategy_sel: &'static str = if strategy == "consume-first" {
        "consume-first"
    } else {
        "best"
    };
    let model_sel: &'static str = match model.as_str() {
        "" => "none",
        "all" => "all",
        _ => "Fable",
    };
    let mut col = div().flex().flex_col();
    if let Some(e) = &s.config_error {
        col = col.child(
            div()
                .text_size(px(11.0))
                .text_color(c(t().danger))
                .mt_3()
                .child(e.clone()),
        );
    }
    col = col
        .child(heading(l("Thresholds", "Eşikler")))
        .child(div().text_size(px(11.0)).text_color(c(t().secondary)).mb_2().child(l("These are cswap's own settings (`cswap config`). They drive `cswap auto`; run `cswap auto` or `cswap menubar --install-service` to have switching happen automatically.", "Bunlar cswap'ın kendi ayarları (`cswap config`) ve `cswap auto`'yu yönetir. Geçişlerin otomatik olması için `cswap auto` ya da `cswap menubar --install-service` çalıştır.")))
        .child(group(vec![
            row(l("Switch at", "Geçiş eşiği"), Some(l("Switch when the binding 5h/7d window reaches this much used.", "Belirleyici 5h/7d penceresi bu kadar kullanılınca geç.")), num_setting("threshold", "autoswitch.threshold", list, 90.0, 5.0, 50.0, 99.0, "%")),
            row(l("Hysteresis", "Histerezis"), Some(l("A target must beat the active account by this many points.", "Hedef hesap aktif hesabı bu kadar puan geçmeli.")), num_setting("hyst", "autoswitch.hysteresisPct", list, 10.0, 1.0, 0.0, 50.0, "%")),
            row(l("Poll interval", "Sorgu aralığı"), Some(l("For the `cswap auto` loop.", "`cswap auto` döngüsü için.")), num_setting("interval", "autoswitch.intervalSeconds", list, 60.0, 15.0, 15.0, 900.0, "s")),
            row(l("Cooldown", "Bekleme süresi"), Some(l("Minimum time between proactive switches.", "Kendiliğinden yapılan iki geçiş arasındaki en kısa süre.")), num_setting("cooldown", "autoswitch.cooldownSeconds", list, 300.0, 60.0, 0.0, 3600.0, "s")),
            row(l("Unhealthy after", "Sağlıksız sayma"), Some(l("Consecutive failed polls before an account is skipped.", "Bir hesap atlanmadan önceki art arda başarısız sorgu sayısı.")), num_setting("unhealthy", "autoswitch.unhealthyTicks", list, 3.0, 1.0, 1.0, 20.0, l(" polls", " sorgu"))),
        ]))
        .child(heading(l("Strategy", "Strateji")))
        .child(group(vec![
            row(
                l("Pick the target by", "Hedefi seçerken"),
                Some(l("best: most headroom · consume-first: soonest weekly reset", "best: en çok boş yer · consume-first: haftalık sıfırlanması en yakın")),
                segmented("strategy", &["best", "consume-first"], strategy_sel, |m| m, |m, cx| {
                    crate::set_cswap_config(cx, "autoswitch.strategy".into(), Some(m.to_string()))
                })
                .into_any_element(),
            ),
            row(
                l("Also watch model limits", "Model limitlerini de izle"),
                Some(l("Switch on weekly model-scoped limits too.", "Haftalık model bazlı limitlerde de geç.")),
                segmented("model", &["none", "Fable", "all"], model_sel, |m| m, |m, cx| {
                    crate::set_cswap_config(cx, "autoswitch.model".into(), if m == "none" { None } else { Some(m.to_string()) })
                })
                .into_any_element(),
            ),
            row(
                l("Include API-key accounts", "API anahtarlı hesapları dahil et"),
                Some(l("Allow rotating onto managed API-key accounts (billed per token).", "Yönetilen API anahtarlı hesaplara geçişe izin ver (token başına ücretlendirilir).")),
                toggle("apikeys", cfg_bool(list, "autoswitch.includeApiKeyAccounts"), |on, cx| crate::set_cswap_config(cx, "autoswitch.includeApiKeyAccounts".into(), Some(on.to_string()))),
            ),
        ]))
        .child(heading(""))
        .child(group(vec![row(
            l("Reset auto-switch settings", "Otomatik geçiş ayarlarını sıfırla"),
            Some(l("Clears every autoswitch.* key back to cswap defaults.", "Tüm autoswitch.* ayarlarını cswap varsayılanlarına döndürür.")),
            button("reset-auto", l("Reset to defaults", "Varsayılanlara dön"), true, |cx| {
                for k in ["threshold", "intervalSeconds", "cooldownSeconds", "hysteresisPct", "strategy", "includeApiKeyAccounts", "unhealthyTicks", "model"] {
                    crate::set_cswap_config(cx, format!("autoswitch.{k}"), None);
                }
            }),
        )]));
    if s.config_busy {
        col = col.child(
            div()
                .text_size(px(11.0))
                .text_color(c(t().tertiary))
                .mt_2()
                .child(l("saving…", "kaydediliyor…")),
        );
    }
    col
}

fn page_about() -> impl IntoElement {
    let built = option_env!("SWAPBAR_BUILT").unwrap_or(l("source build", "kaynak koddan derleme"));
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .py_6()
                .mt_3()
                .rounded_lg()
                .bg(c(t().group))
                .border_1()
                .border_color(c(t().hairline))
                .child(icon("logomark", 64.0, c(t().accent)))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c(t().text))
                        .mt_2()
                        .child(APP_NAME),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(c(t().secondary))
                        .child(lf!("Version {APP_VERSION}", "Sürüm {APP_VERSION}")),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(t().tertiary))
                        .child(built.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(c(t().secondary))
                        .mt_2()
                        .child(l(
                            "Every Claude account's quota in view, one click to switch.",
                            "Her Claude hesabının kotası gözünün önünde, geçiş tek tık.",
                        )),
                ),
        )
        .child(heading(l("Author", "Geliştirici")))
        .child(group(vec![row(
            "Mustafa Afat",
            Some(l(
                "Co-Owner & CTO of Miova and Founder & CEO of Fidkeep",
                "Miova'nın Ortağı ve CTO'su, Fidkeep'in Kurucusu ve CEO'su",
            )),
            div()
                .text_size(px(11.0))
                .text_color(c(t().tertiary))
                .child(l("Turkey", "Türkiye")),
        )]))
        .child(heading(l("Links", "Bağlantılar")))
        .child(group(vec![
            link(
                "l-src",
                "github",
                l("Swapbar source", "Swapbar kaynak kodu"),
                "https://github.com/afatmustafa/swapbar",
            )
            .into_any_element(),
            link(
                "l-web",
                "globe",
                l("Website", "Web sitesi"),
                "https://afat.me",
            )
            .into_any_element(),
            link("l-gh", "github", "GitHub", "https://github.com/afatmustafa").into_any_element(),
            link("l-x", "x", "X / Twitter", "https://twitter.com/afatmus").into_any_element(),
            link(
                "l-ig",
                "instagram",
                "Instagram",
                "https://www.instagram.com/afatmustafa/",
            )
            .into_any_element(),
            link(
                "l-miova",
                "rocket",
                "Miova",
                "https://github.com/miovaagency",
            )
            .into_any_element(),
        ]))
        .child(heading(l("Built with", "Kullanılanlar")))
        .child(group(vec![
            link(
                "l-cswap",
                "github",
                l(
                    "claude-swap — multi-account switching and quota polling",
                    "claude-swap — çoklu hesap geçişi ve kota sorgulama",
                ),
                "https://github.com/realiti4/claude-swap",
            )
            .into_any_element(),
            link(
                "l-gpui",
                "github",
                l(
                    "gpui — Zed's GPU-accelerated UI framework",
                    "gpui — Zed'in GPU hızlandırmalı arayüz kütüphanesi",
                ),
                "https://github.com/zed-industries/zed/tree/main/crates/gpui",
            )
            .into_any_element(),
            link(
                "l-hugeicons",
                "globe",
                l("Hugeicons — icon set", "Hugeicons — ikon seti"),
                "https://hugeicons.com/?via=afat",
            )
            .into_any_element(),
        ]))
        .child(
            div()
                .text_size(px(10.5))
                .text_color(c(t().tertiary))
                .mt_4()
                .mb_2()
                .flex()
                .justify_center()
                .child(l(
                    "© 2026 Mustafa Afat. MIT License.",
                    "© 2026 Mustafa Afat. MIT Lisansı.",
                )),
        )
}

// ---------- root ----------

impl Render for SettingsRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync(cx);
        let s = self.state.read(cx).clone();
        let page = self.page;

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(200.0))
            .h_full()
            .bg(c(t().sidebar))
            .border_r_1()
            .border_color(c(t().hairline))
            .pt(px(48.0))
            .px_2()
            .gap(px(2.0));
        for p in Page::ALL {
            let selected = p == page;
            sidebar = sidebar.child(
                div()
                    .id(SharedString::from(format!("nav-{}", p.label())))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(px(28.0))
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(c(t().selected)))
                    .when(!selected, |d| d.hover(|d| d.bg(c(t().hover))))
                    .child(div().w(px(18.0)).flex().justify_center().child(icon(
                        p.glyph(),
                        15.0,
                        c(t().secondary),
                    )))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(c(t().text))
                            .child(p.label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = p;
                        if p == Page::AutoSwitch {
                            crate::refresh_config(cx);
                        }
                        cx.notify();
                    })),
            );
        }

        let content: gpui::AnyElement = match page {
            Page::General => page_general(&s).into_any_element(),
            Page::MenuBar => page_menubar(&s).into_any_element(),
            Page::Notifications => page_notifications(&s).into_any_element(),
            Page::Accounts => page_accounts(&s).into_any_element(),
            Page::AutoSwitch => page_autoswitch(&s).into_any_element(),
            Page::About => page_about().into_any_element(),
        };

        div()
            .flex()
            .size_full()
            .bg(c(t().content))
            .font_family(UI_FONT)
            .text_color(c(t().text))
            .child(sidebar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .h(px(48.0))
                            .flex()
                            .items_center()
                            .px_6()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(page.label()),
                    )
                    .child(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .overflow_y_scroll()
                            .px_6()
                            .pb_6()
                            .child(content),
                    ),
            )
    }
}
