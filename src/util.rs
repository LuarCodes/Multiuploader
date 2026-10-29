//! Small helpers: wide strings, human-readable sizes and times.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

/// UTF-16, NUL-terminated, for Win32 calls.
pub fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Wide path (no NUL) -> PathBuf, lossless.
pub fn path_from_wide(p: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(p))
}

pub fn wide_of_path(p: &std::path::Path) -> Vec<u16> {
    p.as_os_str().encode_wide().collect()
}

pub fn human_size(mut n: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut i = 0;
    while n >= 1024.0 && i < 4 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{:.0} {}", n, UNITS[i])
    } else {
        format!("{:.1} {}", n, UNITS[i])
    }
}

pub fn human_time(sec: f64) -> String {
    if !(sec >= 0.0) || sec.is_infinite() {
        return "--:--".into();
    }
    let s = sec as i64;
    let (h, m, r) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, r)
    } else {
        format!("{:02}:{:02}", m, r)
    }
}

pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}
