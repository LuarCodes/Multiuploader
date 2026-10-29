//! Multi Uploader - native Win32 / Rust port.
//! Upload files to many free file hosts with one click. No runtime, no installer.
#![windows_subsystem = "windows"]
#![allow(static_mut_refs, clippy::missing_safety_doc)]

mod engine;
mod json;
mod sites;
mod uploader;
mod util;
mod win;

use engine::{Engine, Event, FileItem};
use sites::{site_index, SITES};
use std::collections::{HashSet, VecDeque};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::Instant;
use uploader::Options;
use util::*;
use win::*;
// explicit imports win over the windows-sys globs below
use win::{
    BS_OWNERDRAW, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, DT_CENTER, DT_SINGLELINE, DT_VCENTER, EN_CHANGE,
    ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_CENTER, ES_MULTILINE, ES_NOHIDESEL, ES_NUMBER, ES_READONLY, WM_APP, WM_DPICHANGED,
    SS_CENTER, SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_LEFT, SS_RIGHT,
};

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{GlobalFree, FALSE, TRUE};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryExW, LoadLibraryW};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::Controls::Dialogs::{GetSaveFileNameW, OPENFILENAMEW};
use windows_sys::Win32::UI::Controls::{ImageList_Create, ImageList_Destroy, InitCommonControlsEx, SetWindowTheme, INITCOMMONCONTROLSEX};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, SetWindowSubclass, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ------------------------------------------------------------------ colours
const ACCENT: u32 = rgb(0x7C, 0x5C, 0xFF);
const ACCENT_HOVER: u32 = rgb(0x68, 0x46, 0xF5);
const BG: u32 = rgb(0x0F, 0x11, 0x17);
const PANEL: u32 = rgb(0x17, 0x1A, 0x23);
const CARD: u32 = rgb(0x1F, 0x23, 0x30);
const CARD_HOVER: u32 = rgb(0x26, 0x2B, 0x3A);
const BORDER: u32 = rgb(0x2C, 0x32, 0x42);
const TEXTC: u32 = rgb(0xE8, 0xEA, 0xF0);
const MUTED: u32 = rgb(0x8B, 0x92, 0xA5);
const GREEN: u32 = rgb(0x3D, 0xDC, 0x97);
const RED: u32 = rgb(0xFF, 0x5C, 0x7A);
const YELLOW: u32 = rgb(0xFF, 0xC8, 0x57);

const APP_NAME: &str = "Multi Uploader";

// ------------------------------------------------------------------ ids
const IDC_L_ICON: i32 = 100;
const IDC_L_TITLE: i32 = 101;
const IDC_L_SUB: i32 = 102;
const IDC_L_UPLOADTO: i32 = 103;
const IDC_ALL: i32 = 104;
const IDC_NONE: i32 = 105;
const IDC_SITES: i32 = 106;
const IDC_L_COPIES: i32 = 107;
const IDC_COPIES: i32 = 108;
const IDC_COPIES_UD: i32 = 109;
const IDC_L_PAR: i32 = 110;
const IDC_PAR: i32 = 111;
const IDC_PAR_UD: i32 = 112;
const IDC_L_LITTER: i32 = 113;
const IDC_LITTER: i32 = 114;
const IDC_SUMMARY: i32 = 115;
const IDC_UPLOAD: i32 = 116;
const IDC_CANCEL: i32 = 117;
const IDC_DROP: i32 = 118;
const IDC_FILES_TITLE: i32 = 119;
const IDC_ADDFILES: i32 = 120;
const IDC_ADDFOLDER: i32 = 121;
const IDC_CLEAR: i32 = 122;
const IDC_FILES: i32 = 123;
const IDC_STATUS: i32 = 124;
const IDC_STATS: i32 = 125;
const IDC_BAR: i32 = 126;
const IDC_LINKS_TITLE: i32 = 127;
const IDC_COPY: i32 = 128;
const IDC_SAVE: i32 = 129;
const IDC_LOG: i32 = 130;

const WM_ENGINE: u32 = WM_APP + 1;
const TIMER_PROGRESS: usize = 1;
const TIMER_AUTOSAVE: usize = 2;
const TIMER_COPIED: usize = 3;

// fonts
const F_TITLE: usize = 0;
const F_BOLD: usize = 1;
const F_NORMAL: usize = 2;
const F_SMALL: usize = 3;
const F_HEAD: usize = 4;
const F_MONO: usize = 5;
const F_ICON: usize = 6;
const F_BIG: usize = 7;

// ------------------------------------------------------------------ state
struct FileEntry {
    path: Vec<u16>,
    name: String,
    size: u64,
    active: i32, // uploads in flight
    touched: bool,
}

struct ResultRow {
    site: usize,
    file: usize,
    copy: u32,
    link: String,
}

struct App {
    inst: Handle,
    wnd: HWND,
    dpi: u32,
    fonts: [Handle; 8],
    br_bg: Handle,
    br_panel: Handle,
    br_card: Handle,
    br_accent: Handle,
    files: Vec<FileEntry>,
    file_set: HashSet<String>,
    results: Vec<ResultRow>,
    engine: Engine,
    last_copies: u32,
    last_litter: String,
    initializing: bool,
    drop_hover: bool,
    prog_rect: RECT,
    side_rect: RECT,
    started: Instant,
    speed: VecDeque<(f64, i64)>,
}

static mut APP: *mut App = null_mut();

fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn s(v: i32) -> i32 {
    muldiv(v, app().dpi as i32, 96)
}

fn ctl(id: i32) -> HWND {
    unsafe { GetDlgItem(app().wnd, id) }
}

fn enable(id: i32, on: bool) {
    unsafe { EnableWindow(ctl(id), if on { TRUE } else { FALSE }) };
}

fn message_box(text: &str, caption: &str, flags: u32) -> i32 {
    unsafe { MessageBoxW(app().wnd, w(text).as_ptr(), w(caption).as_ptr(), flags) }
}

// ------------------------------------------------------------------ dpi / dark helpers
fn dpi_for_window(h: HWND) -> u32 {
    unsafe {
        if !h.is_null() {
            let d = GetDpiForWindow(h);
            if d != 0 {
                return d;
            }
        }
        let dc = GetDC(null_mut());
        let d = GetDeviceCaps(dc, LOGPIXELSX as _) as u32;
        ReleaseDC(null_mut(), dc);
        if d == 0 { 96 } else { d }
    }
}

fn allow_dark_app() {
    unsafe {
        let ux = LoadLibraryExW(w("uxtheme.dll").as_ptr(), null_mut(), 0x800 /* LOAD_LIBRARY_SEARCH_SYSTEM32 */);
        if ux.is_null() {
            return;
        }
        // SetPreferredAppMode, ordinal 135 (Win10 1809+)
        if let Some(f) = GetProcAddress(ux, 135usize as *const u8) {
            let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            f(1); // AllowDark
        }
    }
}

fn dark_title_bar(h: HWND) {
    let on: i32 = 1;
    unsafe {
        if DwmSetWindowAttribute(h, 20, &on as *const i32 as *const c_void, 4) < 0 {
            DwmSetWindowAttribute(h, 19, &on as *const i32 as *const c_void, 4);
        }
    }
}

fn make_font(pt: i32, weight: i32, face: &str) -> Handle {
    unsafe {
        CreateFontW(
            -muldiv(pt, app().dpi as i32, 72),
            0, 0, 0, weight, 0, 0, 0,
            1 /* DEFAULT_CHARSET */, 0, 0, 5 /* CLEARTYPE_QUALITY */, 0,
            w(face).as_ptr(),
        ) as Handle
    }
}

