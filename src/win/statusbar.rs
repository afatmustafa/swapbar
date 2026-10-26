//! The tray icon (Shell_NotifyIcon), the Windows counterpart of the macOS status item.
//! The icon is Swapbar's mark, or the active account's 5h percentage when the title is on:
//! a tray icon has no room for text beside it, so the number goes in the icon and the full
//! reading in the tooltip. Left click toggles the popover; right click opens a small menu.

use std::cell::RefCell;

use gpui::{Bounds, Pixels, point, px, size};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, CreateBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FF_SWISS, FW_BOLD,
    GdiFlush, GetMonitorInfoW, GetTextExtentPoint32W, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Shell::{
    ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA, NIF_ICON, NIF_INFO, NIF_MESSAGE,
    NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, SHAppBarMessage, Shell_NotifyIconGetRect,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu,
    GA_ROOT, GetAncestor, GetCursorPos, GetWindowThreadProcessId, HHOOK, HICON, HWND_TOPMOST,
    ICONINFO, MF_SEPARATOR, MF_STRING, MSLLHOOKSTRUCT, PostMessageW, SM_CXICON, SM_CXSMICON,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetMenuDefaultItem, SetWindowPos,
    SetWindowsHookExW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NULL, WM_RBUTTONDOWN, WM_SETTINGCHANGE, WindowFromPoint,
};
use windows::core::{PCWSTR, w};

use crate::i18n::l;
use crate::win::{WM_TRAY, fill, hwnd, wide};

const LOGO: &[u8] = include_bytes!("../../assets/logomark.svg");
const ICON_ID: u32 = 1;
/// Enter or space on the focused icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
/// Gap between the popover and the taskbar, like the system flyouts.
const MARGIN: f64 = 12.0;

/// What the tray's context menu asked for.
#[derive(Debug, Clone, Copy)]
pub enum MenuAction {
    Open,
    Settings,
    Quit,
}

#[derive(Default)]
struct Tray {
    added: bool,
    /// The number shown in place of the mark, if any.
    number: Option<String>,
    tooltip: String,
    icon: Option<HICON>,
    balloon_icon: Option<HICON>,
}

type MenuCallback = Box<dyn Fn(MenuAction)>;

thread_local! {
    static ON_CLICK: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
    static ON_MENU: RefCell<Option<MenuCallback>> = RefCell::new(None);
    static ON_OUTSIDE_CLICK: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
    /// The installed mouse hook, with the tray icon's rectangle at the time.
    static HOOK: RefCell<Option<(HHOOK, Option<RECT>)>> = const { RefCell::new(None) };
    static TRAY: RefCell<Tray> = RefCell::new(Tray::default());
}

pub struct StatusItem {
    _private: (),
}

impl StatusItem {
    pub fn new(title: &str, on_click: Box<dyn Fn()>) -> StatusItem {
        ON_CLICK.with(|cb| *cb.borrow_mut() = Some(on_click));
        TRAY.with(|t| t.borrow_mut().tooltip = crate::APP_NAME.to_string());
        let si = StatusItem { _private: () };
        si.set_title(title);
        si
    }

    /// The macOS title (" 73%" or " 73%·40%"): its first percentage goes in the icon.
    pub fn set_title(&self, title: &str) {
        let number = first_number(title);
        let changed = TRAY.with(|t| {
            let mut t = t.borrow_mut();
            let changed = t.number != number || t.icon.is_none();
            t.number = number;
            changed
        });
        if changed {
            refresh_icon();
        }
    }

    pub fn set_tooltip(&self, text: &str) {
        let changed = TRAY.with(|t| {
            let mut t = t.borrow_mut();
            let changed = t.tooltip != text;
            t.tooltip = text.to_string();
            changed
        });
        if changed {
            update(false);
        }
    }

    /// The tray has no pressed state to show while the popover is open.
    pub fn set_highlighted(&self, _on: bool) {}

