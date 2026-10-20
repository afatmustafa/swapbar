//! Setup window: checks for Claude Code and claude-swap, installs claude-swap, and adds
//! accounts by signing Claude Code in to each one. Opens on first run (no cswap or no
//! accounts), and from Settings › Accounts to add an account or repair a saved login.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, FontWeight, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, WeakEntity, Window, WindowBounds, WindowKind,
    WindowOptions, div, point, prelude::*, px, size,
};

use crate::assets::icon;
use crate::cswap;
use crate::dialog::{self, Alert};
use crate::i18n::{l, os};
use crate::lf;
use crate::settings_ui::{
    MONO_FONT, UI_FONT, button, c, group, heading, primary_button, row, shortcut, t, toggle,
};
use crate::setup::{self, AuthStatus, Login, Tools};
use crate::{APP_NAME, AppState, Globals, hotkey, notify};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The full walkthrough.
    Welcome,
    /// Straight to signing in another account.
    AddAccount,
    /// Sign this slot's account in again (its saved login stopped working).
    Relogin(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Install,
    Accounts,
    Finish,
}

impl Step {
    const ALL: [Step; 4] = [Step::Welcome, Step::Install, Step::Accounts, Step::Finish];
    fn label(self) -> &'static str {
        match self {
            Step::Welcome => l("Welcome", "Hoş geldin"),
            Step::Install => "claude-swap",
            Step::Accounts => l("Accounts", "Hesaplar"),
            Step::Finish => l("Finish", "Bitiş"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Job {
    Idle,
    Checking,
    Installing,
    /// Waiting for the browser sign-in.
    SigningIn,
    /// Saving the new login with cswap.
    Saving,
}

pub struct SetupRoot {
    state: Entity<AppState>,
    entry: Entry,
    step: Step,
    tools: Option<Tools>,
    signed_in: Option<AuthStatus>,
    job: Job,
    log: Arc<Mutex<Vec<String>>>,
    installed: bool,
    login: Option<Arc<Login>>,
    notice: Option<String>,
    /// Neutral status, e.g. a cancelled sign-in.
    hint: Option<String>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

const LOG_LINES: usize = 10;

impl SetupRoot {
    fn new(entry: Entry, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.global::<Globals>().state.clone();
        let subs = vec![
            cx.observe(&state, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        cx.on_release(|this, cx| {
            if let Some(login) = this.login.take() {
                login.cancel();
            }
            cx.global_mut::<Globals>().setup_window = None;
        })
        .detach();
        let mut root = SetupRoot {
            state,
            entry,
            step: Step::Welcome,
            tools: None,
            signed_in: None,
            job: Job::Idle,
            log: Arc::new(Mutex::new(Vec::new())),
            installed: false,
            login: None,
            notice: None,
            hint: None,
            error: None,
            _subs: subs,
        };
        root.enter(entry, cx);
        root
    }

    fn enter(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.entry = entry;
        self.step = match entry {
            Entry::Welcome => Step::Welcome,
            Entry::AddAccount | Entry::Relogin(_) => Step::Accounts,
        };
        self.notice = None;
        self.hint = None;
        self.error = None;
        self.recheck(cx);
    }

    fn walkthrough(&self) -> bool {
        self.entry == Entry::Welcome
    }

    fn busy(&self) -> bool {
        self.job != Job::Idle && self.job != Job::Checking
    }

    /// Look for the tools and the current Claude Code login again.
    fn recheck(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.job = Job::Checking;
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let (tools, auth) = cx
                .background_executor()
                .spawn(async {
                    let tools = setup::detect();
                    let auth = tools
                        .claude
                        .as_ref()
                        .and_then(|_| setup::auth_status().ok());
                    (tools, auth)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.tools = Some(tools);
                this.signed_in = auth;
                if this.job == Job::Checking {
                    this.job = Job::Idle;
                }
                cx.notify();
            });
            let _ = cx.update(crate::refresh_accounts);
        })
        .detach();
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.job = Job::Installing;
        self.error = None;
        self.log.lock().unwrap().clear();
        cx.notify();
        self.redraw_while(Job::Installing, cx);
        let log = self.log.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move { setup::install_cswap(&|line| log.lock().unwrap().push(line)) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.job = Job::Idle;
                match result {
                    Ok(()) => this.installed = true,
                    Err(e) => this.error = Some(e),
                }
                this.recheck(cx);
            });
        })
        .detach();
    }

    /// Redraw a few times a second while `job` runs: the install log and the sign-in
    /// URL arrive from reader threads.
    fn redraw_while(&self, job: Job, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let running = this.update(cx, |this, cx| {
                    cx.notify();
                    this.job == job
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Save the account Claude Code is already signed in to.
    fn add_signed_in(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let email = self.signed_in.as_ref().and_then(|s| s.email.clone());
        self.job = Job::Saving;
        self.error = None;
        self.notice = None;
        self.hint = None;
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async { cswap::add_current(None) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.job = Job::Idle;
                match result {
                    Ok(()) => {
                        this.notice = Some(lf!(
                            "Added {}.",
                            "{} eklendi.",
                            email.as_deref().unwrap_or(l("the account", "Hesap"))
                        ))
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            });
            let _ = cx.update(crate::refresh_accounts);
        })
        .detach();
    }

    /// Sign Claude Code in to another account in the browser, save it with cswap, then
    /// switch Claude Code back to the account it was using.
    fn sign_in(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let list = self.state.read(cx).accounts.clone().unwrap_or_default();
        let previous = list
            .accounts
            .iter()
            .find(|a| a.active)
            .map(|a| (a.number, a.name().to_string()));
        let repair = match self.entry {
            Entry::Relogin(n) => list
                .accounts
                .iter()
                .find(|a| a.number == n)
                .map(|a| (n, a.email.clone())),
            _ => None,
        };
        let managed: Vec<String> = list.accounts.iter().map(|a| a.email.clone()).collect();
        let signed_in = self.signed_in.as_ref().and_then(|s| s.email.clone());

        self.job = Job::SigningIn;
        self.error = None;
        self.notice = None;
        self.hint = None;
        self.redraw_while(Job::SigningIn, cx);
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let bg = cx.background_executor().clone();
            // Refresh the backup of the current login first: the sign-in replaces it.
            let email = repair.as_ref().map(|(_, e)| e.clone());
            let started = bg
                .spawn(async move {
                    if signed_in.is_some_and(|e| managed.contains(&e)) {
                        let _ = cswap::add_current(None);
                    }
                    Login::start(email.as_deref()).map(Arc::new)
                })
                .await;
            let login = match started {
                Ok(login) => login,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| this.fail(e, cx));
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.login = Some(login.clone());
                cx.notify();
            });

            let waiting = login.clone();
            let signed = bg.spawn(async move { waiting.wait() }).await;
            let still_ours = this
                .update(cx, |this, _| {
                    this.login.as_ref().is_some_and(|l| Arc::ptr_eq(l, &login))
                })
                .unwrap_or(false);
            if !still_ours {
                return; // cancelled
            }
            if let Err(e) = signed {
                let _ = this.update(cx, |this, cx| this.fail(e, cx));
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.login = None;
                this.job = Job::Saving;
                cx.notify();
            });

            let outcome = bg
                .spawn(async move { save_new_login(repair, previous) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.job = Job::Idle;
                match outcome {
                    Ok(msg) => this.notice = Some(msg),
                    Err(e) => this.error = Some(e),
                }
                this.recheck(cx);
            });
        })
        .detach();
    }

    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.login = None;
        self.job = Job::Idle;
        self.error = Some(error);
        cx.notify();
    }

    fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        if let Some(login) = self.login.take() {
            login.cancel();
        }
        self.job = Job::Idle;
        self.hint = Some(
            l(
                "Sign-in cancelled. Claude Code keeps the account it had.",
                "Giriş iptal edildi. Claude Code önceki hesabında kaldı.",
            )
            .into(),
        );
        cx.notify();
    }

    fn paste_code(&mut self, cx: &mut Context<Self>) {
        let Some(login) = self.login.clone() else {
            return;
        };
        let this = cx.entity().downgrade();
        dialog::ask(
            cx,
            Alert::confirm(
                l("Paste the sign-in code", "Giriş kodunu yapıştır"),
                l("If the browser shows a code instead of returning to Claude Code, paste it here.", "Tarayıcı Claude Code'a dönmek yerine bir kod gösteriyorsa buraya yapıştır."),
                l("Continue", "Devam"),
            )
            .with_input("", l("Code from the browser", "Tarayıcıdaki kod")),
            move |code, cx| {
                let Some(code) = code.filter(|c| !c.trim().is_empty()) else {
                    return;
                };
                if let Err(e) = login.paste_code(&code) {
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(e);
                        cx.notify();
                    });
                }
            },
        );
    }

    fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        self.step = step;
        self.notice = None;
        self.hint = None;
        self.error = None;
        if step == Step::Accounts {
            self.recheck(cx);
        }
        cx.notify();
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_cswap = self.tools.as_ref().is_some_and(|t| t.cswap.is_some());
        match self.step {
            Step::Welcome if has_cswap => self.go(Step::Accounts, cx),
            Step::Welcome => self.go(Step::Install, cx),
            Step::Install => self.go(Step::Accounts, cx),
            Step::Accounts if self.walkthrough() => self.go(Step::Finish, cx),
            Step::Accounts | Step::Finish => {
                window.remove_window();
                if self.walkthrough() {
                    cx.defer(crate::open_popover);
                }
            }
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let has_cswap = self.tools.as_ref().is_some_and(|t| t.cswap.is_some());
        let prev = match self.step {
            Step::Welcome => return,
            Step::Install => Step::Welcome,
            Step::Accounts if self.installed || !has_cswap => Step::Install,
            Step::Accounts => Step::Welcome,
            Step::Finish => Step::Accounts,
        };
        self.go(prev, cx);
    }
}

