//! Native prompts: confirmations through a task dialog, one-line text input through a small
//! dialog built in memory (Win32 has no stock input box, and gpui has no text field).
//! Both run modally from the hidden window's message handler, outside any gpui update, and
//! the answer comes back over a channel.

use std::sync::mpsc;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK, DeleteObject, DrawTextW, GetDC,
    GetMonitorInfoW, GetObjectW, HFONT, HGDIOBJ, LOGFONTW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_POSITION_RELATIVE_TO_WINDOW, TaskDialogIndirect,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, DLGTEMPLATE, DialogBoxIndirectParamW, EndDialog, GWL_EXSTYLE, GWL_STYLE,
    GWLP_USERDATA, GetDlgItem, GetParent, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, IDCANCEL, IDOK, MapDialogRect, MoveWindow, SWP_NOZORDER, SendMessageW,
    SetDlgItemTextW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_COMMAND, WM_DESTROY, WM_GETFONT, WM_INITDIALOG, WM_SETFONT, WS_EX_TOOLWINDOW,
};
use windows::core::PCWSTR;

use crate::win::wide;

pub struct Alert {
    pub title: String,
    pub message: String,
    pub ok: String,
    /// The default button is Cancel, for removals.
    pub destructive: bool,
    /// Show a text field with this initial value.
    pub input: Option<String>,
    pub placeholder: Option<String>,
}

impl Alert {
    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        ok: impl Into<String>,
    ) -> Alert {
        Alert {
            title: title.into(),
            message: message.into(),
            ok: ok.into(),
            destructive: false,
            input: None,
            placeholder: None,
        }
    }

    pub fn destructive(mut self) -> Alert {
        self.destructive = true;
        self
    }

    pub fn with_input(mut self, value: impl Into<String>, placeholder: impl Into<String>) -> Alert {
        self.input = Some(value.into());
        self.placeholder = Some(placeholder.into());
        self
    }
}

/// Show the alert. The receiver yields `Some(text)` for OK (empty without a text
/// field) and `None` for Cancel.
pub fn show(alert: Alert) -> mpsc::Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    crate::win::on_main(move || {
        let answer = match alert.input {
            Some(_) => run_input(&alert),
            None => run_confirm(&alert),
        };
        let _ = tx.send(answer);
    });
    rx
}

/// Show `alert` and call `then` with the answer on the gpui side.
pub fn ask(
    cx: &mut gpui::App,
    alert: Alert,
    then: impl FnOnce(Option<String>, &mut gpui::App) + 'static,
) {
    let rx = show(alert);
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let answer = cx
            .background_executor()
            .spawn(async move { rx.recv().ok().flatten() })
            .await;
        let _ = cx.update(|cx| then(answer, cx));
    })
    .detach();
}

/// The settings or setup window the prompt belongs to; not the popover, which closes as
/// soon as the prompt takes focus.
fn owner() -> Option<HWND> {
    let hwnd = unsafe { GetActiveWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    (ex & WS_EX_TOOLWINDOW.0 == 0).then_some(hwnd)
}

fn cancel_label() -> &'static str {
    crate::i18n::l("Cancel", "İptal")
}

fn run_confirm(a: &Alert) -> Option<String> {
    let (window_title, title, message) = (wide(crate::APP_NAME), wide(&a.title), wide(&a.message));
    let (ok, cancel) = (wide(&a.ok), wide(cancel_label()));
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: IDOK.0,
            pszButtonText: PCWSTR(ok.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: IDCANCEL.0,
            pszButtonText: PCWSTR(cancel.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner().unwrap_or_default(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        pszWindowTitle: PCWSTR(window_title.as_ptr()),
        pszMainInstruction: PCWSTR(title.as_ptr()),
        pszContent: PCWSTR(message.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: if a.destructive { IDCANCEL.0 } else { IDOK.0 },
        ..Default::default()
    };
    let mut pressed = 0;
    let shown = unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) };
    (shown.is_ok() && pressed == IDOK.0).then(String::new)
}

// ---- text input -------------------------------------------------------------

const ID_TITLE: i32 = 100;
const ID_MESSAGE: i32 = 101;
const ID_EDIT: i32 = 102;
const EM_SETSEL: u32 = 0x00b1;
const EM_SETCUEBANNER: u32 = 0x1501;

/// Dialog width in dialog units; the height follows the text.
const WIDTH_DLU: i32 = 250;

struct Input {
    title: Vec<u16>,
    message: Vec<u16>,
    ok: Vec<u16>,
    cancel: Vec<u16>,
    value: Vec<u16>,
    placeholder: Vec<u16>,
    bold: HFONT,
    answer: Option<String>,
}

fn run_input(a: &Alert) -> Option<String> {
    let mut input = Input {
        title: wide(&a.title),
        message: wide(&a.message),
        ok: wide(&a.ok),
        cancel: wide(cancel_label()),
        value: wide(a.input.as_deref().unwrap_or("")),
        placeholder: wide(a.placeholder.as_deref().unwrap_or("")),
        bold: HFONT::default(),
        answer: None,
    };
    let template = template();
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        DialogBoxIndirectParamW(
            Some(instance.into()),
            template.as_ptr() as *const DLGTEMPLATE,
            owner(),
            Some(input_proc),
            LPARAM(&mut input as *mut Input as isize),
        );
    }
    input.answer
}

