//! System-wide shortcuts through RegisterHotKey. WM_HOTKEY arrives at the hidden window
//! on the main thread and calls a thread-local callback set from the gpui App.

use std::cell::{Cell, RefCell};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
    UnregisterHotKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    /// Ctrl+Alt+S: open or close the popover.
    Toggle = 1,
    /// Ctrl+Alt+Shift+S: switch to the account with the most quota left.
    SwitchBest = 2,
}

pub const TOGGLE_LABEL: &str = "Ctrl+Alt+S";
pub const SWITCH_BEST_LABEL: &str = "Ctrl+Alt+Shift+S";

const KEY_S: u32 = b'S' as u32;

type Callback = Box<dyn Fn(Hotkey)>;

thread_local! {
    static ON_HOTKEY: RefCell<Option<Callback>> = RefCell::new(None);
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

pub fn set_handler(f: Box<dyn Fn(Hotkey)>) {
    ON_HOTKEY.with(|cb| *cb.borrow_mut() = Some(f));
}

pub(super) fn on_hotkey(id: i32) {
    let key = match id {
        1 => Hotkey::Toggle,
        2 => Hotkey::SwitchBest,
        _ => return,
    };
    crate::debug_log(format_args!("hotkey {key:?}"));
    ON_HOTKEY.with(|cb| {
        if let Some(f) = cb.borrow().as_ref() {
            f(key);
        }
    });
}

/// Register or unregister both shortcuts. Safe to call repeatedly.
pub fn set_enabled(on: bool) {
    if on == REGISTERED.with(Cell::get) {
        return;
    }
    REGISTERED.with(|r| r.set(on));
    let hwnd = crate::win::hwnd();
    let keys: [(HOT_KEY_MODIFIERS, Hotkey); 2] = [
        (MOD_CONTROL | MOD_ALT, Hotkey::Toggle),
        (MOD_CONTROL | MOD_ALT | MOD_SHIFT, Hotkey::SwitchBest),
    ];
    for (mods, id) in keys {
        unsafe {
            if on {
                if let Err(e) = RegisterHotKey(Some(hwnd), id as i32, mods | MOD_NOREPEAT, KEY_S) {
                    // another app owns the combination
                    eprintln!("hotkey {id:?} not registered: {e}");
                }
            } else {
                let _ = UnregisterHotKey(Some(hwnd), id as i32);
            }
        }
    }
}