/// After a successful browser sign-in: check it's the right account (repairs), save it,
/// and switch Claude Code back. Returns the message to show.
fn save_new_login(
    repair: Option<(u32, String)>,
    previous: Option<(u32, String)>,
) -> Result<String, String> {
    let status = setup::auth_status()?;
    let email = status.email.clone().ok_or(l(
        "Claude Code isn't signed in",
        "Claude Code'da giriş yapılmamış",
    ))?;
    if let Some((n, expected)) = &repair
        && !email.eq_ignore_ascii_case(expected)
    {
        return Err(lf!(
            "You signed in as {email}, but account {n} is {expected}. Try again and pick {expected}.",
            "{email} ile giriş yaptın, ama {n} numaralı hesap {expected}. Tekrar dene ve {expected} hesabını seç."
        ));
    }
    cswap::add_current(None)?;
    let list = cswap::list()?;
    let now = list.accounts.iter().find(|a| a.active);
    let number = now.map(|a| a.number);
    let mut msg = match (&repair, number) {
        (Some((n, _)), _) => lf!(
            "Account {n} ({email}) is signed in again.",
            "{n} numaralı hesap ({email}) yeniden giriş yaptı."
        ),
        (None, Some(n)) => lf!(
            "Added {email} as account {n}.",
            "{email}, {n} numaralı hesap olarak eklendi."
        ),
        (None, None) => lf!("Added {email}.", "{email} eklendi."),
    };
    if let Some((prev, name)) = previous
        && number != Some(prev)
    {
        match cswap::switch(prev) {
            Ok(()) => msg.push_str(&lf!(
                " Claude Code is back on {name}.",
                " Claude Code yeniden {name} hesabında."
            )),
            Err(e) => msg.push_str(&lf!(
                " Couldn't switch back to {name}: {e}",
                " {name} hesabına geri geçilemedi: {e}"
            )),
        }
    }
    Ok(msg)
}