fn create_fonts() {
    let a = app();
    for f in a.fonts.iter_mut() {
        if !f.is_null() {
            unsafe { DeleteObject(*f as _) };
            *f = null_mut();
        }
    }
    a.fonts[F_TITLE] = make_font(15, 700, "Segoe UI");
    a.fonts[F_BOLD] = make_font(10, 600, "Segoe UI");
    a.fonts[F_NORMAL] = make_font(10, 400, "Segoe UI");
    a.fonts[F_SMALL] = make_font(9, 400, "Segoe UI");
    a.fonts[F_HEAD] = make_font(8, 700, "Segoe UI");
    a.fonts[F_MONO] = make_font(10, 400, "Consolas");
    a.fonts[F_ICON] = make_font(18, 700, "Segoe UI Symbol");
    a.fonts[F_BIG] = make_font(12, 700, "Segoe UI");
}

// ------------------------------------------------------------------ misc helpers
fn get_int(id: i32, lo: i32, hi: i32, def: i32) -> i32 {
    let t = get_text(ctl(id));
    let t = t.trim();
    let v = if t.is_empty() {
        def
    } else {
        // like _wtoi: leading digits, 0 if none
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<i64>().unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    };
    v.clamp(lo, hi)
}

fn selected_sites() -> Vec<usize> {
    let lv = ctl(IDC_SITES);
    (0..SITES.len()).filter(|&i| lv_get_check(lv, i as i32)).collect()
}

fn litter_choice() -> String {
    const OPTS: [&str; 4] = ["1h", "12h", "24h", "72h"];
    let i = send(ctl(IDC_LITTER), CB_GETCURSEL, 0, 0);
    OPTS[if (0..4).contains(&i) { i as usize } else { 3 }].to_string()
}

fn update_summary() {
    if app().initializing {
        return;
    }
    let sel = selected_sites();
    let copies = get_int(IDC_COPIES, 1, 1000, 1) as usize;
    let n = app().files.len();
    set_text(
        ctl(IDC_SUMMARY),
        &format!("{} file(s)  \u{00D7}  {} site(s)  \u{00D7}  {} copies  =  {} uploads", n, sel.len(), copies, n * sel.len() * copies),
    );
    let litter = site_index("litterbox").is_some_and(|i| lv_get_check(ctl(IDC_SITES), i as i32));
    enable(IDC_LITTER, litter && !app().engine.running());
}

fn update_files_title() {
    let a = app();
    let total: u64 = a.files.iter().map(|f| f.size).sum();
    let mut t = String::from("FILES");
    if !a.files.is_empty() {
        t += &format!("  \u{00B7}  {}  \u{00B7}  {}", a.files.len(), human_size(total as f64));
    }
    set_text(ctl(IDC_FILES_TITLE), &t);
    update_summary();
}

fn set_file_status(i: usize, text: &str) {
    lv_set_text(ctl(IDC_FILES), i as i32, 2, text);
}

// ------------------------------------------------------------------ log (RichEdit)
fn append_log(text: &str, color: u32) {
    let re = ctl(IDC_LOG);
    let mut cf: CharFormatW = unsafe { std::mem::zeroed() };
    cf.cbSize = std::mem::size_of::<CharFormatW>() as u32;
    cf.dwMask = CFM_COLOR;
    cf.crTextColor = color;
    let len = unsafe { GetWindowTextLengthW(re) } as usize;
    let t = w(text);
    send(re, EM_SETREADONLY, 0, 0);
    send(re, EM_SETSEL, len, len as isize);
    send(re, EM_SETCHARFORMAT, SCF_SELECTION, &cf as *const _ as isize);
    send(re, EM_REPLACESEL, 0, t.as_ptr() as isize);
    send(re, EM_SETREADONLY, 1, 0);
    send(re, WM_VSCROLL, 7 /* SB_BOTTOM */, 0);
}

fn tag_color(tag: u8) -> u32 {
    match tag {
        1 => GREEN,
        2 => RED,
        3 => YELLOW,
        4 => MUTED,
        5 => ACCENT,
        _ => TEXTC,
    }
}

fn log_line(site: Option<usize>, text: &str, tag: u8) {
    if let Some(si) = site {
        append_log(&format!("[{}] ", SITES[si].name), ACCENT);
    }
    append_log(&format!("{}\r\n", text), tag_color(tag));
}

fn clear_log() {
    set_text(ctl(IDC_LOG), "");
}

// ------------------------------------------------------------------ files
fn add_file(raw: PathBuf) {
    let p = std::path::absolute(&raw).unwrap_or(raw);
    let Ok(md) = std::fs::metadata(&p) else { return };
    if md.is_dir() {
        let mut files_here: Vec<PathBuf> = Vec::new();
        let mut dirs_here: Vec<PathBuf> = Vec::new();
        let Ok(rd) = std::fs::read_dir(&p) else { return };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let child = e.path();
            if ft.is_symlink() {
                // links to folders are skipped (no loops); links to files are followed
                if std::fs::metadata(&child).is_ok_and(|m| m.is_file()) {
                    files_here.push(child);
                }
            } else if ft.is_dir() {
                dirs_here.push(child);
            } else {
                files_here.push(child);
            }
        }
        let key = |a: &PathBuf| a.to_string_lossy().to_lowercase();
        files_here.sort_by_key(key);
        dirs_here.sort_by_key(key);
        for f in files_here {
            add_file(f);
        }
        for d in dirs_here {
            add_file(d);
        }
        return;
    }
    let key = p.to_string_lossy().to_lowercase();
    let a = app();
    if !a.file_set.insert(key) {
        return;
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned());
    let e = FileEntry { path: wide_of_path(&p), name: name.clone(), size: md.len(), active: 0, touched: false };
    let size = human_size(e.size as f64);
    a.files.push(e);
    let lv = ctl(IDC_FILES);
    let idx = lv_insert_item(lv, a.files.len() as i32 - 1, &name);
    lv_set_text(lv, idx, 1, &size);
    lv_set_text(lv, idx, 2, "Ready");
}

fn add_paths(paths: Vec<PathBuf>) {
    if app().engine.running() || paths.is_empty() {
        return;
    }
    let lv = ctl(IDC_FILES);
    send(lv, WM_SETREDRAW, 0, 0);
    for p in paths {
        add_file(p);
    }
    send(lv, WM_SETREDRAW, 1, 0);
    unsafe { InvalidateRect(lv, null(), TRUE) };
    update_files_title();
}

fn remove_selected() {
    if app().engine.running() {
        return;
    }
    let lv = ctl(IDC_FILES);
    let mut sel = Vec::new();
    let mut i: isize = -1;
    loop {
        i = send(lv, LVM_GETNEXTITEM, i as usize, LVNI_SELECTED);
        if i == -1 {
            break;
        }
        sel.push(i as usize);
    }
    let a = app();
    for &i in sel.iter().rev() {
        if i < a.files.len() {
            let k = path_from_wide(&a.files[i].path).to_string_lossy().to_lowercase();
            a.file_set.remove(&k);
            a.files.remove(i);
            send(lv, LVM_DELETEITEM, i, 0);
        }
    }
    update_files_title();
}

fn clear_files() {
    if app().engine.running() {
        return;
    }
    app().files.clear();
    app().file_set.clear();
    send(ctl(IDC_FILES), LVM_DELETEALLITEMS, 0, 0);
    update_files_title();
}

