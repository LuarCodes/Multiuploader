//! Win32 constants, struct layouts and common-control helpers.
//! Spelled out explicitly (instead of relying on header-version-specific
//! bindings) so the layout matches the C headers exactly.
#![allow(dead_code, non_snake_case)]

pub use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, SetWindowTextW, GetWindowTextW, GetWindowTextLengthW};
use crate::util::w;
use std::ffi::c_void;

pub type Handle = *mut c_void;

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub fn muldiv(a: i32, b: i32, c: i32) -> i32 {
    if c == 0 {
        return -1;
    }
    let p = a as i64 * b as i64;
    let half = (c as i64).abs() / 2;
    (if (p < 0) != (c < 0) { (p - half) / c as i64 } else { (p + half) / c as i64 }) as i32
}

pub fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(h, msg, wp, lp) }
}

pub fn set_text(h: HWND, s: &str) {
    let ws = w(s);
    unsafe { SetWindowTextW(h, ws.as_ptr()) };
}

pub fn get_text(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

pub fn loword(x: usize) -> u32 { (x & 0xFFFF) as u32 }
pub fn hiword(x: usize) -> u32 { ((x >> 16) & 0xFFFF) as u32 }

// ---------------------------------------------------------------- messages
pub const WM_USER: u32 = 0x0400;
pub const WM_APP: u32 = 0x8000;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_COPYGLOBALDATA: u32 = 0x0049;

pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_REPLACESEL: u32 = 0x00C2;
pub const EM_SETLIMITTEXT: u32 = 0x00C5;
pub const EM_SETREADONLY: u32 = 0x00CF;
pub const EM_EXLIMITTEXT: u32 = WM_USER + 53;
pub const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
pub const EM_SETCHARFORMAT: u32 = WM_USER + 68;
pub const SCF_SELECTION: usize = 1;
pub const SCF_ALL: usize = 4;
pub const CFM_SIZE: u32 = 0x8000_0000;
pub const CFM_FACE: u32 = 0x2000_0000;
pub const CFM_COLOR: u32 = 0x4000_0000;
pub const EN_CHANGE: u32 = 0x0300;

pub const LVM_FIRST: u32 = 0x1000;
pub const LVM_SETBKCOLOR: u32 = LVM_FIRST + 1;
pub const LVM_SETIMAGELIST: u32 = LVM_FIRST + 3;
pub const LVM_DELETEITEM: u32 = LVM_FIRST + 8;
pub const LVM_DELETEALLITEMS: u32 = LVM_FIRST + 9;
pub const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
pub const LVM_SETCOLUMNWIDTH: u32 = LVM_FIRST + 30;
pub const LVM_GETHEADER: u32 = LVM_FIRST + 31;
pub const LVM_SETTEXTCOLOR: u32 = LVM_FIRST + 36;
pub const LVM_SETTEXTBKCOLOR: u32 = LVM_FIRST + 38;
pub const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
pub const LVM_GETITEMSTATE: u32 = LVM_FIRST + 44;
pub const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
pub const LVM_SUBITEMHITTEST: u32 = LVM_FIRST + 57;
pub const LVM_INSERTITEMW: u32 = LVM_FIRST + 77;
pub const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
pub const LVM_SETITEMTEXTW: u32 = LVM_FIRST + 116;

pub const LVS_REPORT: u32 = 0x0001;
pub const LVS_SINGLESEL: u32 = 0x0004;
pub const LVS_SHOWSELALWAYS: u32 = 0x0008;
pub const LVS_NOCOLUMNHEADER: u32 = 0x4000;
pub const LVS_EX_CHECKBOXES: isize = 0x0004;
pub const LVS_EX_FULLROWSELECT: isize = 0x0020;
pub const LVS_EX_DOUBLEBUFFER: isize = 0x0001_0000;
pub const LVIF_TEXT: u32 = 0x0001;
pub const LVIF_STATE: u32 = 0x0008;
pub const LVIS_STATEIMAGEMASK: u32 = 0xF000;
pub const LVNI_SELECTED: isize = 0x0002;
pub const LVCF_WIDTH: u32 = 0x0002;
pub const LVCF_TEXT: u32 = 0x0004;
pub const LVSIL_SMALL: usize = 1;
pub const LVHT_ONITEMSTATEICON: u32 = 0x0008;

pub const NM_CLICK: u32 = (-2i32) as u32;
pub const NM_DBLCLK: u32 = (-3i32) as u32;
pub const NM_CUSTOMDRAW: u32 = (-12i32) as u32;
pub const LVN_ITEMCHANGED: u32 = (-101i32) as u32;
pub const LVN_GETEMPTYMARKUP: u32 = (-187i32) as u32;
pub const CDDS_PREPAINT: u32 = 0x0001;
pub const CDDS_ITEMPREPAINT: u32 = 0x0001_0001;
pub const CDRF_DODEFAULT: isize = 0x0;
pub const CDRF_NOTIFYITEMDRAW: isize = 0x20;
pub const EMF_CENTERED: u32 = 0x1;

pub const UDM_SETBUDDY: u32 = WM_USER + 105;
pub const UDM_SETRANGE32: u32 = WM_USER + 111;
pub const UDS_SETBUDDYINT: u32 = 0x0002;
pub const UDS_ARROWKEYS: u32 = 0x0020;
pub const UDS_NOTHOUSANDS: u32 = 0x0080;

pub const PBM_SETPOS: u32 = WM_USER + 2;
pub const PBM_SETRANGE32: u32 = WM_USER + 6;
pub const PBM_SETBARCOLOR: u32 = WM_USER + 9;
pub const PBM_SETBKCOLOR: u32 = 0x2001;
pub const PBS_SMOOTH: u32 = 0x01;

pub const CB_ADDSTRING: u32 = 0x0143;
pub const CB_GETCURSEL: u32 = 0x0147;
pub const CB_SETCURSEL: u32 = 0x014E;
pub const CBS_DROPDOWNLIST: u32 = 0x0003;

pub const SS_LEFT: u32 = 0x0000;
pub const SS_CENTER: u32 = 0x0001;
pub const SS_RIGHT: u32 = 0x0002;
pub const SS_CENTERIMAGE: u32 = 0x0200;
pub const SS_ENDELLIPSIS: u32 = 0x4000;
pub const ES_CENTER: u32 = 0x0001;
pub const ES_MULTILINE: u32 = 0x0004;
pub const ES_AUTOVSCROLL: u32 = 0x0040;
pub const ES_AUTOHSCROLL: u32 = 0x0080;
pub const ES_NOHIDESEL: u32 = 0x0100;
pub const ES_READONLY: u32 = 0x0800;
pub const ES_NUMBER: u32 = 0x2000;
pub const BS_OWNERDRAW: u32 = 0x000B;

pub const ODT_BUTTON: u32 = 4;
pub const ODS_SELECTED: u32 = 0x0001;
pub const ODS_DISABLED: u32 = 0x0004;

pub const ICC_LISTVIEW_CLASSES: u32 = 0x0001;
pub const ICC_BAR_CLASSES: u32 = 0x0004;
pub const ICC_UPDOWN_CLASS: u32 = 0x0010;
pub const ICC_PROGRESS_CLASS: u32 = 0x0020;
pub const ICC_STANDARD_CLASSES: u32 = 0x4000;

pub const DT_CENTER: u32 = 0x01;
pub const DT_VCENTER: u32 = 0x04;
pub const DT_SINGLELINE: u32 = 0x20;

// ---------------------------------------------------------------- structs
#[repr(C)]
pub struct LvItemW {
    pub mask: u32,
    pub iItem: i32,
    pub iSubItem: i32,
    pub state: u32,
    pub stateMask: u32,
    pub pszText: *mut u16,
    pub cchTextMax: i32,
    pub iImage: i32,
    pub lParam: LPARAM,
    pub iIndent: i32,
    pub iGroupId: i32,
    pub cColumns: u32,
    pub puColumns: *mut u32,
    pub piColFmt: *mut i32,
    pub iGroup: i32,
}

#[repr(C)]
pub struct LvColumnW {
    pub mask: u32,
    pub fmt: i32,
    pub cx: i32,
    pub pszText: *mut u16,
    pub cchTextMax: i32,
    pub iSubItem: i32,
    pub iImage: i32,
    pub iOrder: i32,
    pub cxMin: i32,
    pub cxDefault: i32,
    pub cxIdeal: i32,
}

#[repr(C)]
pub struct NmHdr {
    pub hwndFrom: HWND,
    pub idFrom: usize,
    pub code: u32,
}

#[repr(C)]
pub struct NmListView {
    pub hdr: NmHdr,
    pub iItem: i32,
    pub iSubItem: i32,
    pub uNewState: u32,
    pub uOldState: u32,
    pub uChanged: u32,
    pub ptAction: POINT,
    pub lParam: LPARAM,
}

#[repr(C)]
pub struct NmItemActivate {
    pub hdr: NmHdr,
    pub iItem: i32,
    pub iSubItem: i32,
    pub uNewState: u32,
    pub uOldState: u32,
    pub uChanged: u32,
    pub ptAction: POINT,
    pub lParam: LPARAM,
    pub uKeyFlags: u32,
}

#[repr(C)]
pub struct LvHitTestInfo {
    pub pt: POINT,
    pub flags: u32,
    pub iItem: i32,
    pub iSubItem: i32,
    pub iGroup: i32,
}

#[repr(C)]
pub struct NmCustomDraw {
    pub hdr: NmHdr,
    pub dwDrawStage: u32,
    pub hdc: Handle,
    pub rc: RECT,
    pub dwItemSpec: usize,
    pub uItemState: u32,
    pub lItemlParam: LPARAM,
}

#[repr(C)]
pub struct NmLvEmptyMarkup {
    pub hdr: NmHdr,
    pub dwFlags: u32,
    pub szMarkup: [u16; 2084],
}

#[repr(C)]
pub struct DrawItemStruct {
    pub CtlType: u32,
    pub CtlID: u32,
    pub itemID: u32,
    pub itemAction: u32,
    pub itemState: u32,
    pub hwndItem: HWND,
    pub hDC: Handle,
    pub rcItem: RECT,
    pub itemData: usize,
}

#[repr(C)]
pub struct CharFormatW {
    pub cbSize: u32,
    pub dwMask: u32,
    pub dwEffects: u32,
    pub yHeight: i32,
    pub yOffset: i32,
    pub crTextColor: u32,
    pub bCharSet: u8,
    pub bPitchAndFamily: u8,
    pub szFaceName: [u16; 32],
}

#[repr(C)]
pub struct MinMaxInfo {
    pub ptReserved: POINT,
    pub ptMaxSize: POINT,
    pub ptMaxPosition: POINT,
    pub ptMinTrackSize: POINT,
    pub ptMaxTrackSize: POINT,
}

// ---------------------------------------------------------------- list-view helpers
pub fn lv_set_check(lv: HWND, i: i32, on: bool) {
    let mut it: LvItemW = unsafe { std::mem::zeroed() };
    it.stateMask = LVIS_STATEIMAGEMASK;
    it.state = (if on { 2 } else { 1 }) << 12;
    send(lv, LVM_SETITEMSTATE, i as usize, &mut it as *mut _ as isize);
}

pub fn lv_get_check(lv: HWND, i: i32) -> bool {
    let st = send(lv, LVM_GETITEMSTATE, i as usize, LVIS_STATEIMAGEMASK as isize) as u32;
    ((st >> 12).wrapping_sub(1)) != 0 && (st >> 12) != 0
}

pub fn lv_insert_item(lv: HWND, i: i32, text: &str) -> i32 {
    let mut t = w(text);
    let mut it: LvItemW = unsafe { std::mem::zeroed() };
    it.mask = LVIF_TEXT;
    it.iItem = i;
    it.pszText = t.as_mut_ptr();
    send(lv, LVM_INSERTITEMW, 0, &mut it as *mut _ as isize) as i32
}

pub fn lv_set_text(lv: HWND, i: i32, sub: i32, text: &str) {
    let mut t = w(text);
    let mut it: LvItemW = unsafe { std::mem::zeroed() };
    it.iSubItem = sub;
    it.pszText = t.as_mut_ptr();
    send(lv, LVM_SETITEMTEXTW, i as usize, &mut it as *mut _ as isize);
}

pub fn lv_insert_column(lv: HWND, i: i32, text: &str, cx: i32) {
    let mut t = w(text);
    let mut col: LvColumnW = unsafe { std::mem::zeroed() };
    col.mask = LVCF_TEXT | LVCF_WIDTH;
    col.cx = cx;
    col.pszText = t.as_mut_ptr();
    send(lv, LVM_INSERTCOLUMNW, i as usize, &mut col as *mut _ as isize);
}

pub fn lv_header(lv: HWND) -> HWND {
    send(lv, LVM_GETHEADER, 0, 0) as HWND
}