pub fn open(cx: &mut App, entry: Entry) {
    if let Some(h) = cx.global::<Globals>().setup_window {
        let reused = h.update(cx, |root, window, cx| {
            if !root.busy() {
                root.enter(entry, cx);
            }
            window.activate_window();
        });
        if reused.is_ok() {
            cx.activate(true);
            return;
        }
        cx.global_mut::<Globals>().setup_window = None;
    }
    crate::close_window(cx);
    let bounds = Bounds::centered(None, size(px(620.0), px(640.0)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(lf!("{APP_NAME} Setup", "{APP_NAME} Kurulumu").into()),
                appears_transparent: crate::TRANSPARENT_TITLEBAR,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            is_movable: true,
            is_resizable: false,
            is_minimizable: true,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| SetupRoot::new(entry, window, cx)),
    );
    if let Ok(h) = handle {
        cx.global_mut::<Globals>().setup_window = Some(h);
        cx.activate(true);
    }
}

// ---------- rendering ----------

fn title(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(20.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(c(t().text))
        .child(text.into())
}

fn lead(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .mt_1p5()
        .mb_2()
        .text_size(px(12.5))
        .line_height(px(18.0))
        .text_color(c(t().secondary))
        .child(text.into())
}

fn mono(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .font_family(MONO_FONT)
        .text_size(px(11.0))
        .text_color(c(t().text))
        .child(text.into())
}

fn status(ok: bool, label: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(11.5))
        .text_color(c(if ok { t().ok } else { t().secondary }))
        .when(ok, |d| d.child(icon("check", 13.0, c(t().ok))))
        .child(label.to_string())
}

