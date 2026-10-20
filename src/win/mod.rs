//! Windows plumbing shared by the tray icon, the shortcuts, the notifications and the
//! dialogs: one hidden window on the main thread receives their messages. gpui's message
//! loop (`GetMessageW` + `DispatchMessageW` for the whole thread) delivers them to it.
//! The handlers only queue work for gpui, so they never re-enter an update.

use std::cell::Cell;

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowKind,
    WindowOptions, div, point, px, size,
};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowW, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SW_SHOWDEFAULT, WINDOW_EX_STYLE, WM_APP, WM_HOTKEY, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Messages for the hidden window.
pub const WM_TRAY: u32 = WM_APP + 1;
/// A boxed job to run on the main thread, outside any gpui update.
const WM_JOB: u32 = WM_APP + 2;
/// Another launch of Swapbar asks this one to show itself.
const WM_SHOW: u32 = WM_APP + 3;

const CLASS: PCWSTR = w!("SwapbarHiddenWindow");

thread_local! {
    static HWND_: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// NUL-terminated UTF-16 for Win32.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy `s` into a fixed UTF-16 buffer, truncated and NUL-terminated.
pub fn fill(buf: &mut [u16], s: &str) {
    let room = buf.len() - 1;
    let mut n = 0;
    for (slot, ch) in buf.iter_mut().zip(s.encode_utf16()).take(room) {
        *slot = ch;
        n += 1;
    }
    buf[n] = 0;
}

/// The hidden window, created on first use. A real (never shown) top-level window rather
/// than a message-only one: the tray's context menu needs a window it can bring forward.
pub fn hwnd() -> HWND {
    let existing = HWND_.with(Cell::get);
    if !existing.is_invalid() {
        return existing;
    }
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
        TASKBAR_CREATED.with(|m| m.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            CLASS,
            w!("Swapbar"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        HWND_.with(|h| h.set(hwnd));
        hwnd
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => crate::statusbar::on_tray_message(wparam, lparam),
        WM_HOTKEY => crate::hotkey::on_hotkey(wparam.0 as i32),
        WM_JOB => {
            let job = unsafe { Box::from_raw(lparam.0 as *mut Job) };
            job();
        }
        WM_SHOW => crate::statusbar::on_show_request(),
        _ if msg != 0 && msg == TASKBAR_CREATED.with(Cell::get) => {
            // Explorer restarted and lost every tray icon.
            crate::statusbar::on_taskbar_created();
        }
        _ => {
            crate::statusbar::on_other_message(msg, wparam, lparam);
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
    }
    LRESULT(0)
}

type Job = Box<dyn FnOnce() + Send>;

/// Run `f` on the main thread on a later turn of the message loop.
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    let job: Box<Job> = Box::new(Box::new(f));
    let ptr = Box::into_raw(job);
    let posted = unsafe { PostMessageW(Some(hwnd()), WM_JOB, WPARAM(0), LPARAM(ptr as isize)) };
    if posted.is_err() {
        // never delivered: take the job back so it is dropped, not leaked
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// Print to the terminal that started us (`swapbar --scan`); release builds have no
/// console of their own.
pub fn attach_console() {
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

/// Open a URL or `ms-settings:` page with its default handler.
pub fn open_url(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWDEFAULT,
        );
    }
}

/// Keep one Swapbar per session. A second launch asks the running one to open its popover
/// and returns `false`.
pub fn claim_single_instance() -> bool {
    unsafe {
        // the handle stays open for the life of the process; that is the lock
        let _ = CreateMutexW(None, true, w!("Local\\SwapbarSingleInstance"));
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        if let Ok(other) = FindWindowW(CLASS, None) {
            let _ = PostMessageW(Some(other), WM_SHOW, WPARAM(0), LPARAM(0));
        }
        false
    }
}

struct KeepAlive;

impl Render for KeepAlive {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// gpui on Windows quits when its last window closes, and the popover closes all the
/// time. A window that is never shown keeps the app running in the tray.
pub fn keep_alive(cx: &mut App) {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: size(px(1.0), px(1.0)),
    };
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |_, cx| cx.new(|_| KeepAlive),
    );
    if let Err(e) = opened {
        eprintln!("keep-alive window failed: {e}");
    }
}