// ---- IFileOpenDialog through raw COM vtables
type Fp = usize;
#[repr(C)]
struct FileOpenDialogVtbl {
    qi: Fp,
    add_ref: Fp,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    show: unsafe extern "system" fn(*mut c_void, HWND) -> i32,
    set_file_types: Fp,
    set_file_type_index: Fp,
    get_file_type_index: Fp,
    advise: Fp,
    unadvise: Fp,
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    set_default_folder: Fp,
    set_folder: Fp,
    get_folder: Fp,
    get_current_selection: Fp,
    set_file_name: Fp,
    get_file_name: Fp,
    set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> i32,
    set_ok_button_label: Fp,
    set_file_name_label: Fp,
    get_result: Fp,
    add_place: Fp,
    set_default_extension: Fp,
    close: Fp,
    set_client_guid: Fp,
    clear_client_data: Fp,
    set_filter: Fp,
    get_results: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    get_selected_items: Fp,
}
#[repr(C)]
struct ShellItemArrayVtbl {
    qi: Fp,
    add_ref: Fp,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind_to_handler: Fp,
    get_property_store: Fp,
    get_property_description_list: Fp,
    get_attributes: Fp,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    get_item_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
    enum_items: Fp,
}
#[repr(C)]
struct ShellItemVtbl {
    qi: Fp,
    add_ref: Fp,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind_to_handler: Fp,
    get_parent: Fp,
    get_display_name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32,
    get_attributes: Fp,
    compare: Fp,
}

unsafe fn vt<T>(obj: *mut c_void) -> &'static T {
    &**(obj as *mut *const T)
}

fn pick_with_dialog(folders: bool) -> Vec<PathBuf> {
    const CLSID_FILE_OPEN_DIALOG: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
    const IID_IFILE_OPEN_DIALOG: GUID = GUID::from_u128(0xD57C7288_D4AD_4768_BE02_9D969532D960);
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_ALLOWMULTISELECT: u32 = 0x200;
    const FOS_NODEREFERENCELINKS: u32 = 0x0010_0000;
    const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
    let mut out = Vec::new();
    unsafe {
        let mut dlg: *mut c_void = null_mut();
        if CoCreateInstance(&CLSID_FILE_OPEN_DIALOG, null_mut(), 1 /* CLSCTX_INPROC_SERVER */, &IID_IFILE_OPEN_DIALOG, &mut dlg) < 0
            || dlg.is_null()
        {
            return out;
        }
        let d = vt::<FileOpenDialogVtbl>(dlg);
        let mut opts: u32 = 0;
        (d.get_options)(dlg, &mut opts);
        opts |= FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT | FOS_NODEREFERENCELINKS;
        if folders {
            opts |= FOS_PICKFOLDERS;
        }
        (d.set_options)(dlg, opts);
        let title = w(if folders { "Choose folders to upload" } else { "Choose files to upload" });
        (d.set_title)(dlg, title.as_ptr());
        if (d.show)(dlg, app().wnd) >= 0 {
            let mut arr: *mut c_void = null_mut();
            if (d.get_results)(dlg, &mut arr) >= 0 && !arr.is_null() {
                let a = vt::<ShellItemArrayVtbl>(arr);
                let mut n: u32 = 0;
                (a.get_count)(arr, &mut n);
                for i in 0..n {
                    let mut item: *mut c_void = null_mut();
                    if (a.get_item_at)(arr, i, &mut item) >= 0 && !item.is_null() {
                        let it = vt::<ShellItemVtbl>(item);
                        let mut p: *mut u16 = null_mut();
                        if (it.get_display_name)(item, SIGDN_FILESYSPATH, &mut p) >= 0 && !p.is_null() {
                            let mut len = 0;
                            while *p.add(len) != 0 {
                                len += 1;
                            }
                            out.push(path_from_wide(std::slice::from_raw_parts(p, len)));
                            CoTaskMemFree(p as *const c_void);
                        }
                        (it.release)(item);
                    }
                }
                (a.release)(arr);
            }
        }
        (d.release)(dlg);
    }
    out
}

fn browse(folders: bool) {
    if app().engine.running() {
        return;
    }
    add_paths(pick_with_dialog(folders));
}

// ------------------------------------------------------------------ settings (ini next to exe)
fn ini_path() -> PathBuf {
    app_dir().join("MultiUploader.ini")
}

fn ini_get(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
    })
}