fn message(text: &str, color: u32) -> impl IntoElement {
    div()
        .mt_3()
        .text_size(px(11.5))
        .line_height(px(16.0))
        .text_color(c(color))
        .child(text.to_string())
}

fn home_relative(p: &std::path::Path) -> String {
    let home = setup::home().display().to_string();
    let s = p.display().to_string();
    match s.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => s,
    }
}

impl SetupRoot {
    fn steps_bar(&self) -> impl IntoElement {
        let mut bar = div().flex().items_center().justify_center().gap_2();
        let current = Step::ALL.iter().position(|s| *s == self.step).unwrap_or(0);
        for (i, s) in Step::ALL.iter().enumerate() {
            if i > 0 {
                bar = bar.child(div().w(px(18.0)).h(px(1.0)).bg(c(t().hairline)));
            }
            let done = i < current;
            let here = i == current;
            bar = bar.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .size(px(16.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(c(if here || done { t().accent } else { t().hover }))
                            .text_color(c(if here || done {
                                0xffffff
                            } else {
                                t().secondary
                            }))
                            .child(if done {
                                icon("check", 10.0, c(0xffffff)).into_any_element()
                            } else {
                                SharedString::from(format!("{}", i + 1)).into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(c(if here { t().text } else { t().secondary }))
                            .when(here, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(s.label()),
                    ),
            );
        }
        bar
    }

    fn page_welcome(&self) -> gpui::AnyElement {
        let tools = self.tools.as_ref();
        let claude_row = match tools.and_then(|t| t.claude.as_ref()) {
            Some(p) => row(
                "Claude Code",
                Some(&home_relative(p)),
                status(true, l("Installed", "Kurulu")),
            ),
            None => row(
                "Claude Code",
                Some(l(
                    "Not found. Swapbar signs in to your accounts through Claude Code, so install it first.",
                    "Bulunamadı. Swapbar hesaplarına Claude Code üzerinden giriş yapar, önce onu kur.",
                )),
                button(
                    "copy-claude",
                    l("Copy install command", "Kurulum komutunu kopyala"),
                    false,
                    |cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            setup::CLAUDE_INSTALLER.to_string(),
                        ))
                    },
                ),
            ),
        };
        let cswap_row = match tools.and_then(|t| t.cswap.as_ref().map(|p| (p, t))) {
            Some((p, t)) => {
                let desc = match &t.cswap_version {
                    Some(v) => lf!("Version {v} · {}", "Sürüm {v} · {}", home_relative(p)),
                    None => home_relative(p),
                };
                row(
                    "claude-swap (cswap)",
                    Some(&desc),
                    status(true, l("Installed", "Kurulu")),
                )
            }
            None => row(
                "claude-swap (cswap)",
                Some(l(
                    "Keeps each account's login and fetches its quota. Swapbar can install it in the next step.",
                    "Her hesabın girişini saklar ve kotasını alır. Swapbar bir sonraki adımda kurabilir.",
                )),
                status(false, l("Not installed", "Kurulu değil")),
            ),
        };
        let checking = tools.is_none();
        div()
            .flex()
            .flex_col()
            .child(icon("logomark", 44.0, c(t().accent)))
            .child(div().mt_4().child(title(lf!("Welcome to {APP_NAME}", "{APP_NAME}'a hoş geldin"))))
            .child(lead(
                l("Swapbar shows the quota of every Claude account you use with claude-swap, warns you before a limit, and switches accounts in one click. First, a quick check of what's installed.", "Swapbar, claude-swap ile kullandığın her Claude hesabının kotasını gösterir, limit dolmadan uyarır ve tek tıkla hesap değiştirir. Önce nelerin kurulu olduğuna hızlıca bakalım."),
            ))
            .child(heading(l("Requirements", "Gereksinimler")))
            .child(if checking {
                group(vec![row(l("Checking…", "Kontrol ediliyor…"), None, div())]).into_any_element()
            } else {
                group(vec![claude_row, cswap_row]).into_any_element()
            })
            .into_any_element()
    }