    /// Where the popover goes: next to the icon on the taskbar's side of the screen, in
    /// gpui's coordinates (physical pixels divided by that monitor's scale).
    pub fn popover_bounds(&self, width: f32, max_height: f32) -> Bounds<Pixels> {
        let anchor = icon_rect()
            .map(|r| POINT {
                x: (r.left + r.right) / 2,
                y: (r.top + r.bottom) / 2,
            })
            .unwrap_or_else(cursor);
        let monitor = unsafe { MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = dpi_x.max(1) as f64 / 96.0;
        let work = info.rcWork;
        let (left, top, right, bottom) = (
            work.left as f64,
            work.top as f64,
            work.right as f64,
            work.bottom as f64,
        );
        let (m, w) = (MARGIN * scale, width as f64 * scale);
        let h = (max_height as f64 * scale).min(bottom - top - 2.0 * m);
        let clamp_x = |x: f64| x.clamp(left + m, (right - w - m).max(left + m));
        let clamp_y = |y: f64| y.clamp(top + m, (bottom - h - m).max(top + m));
        let (ax, ay) = (anchor.x as f64, anchor.y as f64);
        let (x, y) = match taskbar_edge() {
            ABE_TOP => (clamp_x(ax - w / 2.0), top + m),
            ABE_LEFT => (left + m, clamp_y(ay - h / 2.0)),
            ABE_RIGHT => (right - w - m, clamp_y(ay - h / 2.0)),
            _ => (clamp_x(ax - w / 2.0), bottom - h - m),
        };
        Bounds {
            origin: point(px((x / scale) as f32), px((y / scale) as f32)),
            size: size(px(width), px((h / scale) as f32)),
        }
    }
}

/// The context menu's handler; a second launch of Swapbar also arrives here as `Open`.
pub fn set_menu_handler(f: Box<dyn Fn(MenuAction)>) {
    ON_MENU.with(|cb| *cb.borrow_mut() = Some(f));
}

/// Called for mouse presses outside Swapbar's windows while [`watch_outside_clicks`] is on.
pub fn set_outside_click_handler(f: Box<dyn Fn()>) {
    ON_OUTSIDE_CLICK.with(|cb| *cb.borrow_mut() = Some(f));
}

/// Install or remove a low-level mouse hook that reports presses in other apps. Windows
/// only lets the popover take focus when it allows Swapbar to come forward; when it
/// doesn't, no deactivation ever arrives, and this is what closes the popover.
pub fn watch_outside_clicks(on: bool) {
    if on == HOOK.with(|h| h.borrow().is_some()) {
        return;
    }
    if !on {
        if let Some((hook, _)) = HOOK.with(|h| h.borrow_mut().take()) {
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
        return;
    }
    // The icon's place, looked up now rather than inside the hook, which has to be quick.
    // Never with HOOK borrowed: the lookup waits on Explorer, and hook calls can arrive
    // while it does.
    let icon = icon_rect();
    let instance = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let hook =
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(instance.into()), 0) };
    if let Ok(hook) = hook {
        HOOK.with(|h| *h.borrow_mut() = Some((hook, icon)));
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            wparam.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        )
    {
        let p = unsafe { (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt };
        let on_icon = HOOK.with(|h| {
            h.borrow()
                .and_then(|(_, r)| r)
                .is_some_and(|r| inside(&r, p))
        });
        if !on_icon && !over_own_window(p) {
            ON_OUTSIDE_CLICK.with(|cb| {
                if let Some(f) = cb.borrow().as_ref() {
                    f();
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Whether `p` is over one of Swapbar's windows (the popover, a dialog, the tray menu).
fn over_own_window(p: POINT) -> bool {
    unsafe {
        let hwnd = WindowFromPoint(p);
        let mut pid = 0;
        GetWindowThreadProcessId(GetAncestor(hwnd, GA_ROOT), Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

/// Keep the popover above other windows and give it focus when Windows allows, like the
/// system's own tray flyouts.
pub fn raise_popover(window: &gpui::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // gpui's own `window_handle()` is its window id; the trait gives the HWND
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Whether the mouse is over the tray icon. Pressing the icon takes focus from the
/// popover before the click arrives; the click handles that press instead.
pub fn mouse_over_button() -> bool {
    let p = cursor();
    icon_rect().is_some_and(|r| inside(&r, p))
}

fn inside(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

/// Show a balloon from the tray icon (Windows 10+ shows it as a notification).
pub fn balloon(title: &str, body: &str) {
    let mut nid = data();
    nid.uFlags = NIF_INFO;
    fill(&mut nid.szInfoTitle, title);
    fill(&mut nid.szInfo, body);
    nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
    nid.hBalloonIcon = TRAY.with(|t| t.borrow().balloon_icon).unwrap_or_default();
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

/// Take the icon out of the tray at quit; otherwise it lingers until hovered.
pub fn remove() {
    let added = TRAY.with(|t| std::mem::take(&mut t.borrow_mut().added));
    if added {
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data()) };
    }
}

pub(super) fn on_tray_message(_wparam: WPARAM, lparam: LPARAM) {
    // NOTIFYICON_VERSION_4: the event is in the low word, the anchor point in wparam
    let event = (lparam.0 as u32) & 0xffff;
    match event {
        NIN_SELECT | NIN_KEYSELECT => {
            crate::debug_log(format_args!("tray icon clicked"));
            ON_CLICK.with(|cb| {
                if let Some(f) = cb.borrow().as_ref() {
                    f();
                }
            });
        }
        WM_CONTEXTMENU => show_menu(),
        NIN_BALLOONUSERCLICK => crate::notify::on_balloon_click(),
        _ => {}
    }
}

pub(super) fn on_show_request() {
    menu_action(MenuAction::Open);
}

pub(super) fn on_taskbar_created() {
    TRAY.with(|t| t.borrow_mut().added = false);
    update(true);
}

/// Theme, scale or display changes: draw the icon again for the new taskbar.
pub(super) fn on_other_message(msg: u32, _wparam: WPARAM, _lparam: LPARAM) {
    if matches!(msg, WM_SETTINGCHANGE | WM_DISPLAYCHANGE | WM_DPICHANGED)
        && TRAY.with(|t| t.borrow().added)
    {
        refresh_icon();
    }
}

fn menu_action(action: MenuAction) {
    ON_MENU.with(|cb| {
        if let Some(f) = cb.borrow().as_ref() {
            f(action);
        }
    });
}

fn show_menu() {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let items = [
            (1, l("Open Swapbar", "Swapbar'ı aç")),
            (2, l("Settings…", "Ayarlar…")),
            (0, ""),
            (3, l("Quit Swapbar", "Swapbar'dan çık")),
        ];
        for (id, label) in items {
            if id == 0 {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            } else {
                let text = wide(label);
                let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(text.as_ptr()));
            }
        }
        let _ = SetMenuDefaultItem(menu, 1, 0);
        let p = cursor();
        // the menu closes on an outside click only when its owner is in front
        let _ = SetForegroundWindow(hwnd());
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON).0,
            p.x,
            p.y,
            hwnd(),
            None,
        );
        let _ = PostMessageW(Some(hwnd()), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match chosen.0 {
            1 => menu_action(MenuAction::Open),
            2 => menu_action(MenuAction::Settings),
            3 => menu_action(MenuAction::Quit),
            _ => {}
        }
    }
}

fn data() -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd(),
        uID: ICON_ID,
        ..Default::default()
    }
}

/// Add the icon if it isn't in the tray yet, otherwise update its image and tooltip.
fn update(add: bool) {
    let (added, icon, tooltip) = TRAY.with(|t| {
        let t = t.borrow();
        (t.added, t.icon, t.tooltip.clone())
    });
    let mut nid = data();
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon.unwrap_or_default();
    fill(&mut nid.szTip, &tooltip);
    unsafe {
        if added {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        } else if add && Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
            TRAY.with(|t| t.borrow_mut().added = true);
        }
    }
}

/// Draw the icon for the current number, taskbar theme and scale, then show it.
fn refresh_icon() {
    let dpi = match unsafe { GetDpiForWindow(hwnd()) } {
        0 => 96,
        d => d,
    };
    let small = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.max(16);
    let large = unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) }.max(32);
    let number = TRAY.with(|t| t.borrow().number.clone());
    let pixels = match &number {
        Some(n) => number_pixels(n, small, taskbar_is_light()),
        None => logo_pixels(small),
    };
    let icon = make_icon(small, &pixels);
    let balloon = make_icon(large, &logo_pixels(large));
    TRAY.with(|t| {
        let mut t = t.borrow_mut();
        for old in [
            std::mem::replace(&mut t.icon, icon),
            std::mem::replace(&mut t.balloon_icon, balloon),
        ]
        .into_iter()
        .flatten()
        {
            let _ = unsafe { DestroyIcon(old) };
        }
    });
    update(true);
}

/// " 73%·40%" → "73".
fn first_number(title: &str) -> Option<String> {
    let digits: String = title
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}

fn icon_rect() -> Option<RECT> {
    let id = NOTIFYICONIDENTIFIER {
        cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: hwnd(),
        uID: ICON_ID,
        ..Default::default()
    };
    unsafe { Shell_NotifyIconGetRect(&id) }.ok()
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    p
}

fn taskbar_edge() -> u32 {
    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) } == 0 {
        return u32::MAX;
    }
    abd.uEdge
}

/// The taskbar's own theme, which can differ from the apps' light/dark setting.
fn taskbar_is_light() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|v| v != 0)
}