/// An in-memory DLGTEMPLATE: two labels, an edit field and two buttons. Positions are
/// placeholders; `layout` sizes everything to the text when the dialog opens.
fn template() -> Vec<u32> {
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_CHILD: u32 = 0x4000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const WS_CAPTION: u32 = 0x00c0_0000;
    const WS_SYSMENU: u32 = 0x0008_0000;
    const WS_TABSTOP: u32 = 0x0001_0000;
    const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
    const DS_SETFONT: u32 = 0x40;
    const DS_MODALFRAME: u32 = 0x80;
    const SS_NOPREFIX: u32 = 0x80;
    const ES_AUTOHSCROLL: u32 = 0x80;
    const BS_DEFPUSHBUTTON: u32 = 0x1;
    const BUTTON: u16 = 0x80;
    const EDIT: u16 = 0x81;
    const STATIC: u16 = 0x82;

    let mut t: Vec<u16> = Vec::new();
    let push_u32 = |t: &mut Vec<u16>, v: u32| t.extend([v as u16, (v >> 16) as u16]);
    let push_str = |t: &mut Vec<u16>, s: &str| t.extend(wide(s));
    push_u32(
        &mut t,
        WS_POPUP | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_SETFONT,
    );
    push_u32(&mut t, 0);
    t.extend([5, 0, 0, WIDTH_DLU as u16, 100]); // items, x, y, cx, cy
    t.extend([0, 0]); // no menu, default class
    push_str(&mut t, crate::APP_NAME);
    t.push(9);
    push_str(&mut t, "Segoe UI");
    let items: [(u32, u32, i32, u16); 5] = [
        (SS_NOPREFIX, 0, ID_TITLE, STATIC),
        (SS_NOPREFIX, 0, ID_MESSAGE, STATIC),
        (WS_TABSTOP | ES_AUTOHSCROLL, WS_EX_CLIENTEDGE, ID_EDIT, EDIT),
        (WS_TABSTOP | BS_DEFPUSHBUTTON, 0, IDOK.0, BUTTON),
        (WS_TABSTOP, 0, IDCANCEL.0, BUTTON),
    ];
    for (style, ex, id, class) in items {
        if t.len() % 2 == 1 {
            t.push(0); // DWORD alignment
        }
        push_u32(&mut t, WS_CHILD | WS_VISIBLE | style);
        push_u32(&mut t, ex);
        t.extend([0, 0, 10, 10, id as u16, 0xffff, class]);
        t.push(0); // text, set in WM_INITDIALOG
        t.push(0); // no creation data
    }
    if t.len() % 2 == 1 {
        t.push(0);
    }
    t.chunks(2)
        .map(|c| c[0] as u32 | (c[1] as u32) << 16)
        .collect()
}

unsafe extern "system" fn input_proc(
    hdlg: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    unsafe {
        match msg {
            WM_INITDIALOG => {
                SetWindowLongPtrW(hdlg, GWLP_USERDATA, lparam.0);
                let input = &mut *(lparam.0 as *mut Input);
                init_input(hdlg, input);
                0 // focus is set
            }
            WM_COMMAND => {
                // the edit field can report changes before WM_INITDIALOG stores the state
                let input = GetWindowLongPtrW(hdlg, GWLP_USERDATA) as *mut Input;
                let id = (wparam.0 & 0xffff) as i32;
                if id == IDOK.0 && !input.is_null() {
                    (*input).answer = Some(read_text(
                        GetDlgItem(Some(hdlg), ID_EDIT).unwrap_or_default(),
                    ));
                    let _ = EndDialog(hdlg, 1);
                } else if id == IDCANCEL.0 {
                    let _ = EndDialog(hdlg, 0);
                }
                1
            }
            WM_DESTROY => {
                let input = GetWindowLongPtrW(hdlg, GWLP_USERDATA) as *mut Input;
                if !input.is_null() && !(*input).bold.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*input).bold.0));
                }
                0
            }
            _ => 0,
        }
    }
}