fn load_settings() {
    let text = std::fs::read(ini_path()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let keys = ini_get(&text, "sites").unwrap_or_else(|| "tempsh".into());
    let lv = ctl(IDC_SITES);
    for k in keys.split(',') {
        if let Some(i) = site_index(k.trim()) {
            lv_set_check(lv, i as i32, true);
        }
    }
    let int = |k: &str, d: i32| ini_get(&text, k).and_then(|v| v.parse::<i32>().ok()).unwrap_or(d);
    set_text(ctl(IDC_COPIES), &int("copies", 1).to_string());
    set_text(ctl(IDC_PAR), &int("parallel", 4).to_string());
    send(ctl(IDC_LITTER), CB_SETCURSEL, int("litter", 3).clamp(0, 3) as usize, 0);
}

fn save_settings() {
    let keys: Vec<&str> = selected_sites().iter().map(|&i| SITES[i].key).collect();
    let text = format!(
        "[settings]\r\nsites={}\r\ncopies={}\r\nparallel={}\r\nlitter={}\r\n",
        keys.join(","),
        get_int(IDC_COPIES, 1, 1000, 1),
        get_int(IDC_PAR, 1, 16, 4),
        send(ctl(IDC_LITTER), CB_GETCURSEL, 0, 0)
    );
    let _ = std::fs::write(ini_path(), text);
}

// ------------------------------------------------------------------ upload control
fn set_busy(busy: bool) {
    for id in [IDC_ADDFILES, IDC_ADDFOLDER, IDC_CLEAR, IDC_SITES, IDC_ALL, IDC_NONE, IDC_COPIES, IDC_COPIES_UD, IDC_PAR, IDC_PAR_UD, IDC_UPLOAD] {
        enable(id, !busy);
    }
    enable(IDC_CANCEL, busy);
    set_text(ctl(IDC_UPLOAD), if busy { "Uploading\u{2026}" } else { "Upload" });
    set_text(ctl(IDC_CANCEL), "Cancel");
    if busy {
        enable(IDC_COPY, false);
        enable(IDC_SAVE, false);
    }
    update_summary();
}

fn start_upload() {
    if app().engine.running() {
        return;
    }
    let sel = selected_sites();
    if app().files.is_empty() {
        message_box("Drop some files first.", "No files", MB_ICONINFORMATION);
        return;
    }
    if sel.is_empty() {
        message_box("Pick at least one site on the left.", "No site", MB_ICONINFORMATION);
        return;
    }
    let copies = get_int(IDC_COPIES, 1, 1000, 1);
    let parallel = get_int(IDC_PAR, 1, 16, 4);
    set_text(ctl(IDC_COPIES), &copies.to_string());
    set_text(ctl(IDC_PAR), &parallel.to_string());
    app().last_copies = copies as u32;
    app().last_litter = litter_choice();
    app().results.clear();
    clear_log();

    let mut items = Vec::new();
    for i in 0..app().files.len() {
        let f = &mut app().files[i];
        // refresh size in case the file changed since it was added
        if let Ok(m) = std::fs::metadata(path_from_wide(&f.path)) {
            f.size = m.len();
        }
        f.active = 0;
        f.touched = false;
        items.push(FileItem { path: f.path.clone(), name: f.name.clone(), size: f.size });
        set_file_status(i, "Waiting");
    }
    send(ctl(IDC_BAR), PBM_SETPOS, 0, 0);
    set_text(ctl(IDC_STATS), "");
    set_text(ctl(IDC_STATUS), "Starting\u{2026}");
    app().speed.clear();
    app().started = Instant::now();

    let opt = Options { litter_time: app().last_litter.clone() };
    let wnd = app().wnd as isize;
    set_busy(true);
    app().engine.start(
        items,
        &sel,
        copies as u32,
        opt,
        parallel as usize,
        Box::new(move |e: Event| {
            let p = Box::into_raw(Box::new(e));
            unsafe {
                if PostMessageW(wnd as HWND, WM_ENGINE, 0, p as isize) == 0 {
                    drop(Box::from_raw(p));
                }
            }
        }),
    );
    unsafe { SetTimer(app().wnd, TIMER_PROGRESS, 150, None) };
}

fn refresh_row_status(file: usize) {
    let a = app();
    let (mut ok, mut bad) = (0, 0);
    for r in a.results.iter().filter(|r| r.file == file) {
        if r.link.is_empty() { bad += 1 } else { ok += 1 }
    }
    let mut st = String::new();
    if a.files[file].active > 0 {
        st = format!("Uploading ({})  ", a.files[file].active);
    }
    if bad > 0 {
        st += &format!("\u{2714} {}   \u{2716} {}", ok, bad);
    } else {
        st += &format!("\u{2714} {} link(s)", ok);
    }
    set_file_status(file, &st);
}

fn update_progress() {
    let a = app();
    let total = a.engine.total_bytes().max(1);
    let done = a.engine.sent_bytes().min(total);
    send(ctl(IDC_BAR), PBM_SETPOS, (done * 10000 / total) as usize, 0);
    let now = a.started.elapsed().as_secs_f64();
    a.speed.push_back((now, done));
    while a.speed.len() > 2 && now - a.speed.front().unwrap().0 > 4.0 {
        a.speed.pop_front();
    }
    let mut speed = 0.0;
    if a.speed.len() > 1 {
        let (f, b) = (a.speed.front().unwrap(), a.speed.back().unwrap());
        let dt = b.0 - f.0;
        if dt > 1e-6 {
            speed = (b.1 - f.1) as f64 / dt;
        }
    }
    let eta = if speed > 0.0 { (total - done) as f64 / speed } else { -1.0 };
    set_text(
        ctl(IDC_STATS),
        &format!(
            "{} / {}   \u{00B7}   {}/s   \u{00B7}   {} left",
            human_size(done as f64),
            human_size(total as f64),
            human_size(speed),
            human_time(eta)
        ),
    );
    set_text(
        ctl(IDC_STATUS),
        &format!(
            "Uploading  \u{00B7}  {} of {} finished  \u{00B7}  {} in parallel",
            a.engine.finished_jobs(),
            a.engine.total_jobs(),
            a.engine.active_jobs()
        ),
    );
}

fn finished(cancelled: bool) {
    let a = app();
    unsafe { KillTimer(a.wnd, TIMER_PROGRESS) };
    a.engine.join();
    let ok = a.results.iter().filter(|r| !r.link.is_empty()).count();
    let bad = a.results.len() - ok;
    let secs = a.started.elapsed().as_secs_f64();
    set_busy(false);
    for i in 0..a.files.len() {
        a.files[i].active = 0;
        if !a.files[i].touched {
            set_file_status(i, if cancelled { "Cancelled" } else { "Skipped" });
        } else {
            refresh_row_status(i);
        }
    }
    if !cancelled {
        send(ctl(IDC_BAR), PBM_SETPOS, 10000, 0);
    }
    let mut st = format!(
        "{}  \u{00B7}  {} uploaded, {} failed  \u{00B7}  {}",
        if cancelled { "Cancelled" } else { "Done" },
        ok,
        bad,
        human_time(secs)
    );
    let sent = a.engine.sent_bytes();
    if secs > 0.5 && sent > 0 && !cancelled {
        st += &format!("  \u{00B7}  avg {}/s", human_size(sent as f64 / secs));
    }
    set_text(ctl(IDC_STATUS), &st);
    set_text(ctl(IDC_STATS), "");
    if ok > 0 {
        enable(IDC_COPY, true);
        enable(IDC_SAVE, true);
        unsafe { SetTimer(a.wnd, TIMER_AUTOSAVE, 250, None) };
    }
}

fn on_engine(e: Event) {
    match e {
        Event::Log { site, tag, text } => log_line(site, &text, tag),
        Event::JobStart { file } => {
            if file < app().files.len() {
                let f = &mut app().files[file];
                f.active += 1;
                f.touched = true;
                refresh_row_status(file);
            }
        }
        Event::Result { site, file, copy, link, error } => {
            if file >= app().files.len() {
                return;
            }
            app().results.push(ResultRow { site, file, copy, link: link.clone() });
            let f = &mut app().files[file];
            f.active = (f.active - 1).max(0);
            refresh_row_status(file);
            let name = format!("{} #{}", app().files[file].name, copy);
            if !link.is_empty() {
                log_line(Some(site), &format!("{}  {}", name, link), 1);
            } else {
                log_line(Some(site), &format!("{}  FAILED  {}", name, error), 2);
            }
        }
        Event::Done { cancelled } => finished(cancelled),
    }
}

// ------------------------------------------------------------------ links output
fn local_time() -> windows_sys::Win32::Foundation::SYSTEMTIME {
    unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    }
}

fn links_text(plain: bool) -> String {
    let a = app();
    let mut rows: Vec<&ResultRow> = a.results.iter().collect();
    rows.sort_by_key(|r| (r.site, r.file, r.copy)); // stable
    let mut out = String::new();
    if plain {
        for r in rows.iter().filter(|r| !r.link.is_empty()) {
            out += &r.link;
            out += "\r\n";
        }
        return out;
    }
    let t = local_time();
    out += &format!(
        "{} links - {:04}-{:02}-{:02} {:02}:{:02}\r\nEach file uploaded {} time(s).\r\n\r\n",
        APP_NAME, t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, a.last_copies
    );
    for (si, site) in SITES.iter().enumerate() {
        if !rows.iter().any(|r| r.site == si) {
            continue;
        }
        let note = if site.key == "litterbox" { format!("Files expire after {}.", a.last_litter) } else { site.note.to_string() };
        out += &format!("=== {} ===  ({})\r\n", site.name, note);
        let mut last_file: Option<usize> = None;
        for r in rows.iter().filter(|r| r.site == si) {
            if last_file != Some(r.file) {
                if last_file.is_some() {
                    out += "\r\n";
                }
                out += &a.files[r.file].name;
                out += "\r\n";
                last_file = Some(r.file);
            }
            out += &format!("  {}. {}\r\n", r.copy, if r.link.is_empty() { "UPLOAD FAILED" } else { &r.link });
        }
        out += "\r\n";
    }
    out
}

fn copy_links() {
    let t: Vec<u16> = links_text(true).encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(app().wnd) == 0 {
            return;
        }
        EmptyClipboard();
        let bytes = t.len() * 2;
        let h = GlobalAlloc(0x0002 /* GMEM_MOVEABLE */, bytes);
        if !h.is_null() {
            let p = GlobalLock(h);
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(t.as_ptr() as *const u8, p as *mut u8, bytes);
                GlobalUnlock(h);
                if SetClipboardData(13 /* CF_UNICODETEXT */, h as _).is_null() {
                    GlobalFree(h);
                }
            } else {
                GlobalFree(h);
            }
        }
        CloseClipboard();
        set_text(ctl(IDC_COPY), "Copied \u{2714}");
        SetTimer(app().wnd, TIMER_COPIED, 1500, None);
    }
}

