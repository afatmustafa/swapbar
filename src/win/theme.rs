//! Light/dark appearance. Views read the resolved choice through `is_dark()`; the native
//! title bars of the settings and setup windows are switched to match, since Windows draws
//! them from the system setting otherwise.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Globals;
use crate::settings::Theme;
use gpui::{App, WindowAppearance};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW};
use windows::core::BOOL;

static DARK: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Title bar mode last set per window, and the system appearance it was set under
    /// (gpui resets the title bars itself when the system appearance changes).
    static APPLIED: RefCell<(Option<bool>, HashMap<isize, bool>)> = RefCell::new((None, HashMap::new()));
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// Call at the top of each root view's `render`. Resolved from the preference rather than
/// `Window::appearance()`, which gpui caches.
pub fn sync(cx: &App) {
    let theme = cx.global::<Globals>().state.read(cx).prefs.theme;
    let system_dark = matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let dark = match theme {
        Theme::Light => false,
        Theme::Dark => true,
        Theme::System => system_dark,
    };
    DARK.store(dark, Ordering::Relaxed);
    let current = gpui_windows();
    let stale: Vec<HWND> = APPLIED.with(|a| {
        let (system, windows) = &mut *a.borrow_mut();
        if *system != Some(system_dark) {
            *system = Some(system_dark);
            windows.clear();
        }
        // forget closed windows, whose handles Windows may hand out again
        windows.retain(|h, _| current.iter().any(|w| w.0 as isize == *h));
        current
            .iter()
            .copied()
            .filter(|w| windows.insert(w.0 as isize, dark) != Some(dark))
            .collect()
    });
    // outside the borrow: changing the frame can send the window messages right away
    let value = BOOL::from(dark);
    for hwnd in stale {
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&value as *const BOOL).cast(),
                std::mem::size_of::<BOOL>() as u32,
            )
        };
    }
}

/// The next render picks the theme up through `sync`.
pub fn apply(_theme: Theme) {}

/// gpui's windows on this thread. The Win32 dialogs and menus stay light whatever the
/// theme, so their title bars are left alone.
fn gpui_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        let list = unsafe { &mut *(list.0 as *mut Vec<HWND>) };
        let mut class = [0u16; 32];
        let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if String::from_utf16_lossy(&class[..n]) == "Zed::Window" {
            list.push(hwnd);
        }
        true.into()
    }
    let mut list: Vec<HWND> = Vec::new();
    let _ = unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect),
            LPARAM(&mut list as *mut Vec<HWND> as isize),
        )
    };
    list
}