    fn page_install(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let tools = self.tools.clone().unwrap_or_default();
        let has_cswap = tools.cswap.is_some();
        let plan = setup::install_plan(&tools);
        let lines: Vec<String> = {
            let log = self.log.lock().unwrap();
            log.iter().rev().take(LOG_LINES).rev().cloned().collect()
        };
        let mut col = div()
            .flex()
            .flex_col()
            .child(title(l("Install claude-swap", "claude-swap'ı kur")))
            .child(lead(
                l("claude-swap (cswap) is the command-line tool that stores each account's login and asks Anthropic for its quota. Swapbar reads everything through it.", "claude-swap (cswap), her hesabın girişini saklayan ve kotasını Anthropic'ten soran komut satırı aracı. Swapbar her şeyi onun üzerinden okur."),
            ));
        if has_cswap && self.job != Job::Installing {
            let v = tools.cswap_version.as_deref().unwrap_or("");
            col = col
                .child(heading(l("Installed", "Kurulu")))
                .child(group(vec![row(
                    &format!("claude-swap {v}"),
                    tools.cswap.as_ref().map(|p| home_relative(p)).as_deref(),
                    status(true, l("Ready", "Hazır")),
                )]));
        } else {
            let mut rows: Vec<gpui::AnyElement> = plan
                .iter()
                .map(|cmd| {
                    div()
                        .px_3()
                        .py_2()
                        .child(mono(format!("$ {cmd}")))
                        .into_any_element()
                })
                .collect();
            if tools.uv.is_none() {
                rows.push(row(
                    l("uv comes first", "Önce uv kurulur"),
                    Some(if tools.brew.is_some() {
                        l("claude-swap is installed with uv, Astral's Python tool manager. Homebrew installs it.", "claude-swap, Astral'ın Python araç yöneticisi uv ile kurulur. uv'yi Homebrew kurar.")
                    } else {
                        l("claude-swap is installed with uv, Astral's Python tool manager, using its official installer.", "claude-swap, Astral'ın Python araç yöneticisi uv ile kurulur. uv resmi kurulum betiğiyle kurulur.")
                    }),
                    div(),
                ));
            }
            col = col
                .child(heading(l(
                    "Swapbar will run",
                    "Swapbar şunları çalıştıracak",
                )))
                .child(group(rows));
        }
        if !lines.is_empty() {
            let mut log = div()
                .mt_3()
                .p_3()
                .rounded_lg()
                .bg(c(t().group))
                .border_1()
                .border_color(c(t().hairline))
                .flex()
                .flex_col()
                .gap(px(2.0));
            for l in lines {
                log = log.child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(px(10.0))
                        .text_color(c(t().secondary))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(l),
                );
            }
            col = col.child(log);
        }
        if self.installed && has_cswap {
            col = col.child(message(
                l("claude-swap is installed.", "claude-swap kuruldu."),
                t().ok,
            ));
        }
        let _ = cx;
        col.into_any_element()
    }

    fn page_accounts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let s = self.state.read(cx);
        let list = s.accounts.clone().unwrap_or_default();
        let repair = match self.entry {
            Entry::Relogin(n) => list.accounts.iter().find(|a| a.number == n).cloned(),
            _ => None,
        };
        let claude = self.tools.as_ref().is_some_and(|t| t.claude.is_some());
        let mut col = div().flex().flex_col();

        col = match &repair {
            Some(a) => {
                let why = if a.needs_login() || list.duplicate_slots().contains(&a.number) {
                    lf!("The login cswap saved for account {} doesn't work right now{}. ", "cswap'ın {} numaralı hesap için sakladığı giriş şu an çalışmıyor{}. ",
                        a.number,
                        a.status_label().filter(|_| a.needs_login()).map(|l| format!(" ({l})")).unwrap_or_default()
                    )
                } else {
                    String::new()
                };
                col.child(title(lf!("Sign in to {} again", "{} hesabına yeniden giriş yap", a.email))).child(lead(lf!("{why}Sign in again in your browser and Swapbar saves the new login. Claude Code switches back to the account it uses now when you're done.", "{why}Tarayıcında yeniden giriş yap, Swapbar yeni girişi kaydetsin. İşin bitince Claude Code şu an kullandığı hesaba geri döner."
                )))
            }
            None => col.child(title(l("Add your accounts", "Hesaplarını ekle"))).child(lead(
                l("Each account signs in once, in your browser. cswap keeps its login so Swapbar can switch to it later. While you sign in, Claude Code uses the new account; Swapbar switches it back afterwards.", "Her hesap tarayıcında bir kez giriş yapar. cswap girişi saklar, böylece Swapbar sonra o hesaba geçebilir. Giriş sırasında Claude Code yeni hesabı kullanır; Swapbar sonra geri döndürür."),
            )),
        };

        if repair.is_none() {
            let mut rows: Vec<gpui::AnyElement> = list
                .accounts
                .iter()
                .map(|a| {
                    let desc = format!(
                        "{}{}{}",
                        a.org_tag(),
                        a.plan
                            .as_deref()
                            .map(|p| format!(" · {p}"))
                            .unwrap_or_default(),
                        if a.active {
                            l(" · current", " · aktif")
                        } else {
                            ""
                        }
                    );
                    row(
                        &format!("{}  {}", a.number, a.name()),
                        Some(&desc),
                        status(true, l("Added", "Eklendi")),
                    )
                })
                .collect();
            if rows.is_empty() {
                rows.push(row(
                    l("No accounts yet", "Henüz hesap yok"),
                    Some(
                        l("Add the account Claude Code is signed in to, then sign in to the others.", "Önce Claude Code'da giriş yapılmış hesabı ekle, sonra diğerlerine giriş yap."),
                    ),
                    div(),
                ));
            }
            col = col
                .child(heading(l(
                    "Accounts in claude-swap",
                    "claude-swap'taki hesaplar",
                )))
                .child(group(rows));
        }

        let signed_in_email = self
            .signed_in
            .as_ref()
            .filter(|s| s.logged_in)
            .and_then(|s| s.email.clone());
        let unmanaged = signed_in_email.clone().filter(|e| {
            !list
                .accounts
                .iter()
                .any(|a| a.email.eq_ignore_ascii_case(e))
        });

        let mut actions: Vec<gpui::AnyElement> = Vec::new();
        if !claude {
            actions.push(row(
                l("Claude Code isn't installed", "Claude Code kurulu değil"),
                Some(l(
                    "Accounts sign in through Claude Code. Install it, then check again.",
                    "Hesaplar Claude Code üzerinden giriş yapar. Kur, sonra tekrar kontrol et.",
                )),
                button(
                    "recheck",
                    l("Check again", "Tekrar kontrol et"),
                    false,
                    self.listener(cx, |this, cx| this.recheck(cx)),
                ),
            ));
        } else if repair.is_none()
            && let Some(email) = unmanaged
        {
            actions.push(row(
                &lf!(
                    "Claude Code is signed in as {email}",
                    "Claude Code'da {email} ile giriş yapılmış"
                ),
                Some(l(
                    "Add it without signing in again.",
                    "Yeniden giriş yapmadan ekle.",
                )),
                primary_button(
                    "add-current",
                    l("Add", "Ekle"),
                    !self.busy(),
                    self.listener(cx, |this, cx| this.add_signed_in(cx)),
                ),
            ));
        }
        if claude {
            actions.push(match self.job {
                Job::SigningIn => {
                    let url = self
                        .login
                        .as_ref()
                        .and_then(|l| l.url.lock().unwrap().clone());
                    row(
                        l("Waiting for you to sign in…", "Giriş yapman bekleniyor…"),
                        Some(match &repair {
                            Some(_) => l("Finish in the browser window that opened.", "Açılan tarayıcı penceresinde tamamla."),
                            None => l("Finish in the browser window that opened, and pick the account you want to add.", "Açılan tarayıcı penceresinde tamamla ve eklemek istediğin hesabı seç."),
                        }),
                        div()
                            .flex()
                            .gap_2()
                            .when_some(url, |d, url| {
                                d.child(button("open-again", l("Open page", "Sayfayı aç"), false, move |cx| cx.open_url(&url)))
                            })
                            .when(self.login.is_some(), |d| {
                                d.child(button("paste-code", l("Paste code", "Kodu yapıştır"), false, self.listener(cx, |this, cx| this.paste_code(cx))))
                            })
                            .child(button("cancel-sign-in", l("Cancel", "İptal"), false, self.listener(cx, |this, cx| this.cancel_sign_in(cx)))),
                    )
                }
                Job::Saving => row(l("Saving the login…", "Giriş kaydediliyor…"), None, div()),
                _ => match &repair {
                    Some(a) => row(
                        &lf!("Sign in as {}", "{} ile giriş yap", a.email),
                        Some(l("Opens claude.ai in your browser.", "Tarayıcında claude.ai'yi açar.")),
                        primary_button("sign-in", l("Sign in…", "Giriş yap…"), !self.busy(), self.listener(cx, |this, cx| this.sign_in(cx))),
                    ),
                    None => row(
                        if list.accounts.is_empty() && signed_in_email.is_none() {
                            l("Sign in to your first account", "İlk hesabına giriş yap")
                        } else {
                            l("Add another account", "Başka bir hesap ekle")
                        },
                        Some(l("Opens claude.ai in your browser.", "Tarayıcında claude.ai'yi açar.")),
                        button("sign-in", l("Sign in…", "Giriş yap…"), false, self.listener(cx, |this, cx| this.sign_in(cx))),
                    ),
                },
            });
        }
        col = col
            .child(heading(if repair.is_some() {
                l("Sign in", "Giriş")
            } else {
                l("Add an account", "Hesap ekle")
            }))
            .child(group(actions));

        if let Some(n) = &self.notice {
            col = col.child(message(n, t().ok));
        }
        if let Some(h) = &self.hint {
            col = col.child(message(h, t().secondary));
        }
        if let Some(e) = &self.error {
            col = col.child(message(e, t().danger));
        }
        col.into_any_element()
    }

    fn page_finish(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let p = self.state.read(cx).prefs.clone();
        let notify_on = p.notify_low || p.notify_pace || p.notify_reset;
        let launch = crate::settings::launch_agent_installed();
        div()
            .flex()
            .flex_col()
            .child(title(l("You're all set", "Her şey hazır")))
            .child(lead(if cfg!(target_os = "windows") {
                lf!("Swapbar lives in the system tray, next to the clock. If you don't see it, click ^ there and drag it onto the taskbar. Click it, or press {} from any app. A few things you may want on:", "Swapbar, saatin yanındaki sistem tepsisinde duruyor. Görmüyorsan oradaki ^ simgesine tıklayıp onu görev çubuğuna sürükle. Tıkla ya da herhangi bir uygulamadan {} tuşlarına bas. Açmak isteyebileceğin birkaç şey:",
                    hotkey::TOGGLE_LABEL
                )
            } else {
                lf!("Swapbar lives in your menu bar. Click it, or press {} from any app. A few things you may want on:", "Swapbar menü çubuğunda duruyor. Tıkla ya da herhangi bir uygulamadan {} tuşlarına bas. Açmak isteyebileceğin birkaç şey:",
                    hotkey::TOGGLE_LABEL
                )
            }))
            .child(heading(l("Preferences", "Tercihler")))
            .child(group(vec![
                row(
                    l("Launch at login", "Girişte başlat"),
                    Some(os(
                        l("Start Swapbar when you sign in to your Mac.", "Mac'e giriş yaptığında Swapbar'ı başlat."),
                        l("Start Swapbar when you sign in to Windows.", "Windows'a giriş yaptığında Swapbar'ı başlat."),
                    )),
                    toggle("setup-launch", launch, |on, cx| {
                        if let Err(e) = crate::settings::set_launch_at_login(on) {
                            eprintln!("launch agent: {e}");
                        }
                        crate::update_prefs(cx, |p| p.launch_at_login = on);
                    }),
                ),
                row(
                    l("Quota notifications", "Kota bildirimleri"),
                    Some(l("Before the current account runs out, and when a drained account resets.", "Aktif hesap bitmeden önce ve tükenen bir hesap sıfırlanınca.")),
                    toggle("setup-notify", notify_on, |on, cx| {
                        crate::update_prefs(cx, |p| {
                            p.notify_low = on;
                            p.notify_pace = on;
                            p.notify_reset = on;
                        });
                        if on {
                            notify::request_authorization();
                        }
                    }),
                ),
                row(
                    l("Global shortcuts", "Global kısayollar"),
                    Some(l("Open Swapbar, or switch to the account with the most quota left.", "Swapbar'ı aç ya da en çok kotası kalan hesaba geç.")),
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(shortcut(hotkey::TOGGLE_LABEL, p.hotkeys))
                        .child(shortcut(hotkey::SWITCH_BEST_LABEL, p.hotkeys))
                        .child(toggle("setup-hotkeys", p.hotkeys, |on, cx| crate::update_prefs(cx, |p| p.hotkeys = on))),
                ),
            ]))
            .into_any_element()
    }

    /// A click handler for the controls, which take `Fn(&mut App)`.
    fn listener(
        &self,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut App) + 'static {
        let this = cx.entity().downgrade();
        move |cx: &mut App| {
            let _ = this.update(cx, |this, cx| f(this, cx));
        }
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_cswap = self.tools.as_ref().is_some_and(|t| t.cswap.is_some());
        let busy = self.busy();
        let (label, enabled): (&str, bool) = match self.step {
            Step::Welcome => (l("Continue", "Devam"), self.tools.is_some()),
            Step::Install if !has_cswap => ("", false),
            Step::Install => (l("Continue", "Devam"), !busy),
            Step::Accounts if self.walkthrough() => (l("Continue", "Devam"), !busy),
            Step::Accounts => (l("Done", "Bitti"), !busy),
            Step::Finish => (l("Open Swapbar", "Swapbar'ı aç"), true),
        };
        let this = cx.entity().downgrade();
        let next = div()
            .id("next")
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
                    .on_click(move |_, window, cx| {
                        let _ = this.update(cx, |this, cx| this.next(window, cx));
                    })
            })
            .child(label.to_string());
        let install = (self.step == Step::Install && !has_cswap).then(|| {
            primary_button(
                "install",
                if self.job == Job::Installing {
                    l("Installing…", "Kuruluyor…")
                } else {
                    l("Install claude-swap", "claude-swap'ı kur")
                },
                !busy,
                self.listener(cx, |this, cx| this.install(cx)),
            )
        });
        let show_back = self.walkthrough() && self.step != Step::Welcome && !busy;
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(56.0))
            .px_6()
            .border_t_1()
            .border_color(c(t().hairline))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .when(show_back, |d| {
                        d.child(button(
                            "back",
                            l("Back", "Geri"),
                            false,
                            self.listener(cx, |this, cx| {
                                if !this.busy() {
                                    this.back(cx)
                                }
                            }),
                        ))
                    })
                    .when(self.step == Step::Welcome, |d| {
                        d.child(button(
                            "check-again",
                            l("Check again", "Tekrar kontrol et"),
                            false,
                            self.listener(cx, |this, cx| this.recheck(cx)),
                        ))
                    }),
            )
            .child(match install {
                Some(b) => b.into_any_element(),
                None => next.into_any_element(),
            })
    }
}

impl Render for SetupRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync(cx);
        let body = match self.step {
            Step::Welcome => self.page_welcome(),
            Step::Install => self.page_install(cx),
            Step::Accounts => self.page_accounts(cx),
            Step::Finish => self.page_finish(cx),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(c(t().content))
            .font_family(UI_FONT)
            .text_color(c(t().text))
            .child(
                div()
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.walkthrough(), |d| d.child(self.steps_bar())),
            )
            .child(
                div()
                    .id("setup-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(40.0))
                    .pt_4()
                    .pb_6()
                    .child(body),
            )
            .child(self.footer(cx))
    }
}