fn save_links() {
    if !app().results.iter().any(|r| !r.link.is_empty()) {
        return;
    }
    let t = local_time();
    let def = format!(
        "links_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.txt",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    let mut file = vec![0u16; 520];
    for (i, c) in def.encode_utf16().enumerate() {
        file[i] = c;
    }
    let dir = w(&app_dir().to_string_lossy());
    let filter: Vec<u16> = "Text file (*.txt)\0*.txt\0All files\0*.*\0\0".encode_utf16().collect();
    let defext = w("txt");
    let title = w("What should we name the .txt?");
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = app().wnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrInitialDir = dir.as_ptr();
    ofn.lpstrDefExt = defext.as_ptr();
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = 0x2 | 0x800 | 0x80000; // OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER
    if unsafe { GetSaveFileNameW(&mut ofn) } == 0 {
        return;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    let path = path_from_wide(&file[..len]);
    if std::fs::write(&path, links_text(false).as_bytes()).is_err() {
        message_box("Could not save the file.", "Could not save", MB_ICONERROR);
        return;
    }
    log_line(None, &format!("Saved to {}", path.to_string_lossy()), 4);
    let wp = w(&path.to_string_lossy());
    unsafe { ShellExecuteW(app().wnd, w("open").as_ptr(), wp.as_ptr(), null(), null(), SW_SHOWNORMAL) };
    message_box("Check made .txt for links!", "Saved", MB_ICONINFORMATION);
}

// ------------------------------------------------------------------ drop zone control
fn draw_text(dc: HDC, text: &str, rc: &mut RECT, fmt: u32) {
    let mut t: Vec<u16> = text.encode_utf16().collect();
    unsafe { DrawTextW(dc, t.as_mut_ptr(), t.len() as i32, rc, fmt) };
}

unsafe extern "system" fn drop_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_PAINT => {
            let a = app();
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(h, &mut rc);
            let mem = CreateCompatibleDC(dc);
            let bmp = CreateCompatibleBitmap(dc, rc.right, rc.bottom);
            let old_bmp = SelectObject(mem, bmp as _);
            FillRect(mem, &rc, a.br_bg as _);
            let pen = CreatePen(PS_SOLID, s(2), if a.drop_hover { ACCENT } else { BORDER });
            let op = SelectObject(mem, pen as _);
            let ob = SelectObject(mem, a.br_panel as _);
            RoundRect(mem, 1, 1, rc.right - 1, rc.bottom - 1, s(18), s(18));
            SelectObject(mem, op);
            DeleteObject(pen as _);
            let cx = rc.right / 2;
            let top = rc.bottom / 2 - s(48);
            let circ = CreateSolidBrush(rgb(0x2A, 0x2F, 0x45));
            SelectObject(mem, circ as _);
            SelectObject(mem, GetStockObject(NULL_PEN));
            Ellipse(mem, cx - s(26), top, cx + s(26), top + s(52));
            SelectObject(mem, ob);
            DeleteObject(circ as _);
            SetBkMode(mem, TRANSPARENT as _);
            let mut ir = RECT { left: cx - s(26), top, right: cx + s(26), bottom: top + s(52) };
            SetTextColor(mem, ACCENT);
            let of = SelectObject(mem, a.fonts[F_ICON] as _);
            draw_text(mem, "\u{2B06}", &mut ir, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            let mut tr = RECT { left: 0, top: top + s(58), right: rc.right, bottom: top + s(84) };
            SelectObject(mem, a.fonts[F_BIG] as _);
            SetTextColor(mem, TEXTC);
            draw_text(mem, "Drop files here", &mut tr, DT_CENTER | DT_SINGLELINE);
            let mut sr = RECT { left: 0, top: top + s(84), right: rc.right, bottom: top + s(104) };
            SelectObject(mem, a.fonts[F_SMALL] as _);
            SetTextColor(mem, MUTED);
            draw_text(mem, "or click to browse  \u{00B7}  folders are added with everything inside", &mut sr, DT_CENTER | DT_SINGLELINE);
            SelectObject(mem, of);
            BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp as _);
            DeleteDC(mem);
            EndPaint(h, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            if !app().drop_hover {
                app().drop_hover = true;
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                TrackMouseEvent(&mut t);
                InvalidateRect(h, null(), FALSE);
            }
            0
        }
        0x02A3 /* WM_MOUSELEAVE */ => {
            app().drop_hover = false;
            InvalidateRect(h, null(), FALSE);
            0
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            1
        }
        WM_LBUTTONUP => {
            browse(false);
            0
        }
        _ => DefWindowProcW(h, m, wp, lp),
    }
}

// ------------------------------------------------------------------ list header colouring
unsafe extern "system" fn list_subclass(h: HWND, m: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if m == WM_NOTIFY {
        let nh = &*(lp as *const NmHdr);
        if nh.code == NM_CUSTOMDRAW && nh.hwndFrom == lv_header(h) {
            let cd = &*(lp as *const NmCustomDraw);
            if cd.dwDrawStage == CDDS_PREPAINT {
                return CDRF_NOTIFYITEMDRAW;
            }
            if cd.dwDrawStage == CDDS_ITEMPREPAINT {
                SetTextColor(cd.hdc as _, MUTED);
                return CDRF_DODEFAULT;
            }
        }
    }
    if m == WM_KEYDOWN && wp == 0x2E /* VK_DELETE */ && GetDlgCtrlID(h) == IDC_FILES {
        remove_selected();
        return 0;
    }
    DefSubclassProc(h, m, wp, lp)
}

// ------------------------------------------------------------------ owner-drawn buttons
fn is_sidebar(id: i32) -> bool {
    (IDC_L_ICON..=IDC_CANCEL).contains(&id)
}

fn draw_button(d: &DrawItemStruct) {
    let a = app();
    let id = d.CtlID as i32;
    let primary = id == IDC_UPLOAD;
    let flat = id == IDC_ALL || id == IDC_NONE || id == IDC_CLEAR;
    let disabled = d.itemState & ODS_DISABLED != 0;
    let pressed = d.itemState & ODS_SELECTED != 0;
    let parent_bg = if is_sidebar(id) { PANEL } else { BG };
    let mut bg = if primary {
        if pressed { ACCENT_HOVER } else { ACCENT }
    } else if flat {
        if pressed { CARD } else { parent_bg }
    } else if pressed {
        CARD_HOVER
    } else {
        CARD
    };
    let mut fg = if primary { rgb(255, 255, 255) } else if flat { MUTED } else { TEXTC };
    if disabled {
        if primary {
            bg = rgb(0x4A, 0x3E, 0x8C);
        }
        fg = if primary { rgb(0xB8, 0xB0, 0xE8) } else { BORDER };
    }
    unsafe {
        let dc = d.hDC as HDC;
        let mut rc = d.rcItem;
        let pb = CreateSolidBrush(parent_bg);
        FillRect(dc, &rc, pb);
        DeleteObject(pb as _);
        let b = CreateSolidBrush(bg);
        let ob = SelectObject(dc, b as _);
        let op = SelectObject(dc, GetStockObject(NULL_PEN));
        RoundRect(dc, rc.left, rc.top, rc.right + 1, rc.bottom + 1, s(10), s(10));
        SelectObject(dc, op);
        SelectObject(dc, ob);
        DeleteObject(b as _);
        let text = get_text(d.hwndItem);
        SetBkMode(dc, TRANSPARENT as _);
        SetTextColor(dc, fg);
        let of = SelectObject(dc, a.fonts[if primary { F_BIG } else if flat { F_SMALL } else { F_NORMAL }] as _);
        draw_text(dc, &text, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, of);
    }
}

// ------------------------------------------------------------------ layout
fn layout() {
    let a = app();
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(a.wnd, &mut rc) };
    let (w_, h) = (rc.right, rc.bottom);
    let sw = s(320);
    let pad = s(20);
    a.side_rect = RECT { left: 0, top: 0, right: sw, bottom: h };
    let mut dw = unsafe { BeginDeferWindowPos(40) };
    let mut place = |id: i32, x: i32, y: i32, cw: i32, ch: i32| unsafe {
        if !dw.is_null() {
            dw = DeferWindowPos(dw, ctl(id), null_mut(), x, y, cw.max(0), ch.max(0), SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    // ---- sidebar
    place(IDC_L_ICON, pad, pad, s(42), s(42));
    place(IDC_L_TITLE, pad + s(54), pad - s(2), sw - pad * 2 - s(54), s(28));
    place(IDC_L_SUB, pad + s(54), pad + s(26), sw - pad * 2 - s(54), s(18));
    let y = pad + s(62);
    place(IDC_L_UPLOADTO, pad, y + s(3), s(120), s(18));
    place(IDC_NONE, sw - pad - s(50), y, s(50), s(24));
    place(IDC_ALL, sw - pad - s(104), y, s(50), s(24));
    let mut bottom = h - pad;
    place(IDC_CANCEL, pad, bottom - s(36), sw - pad * 2, s(36));
    bottom -= s(36) + s(8);
    place(IDC_UPLOAD, pad, bottom - s(48), sw - pad * 2, s(48));
    bottom -= s(48) + s(10);
    place(IDC_SUMMARY, pad, bottom - s(20), sw - pad * 2, s(20));
    bottom -= s(20) + s(12);
    let (row_h, edit_w, ud_w, field_x) = (s(30), s(64), s(22), sw - pad - s(110));
    place(IDC_L_LITTER, pad, bottom - row_h + s(7), s(160), s(18));
    place(IDC_LITTER, field_x, bottom - row_h, s(110), s(200));
    bottom -= row_h + s(10);
    place(IDC_L_PAR, pad, bottom - row_h + s(7), s(160), s(18));
    place(IDC_PAR, sw - pad - edit_w - ud_w, bottom - row_h, edit_w, row_h);
    place(IDC_PAR_UD, sw - pad - ud_w, bottom - row_h, ud_w, row_h);
    bottom -= row_h + s(10);
    place(IDC_L_COPIES, pad, bottom - row_h + s(7), s(160), s(18));
    place(IDC_COPIES, sw - pad - edit_w - ud_w, bottom - row_h, edit_w, row_h);
    place(IDC_COPIES_UD, sw - pad - ud_w, bottom - row_h, ud_w, row_h);
    bottom -= row_h + s(14);
    let list_top = y + s(32);
    place(IDC_SITES, pad - s(6), list_top, sw - pad * 2 + s(12), bottom - list_top);

    // ---- main
    let x0 = sw + pad;
    let ww = w_ - x0 - pad;
    let mut yy = pad;
    place(IDC_DROP, x0, yy, ww, s(140));
    yy += s(140) + s(16);
    place(IDC_FILES_TITLE, x0, yy + s(4), ww / 2, s(18));
    place(IDC_CLEAR, x0 + ww - s(80), yy, s(80), s(26));
    place(IDC_ADDFOLDER, x0 + ww - s(80) - s(8) - s(100), yy, s(100), s(26));
    place(IDC_ADDFILES, x0 + ww - s(80) - s(16) - s(190), yy, s(90), s(26));
    yy += s(34);
    let (prog_h, links_head) = (s(70), s(38));
    let avail = h - pad - yy - prog_h - s(14) - links_head;
    let files_h = s(80).max(avail * 55 / 100);
    place(IDC_FILES, x0, yy, ww, files_h);
    yy += files_h + s(14);
    let prog_rect = RECT { left: x0, top: yy, right: x0 + ww, bottom: yy + prog_h };
    place(IDC_STATUS, x0 + s(16), yy + s(12), ww * 6 / 10 - s(16), s(20));
    place(IDC_STATS, x0 + ww * 6 / 10, yy + s(12), ww * 4 / 10 - s(16), s(20));
    place(IDC_BAR, x0 + s(16), yy + s(42), ww - s(32), s(10));
    yy += prog_h;
    place(IDC_LINKS_TITLE, x0, yy + s(18), s(120), s(18));
    place(IDC_SAVE, x0 + ww - s(96), yy + s(12), s(96), s(28));
    place(IDC_COPY, x0 + ww - s(96) - s(8) - s(96), yy + s(12), s(96), s(28));
    yy += links_head + s(6);
    place(IDC_LOG, x0, yy, ww, h - pad - yy);
    if !dw.is_null() {
        unsafe { EndDeferWindowPos(dw) };
    }
    app().prog_rect = prog_rect;

    // list columns
    let scroll = unsafe { GetSystemMetrics(SM_CXVSCROLL) };
    let fl = ctl(IDC_FILES);
    send(fl, LVM_SETCOLUMNWIDTH, 1, s(100) as isize);
    send(fl, LVM_SETCOLUMNWIDTH, 2, s(190) as isize);
    send(fl, LVM_SETCOLUMNWIDTH, 0, s(120).max(ww - s(100) - s(190) - scroll - s(4)) as isize);
    let sl = ctl(IDC_SITES);
    send(sl, LVM_SETCOLUMNWIDTH, 0, s(122) as isize);
    send(sl, LVM_SETCOLUMNWIDTH, 1, s(80).max(sw - pad * 2 + s(12) - s(122) - scroll - s(4)) as isize);
    unsafe { InvalidateRect(app().wnd, null(), TRUE) };
}

fn apply_fonts() {
    let a = app();
    let map = [
        (IDC_L_ICON, F_ICON), (IDC_L_TITLE, F_TITLE), (IDC_L_SUB, F_SMALL), (IDC_L_UPLOADTO, F_HEAD),
        (IDC_SITES, F_NORMAL), (IDC_L_COPIES, F_HEAD), (IDC_COPIES, F_BOLD), (IDC_L_PAR, F_HEAD), (IDC_PAR, F_BOLD),
        (IDC_L_LITTER, F_HEAD), (IDC_LITTER, F_NORMAL), (IDC_SUMMARY, F_SMALL), (IDC_FILES_TITLE, F_HEAD),
        (IDC_FILES, F_NORMAL), (IDC_STATUS, F_BOLD), (IDC_STATS, F_SMALL), (IDC_LINKS_TITLE, F_HEAD),
    ];
    for (id, f) in map {
        send(ctl(id), WM_SETFONT, a.fonts[f] as usize, 1);
    }
    // RichEdit font
    let mut cf: CharFormatW = unsafe { std::mem::zeroed() };
    cf.cbSize = std::mem::size_of::<CharFormatW>() as u32;
    cf.dwMask = CFM_FACE | CFM_SIZE | CFM_COLOR;
    cf.yHeight = 200; // 10pt in twips
    cf.crTextColor = TEXTC;
    for (i, c) in "Consolas".encode_utf16().enumerate() {
        cf.szFaceName[i] = c;
    }
    send(ctl(IDC_LOG), EM_SETCHARFORMAT, SCF_ALL, &cf as *const _ as isize);
    // taller rows in the lists
    unsafe {
        let il = ImageList_Create(1, s(28), 0x20 /* ILC_COLOR32 */, 1, 0);
        let old = send(ctl(IDC_SITES), LVM_SETIMAGELIST, LVSIL_SMALL, il as isize);
        if old != 0 {
            ImageList_Destroy(old as _);
        }
        let il2 = ImageList_Create(1, s(26), 0x20, 1, 0);
        let old = send(ctl(IDC_FILES), LVM_SETIMAGELIST, LVSIL_SMALL, il2 as isize);
        if old != 0 {
            ImageList_Destroy(old as _);
        }
    }
}

// ------------------------------------------------------------------ creation
fn mk(cls: &str, text: &str, style: u32, id: i32, ex: u32) -> HWND {
    unsafe {
        CreateWindowExW(
            ex,
            w(cls).as_ptr(),
            w(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0, 0, 10, 10,
            app().wnd,
            id as usize as HMENU,
            app().inst as _,
            null(),
        )
    }
}

fn set_theme(h: HWND, app_name: Option<&str>, id_list: Option<&str>) {
    let a = app_name.map(w);
    let b = id_list.map(w);
    unsafe {
        SetWindowTheme(h, a.as_ref().map_or(null(), |v| v.as_ptr()), b.as_ref().map_or(null(), |v| v.as_ptr()));
    }
}

fn style_list(lv: HWND, bg: u32) {
    set_theme(lv, Some("DarkMode_Explorer"), None);
    set_theme(lv_header(lv), Some("DarkMode_ItemsView"), None);
    send(lv, LVM_SETBKCOLOR, 0, bg as isize);
    send(lv, LVM_SETTEXTBKCOLOR, 0, bg as isize);
    send(lv, LVM_SETTEXTCOLOR, 0, TEXTC as isize);
    unsafe { SetWindowSubclass(lv, Some(list_subclass), 1, 0) };
}

fn create_controls() {
    mk("STATIC", "\u{21EA}", SS_CENTER | SS_CENTERIMAGE, IDC_L_ICON, 0);
    mk("STATIC", APP_NAME, SS_LEFT, IDC_L_TITLE, 0);
    mk("STATIC", "One drop, many links", SS_LEFT, IDC_L_SUB, 0);
    mk("STATIC", "UPLOAD TO", SS_LEFT, IDC_L_UPLOADTO, 0);
    mk("BUTTON", "All", BS_OWNERDRAW, IDC_ALL, 0);
    mk("BUTTON", "None", BS_OWNERDRAW, IDC_NONE, 0);

    let sl = mk("SysListView32", "", LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SINGLESEL | WS_TABSTOP, IDC_SITES, 0);
    send(sl, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
    style_list(sl, PANEL);
    lv_insert_column(sl, 0, "Site", 120);
    lv_insert_column(sl, 1, "Limits", 120);
    for (i, site) in SITES.iter().enumerate() {
        lv_insert_item(sl, i as i32, site.name);
        lv_set_text(sl, i as i32, 1, site.tag);
    }

    mk("STATIC", "COPIES PER FILE", SS_LEFT, IDC_L_COPIES, 0);
    mk("EDIT", "1", ES_CENTER | ES_NUMBER | WS_TABSTOP, IDC_COPIES, 0);
    let ud = mk("msctls_updown32", "", UDS_ARROWKEYS | UDS_NOTHOUSANDS | UDS_SETBUDDYINT, IDC_COPIES_UD, 0);
    send(ud, UDM_SETBUDDY, ctl(IDC_COPIES) as usize, 0);
    send(ud, UDM_SETRANGE32, 1, 1000);
    send(ctl(IDC_COPIES), EM_SETLIMITTEXT, 4, 0);

    mk("STATIC", "PARALLEL UPLOADS", SS_LEFT, IDC_L_PAR, 0);
    mk("EDIT", "4", ES_CENTER | ES_NUMBER | WS_TABSTOP, IDC_PAR, 0);
    let ud2 = mk("msctls_updown32", "", UDS_ARROWKEYS | UDS_NOTHOUSANDS | UDS_SETBUDDYINT, IDC_PAR_UD, 0);
    send(ud2, UDM_SETBUDDY, ctl(IDC_PAR) as usize, 0);
    send(ud2, UDM_SETRANGE32, 1, 16);
    send(ctl(IDC_PAR), EM_SETLIMITTEXT, 2, 0);

    mk("STATIC", "LITTERBOX EXPIRY", SS_LEFT, IDC_L_LITTER, 0);
    let cb = mk("ComboBox", "", CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP, IDC_LITTER, 0);
    set_theme(cb, Some("DarkMode_CFD"), None);
    for t in ["1 hour", "12 hours", "24 hours", "72 hours"] {
        let wt = w(t);
        send(cb, CB_ADDSTRING, 0, wt.as_ptr() as isize);
    }
    send(cb, CB_SETCURSEL, 3, 0);

    mk("STATIC", "", SS_LEFT | SS_ENDELLIPSIS, IDC_SUMMARY, 0);
    mk("BUTTON", "Upload", BS_OWNERDRAW | WS_TABSTOP, IDC_UPLOAD, 0);
    mk("BUTTON", "Cancel", BS_OWNERDRAW | WS_TABSTOP | WS_DISABLED, IDC_CANCEL, 0);

    // main side
    mk("MUDropZone", "", 0, IDC_DROP, 0);
    mk("STATIC", "FILES", SS_LEFT, IDC_FILES_TITLE, 0);
    mk("BUTTON", "Add files", BS_OWNERDRAW, IDC_ADDFILES, 0);
    mk("BUTTON", "Add folder", BS_OWNERDRAW, IDC_ADDFOLDER, 0);
    mk("BUTTON", "Clear all", BS_OWNERDRAW, IDC_CLEAR, 0);
    let fl = mk("SysListView32", "", LVS_REPORT | LVS_SHOWSELALWAYS | WS_TABSTOP, IDC_FILES, 0);
    send(fl, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
    style_list(fl, PANEL);
    lv_insert_column(fl, 0, "File", 120);
    lv_insert_column(fl, 1, "Size", 120);
    lv_insert_column(fl, 2, "Status", 120);

    mk("STATIC", "Ready to upload", SS_LEFT | SS_ENDELLIPSIS, IDC_STATUS, 0);
    mk("STATIC", "", SS_RIGHT | SS_ENDELLIPSIS, IDC_STATS, 0);
    let bar = mk("msctls_progress32", "", PBS_SMOOTH, IDC_BAR, 0);
    set_theme(bar, Some(""), Some(""));
    send(bar, PBM_SETRANGE32, 0, 10000);
    send(bar, PBM_SETBARCOLOR, 0, ACCENT as isize);
    send(bar, PBM_SETBKCOLOR, 0, CARD as isize);

    mk("STATIC", "LINKS", SS_LEFT, IDC_LINKS_TITLE, 0);
    mk("BUTTON", "Copy all", BS_OWNERDRAW | WS_DISABLED, IDC_COPY, 0);
    mk("BUTTON", "Save .txt", BS_OWNERDRAW | WS_DISABLED, IDC_SAVE, 0);
    let re = mk(
        "RICHEDIT50W",
        "",
        WS_VSCROLL | WS_HSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | ES_AUTOHSCROLL | ES_NOHIDESEL,
        IDC_LOG,
        0,
    );
    set_theme(re, Some("DarkMode_Explorer"), None);
    send(re, EM_SETBKGNDCOLOR, 0, PANEL as isize);
    send(re, EM_EXLIMITTEXT, 0, 64 * 1024 * 1024);
}

// ------------------------------------------------------------------ window proc
unsafe extern "system" fn wnd_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_CREATE => {
            app().wnd = h;
            app().dpi = dpi_for_window(h);
            dark_title_bar(h);
            create_fonts();
            create_controls();
            apply_fonts();
            load_settings();
            app().initializing = false;
            update_files_title();
            layout();
            DragAcceptFiles(h, TRUE);
            0
        }
        WM_SIZE => {
            if wp != 1 /* SIZE_MINIMIZED */ {
                layout();
            }
            0
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lp as *mut MinMaxInfo);
            if !APP.is_null() && !app().wnd.is_null() {
                mm.ptMinTrackSize.x = s(980);
                mm.ptMinTrackSize.y = s(660);
            }
            0
        }
        WM_DPICHANGED => {
            app().dpi = hiword(wp);
            create_fonts();
            apply_fonts();
            let r = &*(lp as *const RECT);
            SetWindowPos(h, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            layout();
            0
        }
        WM_ERASEBKGND => {
            let a = app();
            let dc = wp as HDC;
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(h, &mut rc);
            FillRect(dc, &rc, a.br_bg as _);
            FillRect(dc, &a.side_rect, a.br_panel as _);
            let ob = SelectObject(dc, a.br_panel as _);
            let op = SelectObject(dc, GetStockObject(NULL_PEN));
            let p = a.prog_rect;
            RoundRect(dc, p.left, p.top, p.right + 1, p.bottom + 1, s(16), s(16));
            SelectObject(dc, ob);
            SelectObject(dc, op);
            1
        }
        WM_CTLCOLORSTATIC => {
            let a = app();
            let dc = wp as HDC;
            let id = GetDlgCtrlID(lp as HWND);
            if id == IDC_L_ICON {
                SetTextColor(dc, rgb(255, 255, 255));
                SetBkColor(dc, ACCENT);
                return a.br_accent as LRESULT;
            }
            let side = is_sidebar(id);
            let prog = id == IDC_STATUS || id == IDC_STATS;
            let bg = if side || prog { PANEL } else { BG };
            let fg = if id == IDC_L_TITLE || id == IDC_STATUS { TEXTC } else { MUTED };
            SetTextColor(dc, fg);
            SetBkColor(dc, bg);
            (if side || prog { a.br_panel } else { a.br_bg }) as LRESULT
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let dc = wp as HDC;
            SetTextColor(dc, TEXTC);
            SetBkColor(dc, CARD);
            app().br_card as LRESULT
        }
        WM_DRAWITEM => {
            let d = &*(lp as *const DrawItemStruct);
            if d.CtlType == ODT_BUTTON {
                draw_button(d);
                return 1;
            }
            DefWindowProcW(h, m, wp, lp)
        }
        WM_COMMAND => {
            let id = loword(wp) as i32;
            let code = hiword(wp);
            match id {
                IDC_UPLOAD => start_upload(),
                IDC_CANCEL => {
                    app().engine.cancel();
                    enable(IDC_CANCEL, false);
                    set_text(ctl(IDC_CANCEL), "Cancelling\u{2026}");
                }
                IDC_ADDFILES => browse(false),
                IDC_ADDFOLDER => browse(true),
                IDC_CLEAR => clear_files(),
                IDC_COPY => copy_links(),
                IDC_SAVE => save_links(),
                IDC_ALL | IDC_NONE => {
                    let lv = ctl(IDC_SITES);
                    for i in 0..SITES.len() {
                        lv_set_check(lv, i as i32, id == IDC_ALL);
                    }
                    update_summary();
                }
                IDC_COPIES => {
                    if code == EN_CHANGE {
                        update_summary();
                    }
                }
                _ => {}
            }
            0
        }
        WM_NOTIFY => {
            let nh = &*(lp as *const NmHdr);
            if nh.idFrom == IDC_SITES as usize {
                if nh.code == LVN_ITEMCHANGED {
                    let nl = &*(lp as *const NmListView);
                    if (nl.uChanged & LVIF_STATE) != 0 && ((nl.uNewState ^ nl.uOldState) & LVIS_STATEIMAGEMASK) != 0 {
                        update_summary();
                    }
                } else if nh.code == NM_CLICK || nh.code == NM_DBLCLK {
                    let ia = &*(lp as *const NmItemActivate);
                    let mut ht: LvHitTestInfo = std::mem::zeroed();
                    ht.pt = ia.ptAction;
                    let item = send(nh.hwndFrom, LVM_SUBITEMHITTEST, 0, &mut ht as *mut _ as isize) as i32;
                    if item >= 0 && (ht.flags & LVHT_ONITEMSTATEICON) == 0 && nh.code == NM_CLICK {
                        let on = lv_get_check(nh.hwndFrom, item);
                        lv_set_check(nh.hwndFrom, item, !on);
                    }
                }
            } else if nh.idFrom == IDC_FILES as usize && nh.code == LVN_GETEMPTYMARKUP {
                let em = &mut *(lp as *mut NmLvEmptyMarkup);
                em.dwFlags = EMF_CENTERED;
                let t: Vec<u16> = "No files yet\0".encode_utf16().collect();
                em.szMarkup[..t.len()].copy_from_slice(&t);
                return 1;
            }
            DefWindowProcW(h, m, wp, lp)
        }
        WM_DROPFILES => {
            let drop = wp as _;
            let mut paths = Vec::new();
            let n = DragQueryFileW(drop, 0xFFFF_FFFF, null_mut(), 0);
            for i in 0..n {
                let len = DragQueryFileW(drop, i, null_mut(), 0);
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, buf.as_mut_ptr(), len + 1);
                paths.push(path_from_wide(&buf[..len as usize]));
            }
            DragFinish(drop);
            add_paths(paths);
            0
        }
        WM_ENGINE => {
            let e = Box::from_raw(lp as *mut Event);
            on_engine(*e);
            0
        }
        WM_TIMER => {
            match wp {
                TIMER_PROGRESS => {
                    if app().engine.running() {
                        update_progress();
                    }
                }
                TIMER_AUTOSAVE => {
                    KillTimer(h, TIMER_AUTOSAVE);
                    save_links();
                }
                TIMER_COPIED => {
                    KillTimer(h, TIMER_COPIED);
                    set_text(ctl(IDC_COPY), "Copy all");
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            if app().engine.running() {
                if message_box("Uploads are still running. Cancel them and quit?", APP_NAME, MB_YESNO | MB_ICONQUESTION) != IDYES {
                    return 0;
                }
                app().engine.cancel();
                app().engine.join();
            }
            save_settings();
            DestroyWindow(h);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, wp, lp),
    }
}

// ------------------------------------------------------------------ entry
fn main() {
    unsafe {
        let inst = GetModuleHandleW(null()) as Handle;
        APP = Box::into_raw(Box::new(App {
            inst,
            wnd: null_mut(),
            dpi: 96,
            fonts: [null_mut(); 8],
            br_bg: CreateSolidBrush(BG) as Handle,
            br_panel: CreateSolidBrush(PANEL) as Handle,
            br_card: CreateSolidBrush(CARD) as Handle,
            br_accent: CreateSolidBrush(ACCENT) as Handle,
            files: Vec::new(),
            file_set: HashSet::new(),
            results: Vec::new(),
            engine: Engine::default(),
            last_copies: 1,
            last_litter: "72h".into(),
            initializing: true,
            drop_hover: false,
            prog_rect: std::mem::zeroed(),
            side_rect: std::mem::zeroed(),
            started: Instant::now(),
            speed: VecDeque::new(),
        }));

        CoInitializeEx(null(), 0x2 | 0x4 /* COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE */);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_PROGRESS_CLASS | ICC_UPDOWN_CLASS | ICC_STANDARD_CLASSES | ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&icc);
        LoadLibraryW(w("Msftedit.dll").as_ptr());
        allow_dark_app();

        // allow drag & drop from Explorer even when running elevated
        ChangeWindowMessageFilter(WM_DROPFILES, MSGFLT_ADD);
        ChangeWindowMessageFilter(WM_COPYDATA, MSGFLT_ADD);
        ChangeWindowMessageFilter(WM_COPYGLOBALDATA, MSGFLT_ADD);

        let cls = w("MultiUploaderWnd");
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = inst as _;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(inst as _, 1usize as *const u16);
        wc.hIconSm = LoadImageW(inst as _, 1usize as *const u16, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), 0) as _;
        wc.hbrBackground = app().br_bg as _;
        wc.lpszClassName = cls.as_ptr();
        RegisterClassExW(&wc);

        let dcls = w("MUDropZone");
        let mut dc: WNDCLASSEXW = std::mem::zeroed();
        dc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        dc.lpfnWndProc = Some(drop_proc);
        dc.hInstance = inst as _;
        dc.hCursor = LoadCursorW(null_mut(), IDC_HAND);
        dc.lpszClassName = dcls.as_ptr();
        RegisterClassExW(&dc);

        app().dpi = dpi_for_window(null_mut());
        let title = w(APP_NAME);
        let wnd = CreateWindowExW(
            WS_EX_ACCEPTFILES,
            cls.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            s(1140),
            s(760),
            null_mut(),
            null_mut(),
            inst as _,
            null(),
        );
        if wnd.is_null() {
            std::process::exit(1);
        }

        // files passed on the command line / "Send to" / dropped onto the exe
        let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
        add_paths(paths);

        ShowWindow(wnd, SW_SHOWDEFAULT);
        UpdateWindow(wnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        CoUninitialize();
        std::process::exit(msg.wParam as i32);
    }
}
