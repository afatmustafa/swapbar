//! Interface language: English or Turkish. `Language::System` follows the first macOS
//! preferred language (the display language on Windows). Strings sit next to their use as
//! English/Turkish pairs: `l()` for plain text, `lf!` for formatted text.

#[cfg(target_os = "macos")]
use cocoa::base::{id, nil};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings::Language;

static TURKISH: AtomicBool = AtomicBool::new(false);

pub fn set(lang: Language) {
    let tr = match lang {
        Language::English => false,
        Language::Turkish => true,
        Language::System => system_is_turkish(),
    };
    TURKISH.store(tr, Ordering::Relaxed);
}

pub fn is_tr() -> bool {
    TURKISH.load(Ordering::Relaxed)
}

/// The English or Turkish text, by the current language.
pub fn l(en: &'static str, tr: &'static str) -> &'static str {
    pick(is_tr(), en, tr)
}

fn pick<T>(turkish: bool, en: T, tr: T) -> T {
    if turkish { tr } else { en }
}

/// The macOS or the Windows wording, for text that names the platform:
/// `os(l("menu bar", "menü çubuğu"), l("tray", "tepsi"))`.
pub fn os<T>(mac: T, windows: T) -> T {
    if cfg!(target_os = "windows") {
        windows
    } else {
        mac
    }
}

/// `format!` with an English and a Turkish template sharing the same arguments.
#[macro_export]
macro_rules! lf {
    ($en:literal, $tr:literal $(, $($arg:tt)*)?) => {
        if $crate::i18n::is_tr() {
            format!($tr $(, $($arg)*)?)
        } else {
            format!($en $(, $($arg)*)?)
        }
    };
}

/// cswap's English time strings in the interface language: "2d 6h" → "2g 6sa",
/// "Oct 22 20:16" → "22 Eki 20:16".
pub fn time(s: &str) -> String {
    if is_tr() { time_tr(s) } else { s.to_string() }
}

const MONTHS: [(&str, &str); 12] = [
    ("Jan", "Oca"),
    ("Feb", "Şub"),
    ("Mar", "Mar"),
    ("Apr", "Nis"),
    ("May", "May"),
    ("Jun", "Haz"),
    ("Jul", "Tem"),
    ("Aug", "Ağu"),
    ("Sep", "Eyl"),
    ("Oct", "Eki"),
    ("Nov", "Kas"),
    ("Dec", "Ara"),
];

fn time_tr(s: &str) -> String {
    let words: Vec<&str> = s.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some((_, tr)) = MONTHS.iter().find(|(en, _)| *en == w)
            && let Some(day) = words
                .get(i + 1)
                .filter(|d| d.chars().all(|c| c.is_ascii_digit()))
        {
            out.push(format!("{day} {tr}"));
            i += 2;
            continue;
        }
        let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let unit = &w[digits.len()..];
        let unit_tr = match unit {
            "d" => Some("g"),
            "h" => Some("sa"),
            "m" => Some("dk"),
            "s" => Some("sn"),
            _ => None,
        };
        match unit_tr {
            Some(u) if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
                out.push(format!("{digits}{u}"))
            }
            _ => out.push(w.to_string()),
        }
        i += 1;
    }
    out.join(" ")
}

#[cfg(target_os = "windows")]
fn system_is_turkish() -> bool {
    const LANG_TURKISH: u16 = 0x1f;
    let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    lang & 0x3ff == LANG_TURKISH
}

#[cfg(target_os = "macos")]
fn system_is_turkish() -> bool {
    unsafe {
        let langs: id = msg_send![class!(NSLocale), preferredLanguages];
        if langs == nil {
            return false;
        }
        let count: usize = msg_send![langs, count];
        if count == 0 {
            return false;
        }
        let first: id = msg_send![langs, objectAtIndex: 0usize];
        let p: *const std::os::raw::c_char = msg_send![first, UTF8String];
        !p.is_null()
            && std::ffi::CStr::from_ptr(p)
                .to_string_lossy()
                .starts_with("tr")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The language is process-wide and tests run in parallel, so test the choice itself.
    #[test]
    fn picks_the_language() {
        assert_eq!(pick(true, "Quit", "Çık"), "Çık");
        assert_eq!(pick(false, "Quit", "Çık"), "Quit");
    }

    #[test]
    fn turkish_times() {
        assert_eq!(time_tr("2d 6h"), "2g 6sa");
        assert_eq!(time_tr("37m"), "37dk");
        assert_eq!(time_tr("Oct 22 20:16"), "22 Eki 20:16");
        assert_eq!(time_tr("17:28"), "17:28");
    }
}
