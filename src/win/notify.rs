//! Notifications as tray balloons, which Windows 10 and 11 show as regular notifications
//! from the tray icon. They need no app registration, but carry no buttons: a click opens
//! the popover, where the switch suggestion offers the same switch the macOS button does.

use std::cell::RefCell;

/// What the user did with a notification.
pub enum Action {
    /// Clicked the notification itself.
    Open,
    /// Pressed "Switch" on a notification about this account (macOS only for now).
    #[allow(dead_code)]
    Switch(u32),
}

type Callback = Box<dyn Fn(Action)>;

thread_local! {
    static ON_ACTION: RefCell<Option<Callback>> = RefCell::new(None);
}

pub fn available() -> bool {
    true
}

pub fn init(on_action: Box<dyn Fn(Action)>) {
    ON_ACTION.with(|cb| *cb.borrow_mut() = Some(on_action));
}

/// Windows asks no permission for tray balloons.
pub fn request_authorization() {}

/// Show a notification. A newer balloon replaces the one on screen, so `ident` and the
/// account to switch to are only used on macOS.
pub fn post(ident: &str, title: &str, body: &str, _switch_to: Option<u32>) {
    crate::debug_log(format_args!("notify {ident}: {title} / {body}"));
    crate::statusbar::balloon(title, body);
}

pub(super) fn on_balloon_click() {
    ON_ACTION.with(|cb| {
        if let Some(f) = cb.borrow().as_ref() {
            f(Action::Open);
        }
    });
}

/// Open Settings › System › Notifications.
pub fn open_system_settings() {
    crate::win::open_url("ms-settings:notifications");
}