/// Straight-alpha RGBA, row by row.
type Pixels32 = Vec<[u8; 4]>;

fn logo_pixels(px: i32) -> Pixels32 {
    let px = px as u32;
    let blank = vec![[0; 4]; (px * px) as usize];
    let Ok(tree) = resvg::usvg::Tree::from_data(LOGO, &resvg::usvg::Options::default()) else {
        return blank;
    };
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(px, px) else {
        return blank;
    };
    let s = tree.size();
    let scale = px as f32 / s.width().max(s.height());
    let (dx, dy) = (
        (px as f32 - s.width() * scale) / 2.0,
        (px as f32 - s.height() * scale) / 2.0,
    );
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
        &mut pixmap.as_mut(),
    );
    pixmap
        .pixels()
        .iter()
        .map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

/// White (dark taskbar) or black digits, as large as fit. GDI draws white on black
/// into a scratch bitmap, and that brightness becomes the alpha.
fn number_pixels(text: &str, px: i32, light_taskbar: bool) -> Pixels32 {
    let color = if light_taskbar {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    let mut out = vec![[0; 4]; (px * px) as usize];
    let mut text: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(
            Some(dc),
            &bitmap_info(px),
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        ) else {
            let _ = DeleteDC(dc);
            return out;
        };
        let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00ff_ffff));
        // shrink until the digits fit the width
        let mut height = px;
        let (font, old_font) = loop {
            let font = CreateFontW(
                -height,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                // grayscale smoothing; ClearType's coloured fringes would break the alpha
                ANTIALIASED_QUALITY,
                (VARIABLE_PITCH.0 | FF_SWISS.0) as u32,
                w!("Segoe UI"),
            );
            let previous = SelectObject(dc, HGDIOBJ(font.0));
            let mut extent = Default::default();
            let _ = GetTextExtentPoint32W(dc, &text, &mut extent);
            if extent.cx <= px || height <= 6 {
                break (font, previous);
            }
            SelectObject(dc, previous);
            let _ = DeleteObject(HGDIOBJ(font.0));
            height -= 1;
        };
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: px,
            bottom: px,
        };
        DrawTextW(
            dc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let _ = GdiFlush();
        let src = std::slice::from_raw_parts(bits as *const [u8; 4], (px * px) as usize);
        for (o, s) in out.iter_mut().zip(src) {
            let coverage = s[0].max(s[1]).max(s[2]);
            *o = [color[0], color[1], color[2], coverage];
        }
        SelectObject(dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
    }
    out
}

fn bitmap_info(px: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: px,
            // negative: rows top to bottom
            biHeight: -px,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A 32-bit icon from straight-alpha RGBA.
fn make_icon(px: i32, rgba: &[[u8; 4]]) -> Option<HICON> {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(
            Some(dc),
            &bitmap_info(px),
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        );
        let _ = DeleteDC(dc);
        let color = color.ok()?;
        let dst = std::slice::from_raw_parts_mut(bits as *mut [u8; 4], (px * px) as usize);
        for (d, [r, g, b, a]) in dst.iter_mut().zip(rgba) {
            *d = [*b, *g, *r, *a];
        }
        // the alpha channel decides; the mask only has to exist
        let mask = CreateBitmap(px, px, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_takes_the_first_percentage() {
        assert_eq!(first_number(" 73%").as_deref(), Some("73"));
        assert_eq!(first_number(" 100%·40%").as_deref(), Some("100"));
        assert_eq!(first_number(""), None);
        assert_eq!(first_number(" …"), None);
    }
}