unsafe fn init_input(hdlg: HWND, input: &mut Input) {
    unsafe {
        let item = |id: i32| GetDlgItem(Some(hdlg), id).unwrap_or_default();
        let font = HFONT(SendMessageW(hdlg, WM_GETFONT, None, None).0 as *mut _);
        let mut lf = LOGFONTW::default();
        GetObjectW(
            HGDIOBJ(font.0),
            std::mem::size_of::<LOGFONTW>() as i32,
            Some((&mut lf as *mut LOGFONTW).cast()),
        );
        lf.lfWeight = 600;
        lf.lfHeight = lf.lfHeight * 6 / 5;
        input.bold = CreateFontIndirectW(&lf);
        SendMessageW(
            item(ID_TITLE),
            WM_SETFONT,
            Some(WPARAM(input.bold.0 as usize)),
            Some(LPARAM(1)),
        );
        let _ = SetDlgItemTextW(hdlg, ID_TITLE, PCWSTR(input.title.as_ptr()));
        let _ = SetDlgItemTextW(hdlg, ID_MESSAGE, PCWSTR(input.message.as_ptr()));
        let _ = SetDlgItemTextW(hdlg, ID_EDIT, PCWSTR(input.value.as_ptr()));
        let _ = SetDlgItemTextW(hdlg, IDOK.0, PCWSTR(input.ok.as_ptr()));
        let _ = SetDlgItemTextW(hdlg, IDCANCEL.0, PCWSTR(input.cancel.as_ptr()));
        let edit = item(ID_EDIT);
        SendMessageW(
            edit,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(input.placeholder.as_ptr() as isize)),
        );
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
        layout(hdlg, input.bold, font);
        let _ = SetForegroundWindow(hdlg);
        let _ = SetFocus(Some(edit));
    }
}

/// Stack title, message, field and buttons, size the dialog to them and centre it on its
/// owner (or the screen).
unsafe fn layout(hdlg: HWND, bold: HFONT, font: HFONT) {
    unsafe {
        let dlu = |x: i32, y: i32| {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: x,
                bottom: y,
            };
            let _ = MapDialogRect(hdlg, &mut r);
            (r.right, r.bottom)
        };
        let (margin, _) = dlu(10, 0);
        let (width, _) = dlu(WIDTH_DLU, 0);
        let (button_w, button_h) = dlu(54, 14);
        let (_, edit_h) = dlu(0, 14);
        let inner = width - 2 * margin;
        let measure = |id: i32, f: HFONT| {
            let ctl = GetDlgItem(Some(hdlg), id).unwrap_or_default();
            let mut text: Vec<u16> = vec![0; GetWindowTextLengthW(ctl) as usize + 1];
            let n = GetWindowTextW(ctl, &mut text) as usize;
            let dc = GetDC(Some(hdlg));
            let old = SelectObject(dc, HGDIOBJ(f.0));
            let mut r = RECT {
                left: 0,
                top: 0,
                right: inner,
                bottom: 0,
            };
            DrawTextW(
                dc,
                &mut text[..n],
                &mut r,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(dc, old);
            ReleaseDC(Some(hdlg), dc);
            r.bottom
        };
        let place = |id: i32, x: i32, y: i32, w: i32, h: i32| {
            let _ = MoveWindow(
                GetDlgItem(Some(hdlg), id).unwrap_or_default(),
                x,
                y,
                w,
                h,
                true,
            );
        };
        let mut y = margin;
        let title_h = measure(ID_TITLE, bold);
        place(ID_TITLE, margin, y, inner, title_h);
        y += title_h + margin / 2;
        let message_h = measure(ID_MESSAGE, font);
        place(ID_MESSAGE, margin, y, inner, message_h);
        y += message_h + margin;
        place(ID_EDIT, margin, y, inner, edit_h);
        y += edit_h + margin * 3 / 2;
        place(IDCANCEL.0, width - margin - button_w, y, button_w, button_h);
        place(
            IDOK.0,
            width - margin - 2 * button_w - margin / 2,
            y,
            button_w,
            button_h,
        );
        y += button_h + margin;

        let mut frame = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: y,
        };
        let style = WINDOW_STYLE(GetWindowLongPtrW(hdlg, GWL_STYLE) as u32);
        let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hdlg, GWL_EXSTYLE) as u32);
        let _ = AdjustWindowRectEx(&mut frame, style, false, ex);
        let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
        let area = match GetParent(hdlg) {
            Ok(owner) if !owner.is_invalid() => {
                let mut r = RECT::default();
                let _ = GetWindowRect(owner, &mut r);
                r
            }
            _ => {
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ =
                    GetMonitorInfoW(MonitorFromWindow(hdlg, MONITOR_DEFAULTTONEAREST), &mut info);
                info.rcWork
            }
        };
        let x = area.left + (area.right - area.left - w) / 2;
        let y = area.top + ((area.bottom - area.top - h) / 3).max(0);
        let _ = SetWindowPos(hdlg, None, x, y, w, h, SWP_NOZORDER);
    }
}

fn read_text(edit: HWND) -> String {
    unsafe {
        let mut buf: Vec<u16> = vec![0; GetWindowTextLengthW(edit) as usize + 1];
        let n = GetWindowTextW(edit, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}
