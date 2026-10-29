//! One streaming multipart/raw upload over WinHTTP.

use crate::json;
use crate::sites::{Body, Reply, Site};
use crate::util::{path_from_wide, w};
use std::cell::Cell;
use std::ffi::c_void;
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::System::Diagnostics::Debug::FormatMessageW;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

pub type HInternet = *mut c_void;

const USER_AGENT: &str = "MultiUploader/3.0 (native; Rust)";
const CHUNK: usize = 1 << 20; // 1 MB writes: few syscalls, full TCP window

// WinHTTP constants (spelled out so we don't depend on header versions)
const ACCESS_DEFAULT_PROXY: u32 = 0;
const ACCESS_AUTOMATIC_PROXY: u32 = 4;
const OPT_SECURE_PROTOCOLS: u32 = 84;
const OPT_MAX_CONNS_PER_SERVER: u32 = 73;
const PROT_TLS1_1: u32 = 0x200;
const PROT_TLS1_2: u32 = 0x800;
const PROT_TLS1_3: u32 = 0x2000;
const FLAG_SECURE: u32 = 0x0080_0000;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const IGNORE_REQUEST_TOTAL_LENGTH: u32 = 0;

const E_TIMEOUT: u32 = 12002;
const E_NAME_NOT_RESOLVED: u32 = 12007;
const E_CANNOT_CONNECT: u32 = 12029;
const E_CONNECTION_ERROR: u32 = 12030;
const E_RESEND_REQUEST: u32 = 12032;
const E_INVALID_SERVER_RESPONSE: u32 = 12152;

#[derive(Clone)]
pub struct Options {
    pub litter_time: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { litter_time: "72h".into() }
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct Outcome {
    pub link: String, // empty on failure
    pub error: String,
    pub retryable: bool,
    pub cancelled: bool,
}

/// Shared cancel switch. Closing a live WinHTTP request handle from another
/// thread aborts a blocking send/receive immediately.
#[derive(Default)]
pub struct CancelToken {
    pub flag: AtomicBool,
    live: Mutex<Vec<usize>>,
}

impl CancelToken {
    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    fn add(&self, h: HInternet) {
        self.live.lock().unwrap().push(h as usize);
    }
    /// true -> caller still owns the handle and must close it
    fn remove(&self, h: HInternet) -> bool {
        let mut v = self.live.lock().unwrap();
        if let Some(i) = v.iter().position(|&x| x == h as usize) {
            v.remove(i);
            true
        } else {
            false
        }
    }
    pub fn cancel_all(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let mut v = self.live.lock().unwrap();
        for &h in v.iter() {
            unsafe { WinHttpCloseHandle(h as HInternet) };
        }
        v.clear();
    }
}

// ------------------------------------------------------------ helpers
fn seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(d.as_nanos());
    }
    h.write(format!("{:?}", std::thread::current().id()).as_bytes());
    h.finish() | 1
}

fn next_rand() -> u64 {
    thread_local!(static STATE: Cell<u64> = Cell::new(seed()));
    STATE.with(|s| {
        // xorshift64*
        let mut x = s.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

pub fn random_string(n: usize, alphabet: &[u8]) -> String {
    (0..n)
        .map(|_| alphabet[(next_rand() % alphabet.len() as u64) as usize] as char)
        .collect()
}

const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn make_pad() -> String {
    format!("\n{}\n", random_string(16, ALNUM))
}

pub fn win_err(code: u32) -> String {
    let mut buf = [0u16; 512];
    let mut n = 0;
    unsafe {
        if (12000..=12200).contains(&code) {
            let m = GetModuleHandleW(w("winhttp.dll").as_ptr());
            if !m.is_null() {
                n = FormatMessageW(0x800 | 0x200, m as _, code, 0, buf.as_mut_ptr(), buf.len() as u32, std::ptr::null());
            }
        }
        if n == 0 {
            n = FormatMessageW(0x1000 | 0x200, std::ptr::null(), code, 0, buf.as_mut_ptr(), buf.len() as u32, std::ptr::null());
        }
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    let s = s.trim_end_matches(['\r', '\n', ' ', '.']);
    if s.is_empty() {
        format!("Error {}", code)
    } else {
        format!("Error {} - {}", code, s)
    }
}

fn retryable_net(e: u32) -> bool {
    matches!(e, E_TIMEOUT | E_CONNECTION_ERROR | E_CANNOT_CONNECT | E_NAME_NOT_RESOLVED | E_RESEND_REQUEST | E_INVALID_SERVER_RESPONSE)
}

fn retryable_status(s: u32) -> bool {
    matches!(s, 408 | 429 | 500 | 502 | 503 | 504 | 520..=524)
}

fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            o.push(c as char);
        } else {
            o.push_str(&format!("%{:02X}", c));
        }
    }
    o
}

fn header_safe(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("%22"),
            '\r' | '\n' => {}
            _ => o.push(c),
        }
    }
    o
}

fn first_line(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    s.trim_start().lines().next().unwrap_or("").trim_end().to_string()
}

fn short_body(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    let mut o = String::new();
    for word in s.split_whitespace() {
        if !o.is_empty() {
            o.push(' ');
        }
        o.push_str(word);
        if o.chars().count() >= 250 {
            o = o.chars().take(250).collect();
            break;
        }
    }
    if o.is_empty() {
        "empty reply".into()
    } else {
        o
    }
}

/// "https://host[:port]/path?query" -> (secure, host, port, object)
fn split_url(url: &str) -> Option<(bool, String, u16, String)> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let (hostport, object) = rest.split_at(cut);
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (hostport, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return None;
    }
    let object = if object.is_empty() {
        "/".to_string()
    } else if object.starts_with('?') {
        format!("/{}", object)
    } else {
        object.to_string()
    };
    Some((secure, host.to_string(), port, object))
}

pub fn open_session() -> HInternet {
    unsafe {
        let ua = w(USER_AGENT);
        let mut s = WinHttpOpen(ua.as_ptr(), ACCESS_AUTOMATIC_PROXY as _, std::ptr::null(), std::ptr::null(), 0);
        if s.is_null() {
            s = WinHttpOpen(ua.as_ptr(), ACCESS_DEFAULT_PROXY as _, std::ptr::null(), std::ptr::null(), 0);
        }
        if s.is_null() {
            return s;
        }
        // resolve, connect, send, receive (receive is long: hosts may process big files before replying)
        WinHttpSetTimeouts(s, 20000, 20000, 120000, 1800000);
        let mut prot: u32 = PROT_TLS1_2 | PROT_TLS1_3;
        if WinHttpSetOption(s, OPT_SECURE_PROTOCOLS, &prot as *const u32 as _, 4) == 0 {
            prot = PROT_TLS1_2 | PROT_TLS1_1;
            WinHttpSetOption(s, OPT_SECURE_PROTOCOLS, &prot as *const u32 as _, 4);
        }
        let max_conns: u32 = 8;
        WinHttpSetOption(s, OPT_MAX_CONNS_PER_SERVER, &max_conns as *const u32 as _, 4);
        s
    }
}

pub fn close_handle(h: HInternet) {
    if !h.is_null() {
        unsafe { WinHttpCloseHandle(h) };
    }
}

struct InetGuard(HInternet);
impl Drop for InetGuard {
    fn drop(&mut self) {
        close_handle(self.0);
    }
}

struct ReqGuard<'a> {
    h: HInternet,
    c: &'a CancelToken,
}
impl Drop for ReqGuard<'_> {
    fn drop(&mut self) {
        if self.c.remove(self.h) {
            close_handle(self.h);
        }
    }
}

fn cancelled() -> Outcome {
    Outcome { cancelled: true, error: "Cancelled".into(), ..Default::default() }
}

fn fail(error: String, retryable: bool) -> Outcome {
    Outcome { error, retryable, ..Default::default() }
}

/// Uploads one file to one site. `progress` is a shared byte counter; every
/// file byte written is added to it and to `added` (so callers can roll back).
#[allow(clippy::too_many_arguments)]
pub fn upload_once(
    session: HInternet,
    site: &Site,
    path: &[u16],
    file_name: &str,
    file_size: u64,
    opt: &Options,
    pad: &str,
    progress: &AtomicI64,
    added: &mut i64,
    cancel: &CancelToken,
) -> Outcome {
    if cancel.is_set() {
        return cancelled();
    }
    if session.is_null() {
        return fail("Could not start WinHTTP".into(), false);
    }

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2 | 0x4) // FILE_SHARE_READ | WRITE | DELETE
        .custom_flags(0x0800_0000) // FILE_FLAG_SEQUENTIAL_SCAN
        .open(path_from_wide(path))
    {
        Ok(f) => f,
        Err(e) => return fail(format!("Cannot open file: {}", win_err(e.raw_os_error().unwrap_or(0) as u32)), false),
    };

    let (url, pre, post, ctype) = if site.body == Body::Multipart {
        let b = format!("----MultiUploader{}", random_string(24, ALNUM));
        let mut pre = String::new();
        let mut add_field = |k: &str, v: &str| {
            pre += &format!("--{b}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n");
        };
        for (k, v) in site.extra {
            add_field(k, v);
        }
        if site.key == "litterbox" {
            add_field("time", &opt.litter_time);
        }
        pre += &format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            site.field,
            header_safe(file_name)
        );
        let post = format!("\r\n--{b}--\r\n");
        (site.endpoint().into_owned(), pre, post, format!("multipart/form-data; boundary={b}"))
    } else {
        let bin = format!("mu{}", random_string(14, b"abcdefghijklmnopqrstuvwxyz0123456789"));
        let safe = file_name.replace(' ', "_");
        (format!("{}{}/{}", site.endpoint(), bin, url_encode(&safe)), String::new(), String::new(), "application/octet-stream".to_string())
    };
    let total: u64 = pre.len() as u64 + file_size + pad.len() as u64 + post.len() as u64;

    let Some((secure, host, port, object)) = split_url(&url) else {
        return fail(format!("Bad URL: {}", url), false);
    };

    unsafe {
        let con = WinHttpConnect(session, w(&host).as_ptr(), port, 0);
        if con.is_null() {
            let e = GetLastError();
            return fail(win_err(e), retryable_net(e));
        }
        let _con_guard = InetGuard(con);

        let verb = w("POST");
        let obj = w(&object);
        let req = WinHttpOpenRequest(
            con,
            verb.as_ptr(),
            obj.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            if secure { FLAG_SECURE } else { 0 } as _,
        );
        if req.is_null() {
            let e = GetLastError();
            return fail(win_err(e), retryable_net(e));
        }
        cancel.add(req);
        let _req_guard = ReqGuard { h: req, c: cancel };

        let fail_net = |e: u32| -> Outcome {
            if cancel.is_set() {
                cancelled()
            } else {
                fail(win_err(e), retryable_net(e))
            }
        };

        let mut headers = format!("Content-Type: {}\r\n", ctype);
        let total_arg: u32 = if total > 0xFFFF_FFFF {
            // > 4 GB: WinHTTP's length is 32-bit, send our own header
            headers += &format!("Content-Length: {}\r\n", total);
            IGNORE_REQUEST_TOTAL_LENGTH
        } else {
            total as u32
        };
        let wh = w(&headers);
        if WinHttpSendRequest(req, wh.as_ptr(), u32::MAX, std::ptr::null(), 0, total_arg, 0) == 0 {
            return fail_net(GetLastError());
        }

        let mut send_err: u32 = 0;
        let mut write_all = |mut data: &[u8]| -> bool {
            while !data.is_empty() {
                if cancel.is_set() {
                    return false;
                }
                let mut written: u32 = 0;
                let len = data.len().min(u32::MAX as usize) as u32;
                if WinHttpWriteData(req, data.as_ptr() as *const c_void, len, &mut written) == 0 {
                    send_err = GetLastError();
                    return false;
                }
                data = &data[written as usize..];
            }
            true
        };

        let mut sent_ok = write_all(pre.as_bytes());
        if sent_ok {
            let mut buf = vec![0u8; CHUNK];
            let mut left = file_size;
            while left > 0 && sent_ok {
                let want = (left.min(CHUNK as u64)) as usize;
                let got = match file.read(&mut buf[..want]) {
                    Ok(n) => n,
                    Err(e) => {
                        return fail(format!("Read error: {}", win_err(e.raw_os_error().unwrap_or(0) as u32)), false)
                    }
                };
                if got == 0 {
                    return fail("File got shorter while uploading".into(), false);
                }
                sent_ok = write_all(&buf[..got]);
                if sent_ok {
                    progress.fetch_add(got as i64, Ordering::Relaxed);
                    *added += got as i64;
                    left -= got as u64;
                }
            }
        }
        if sent_ok {
            sent_ok = write_all(pad.as_bytes()) && write_all(post.as_bytes());
        }
        drop(write_all);

        if cancel.is_set() {
            return cancelled();
        }

        // Even if the send broke, the server may already have answered (e.g. 413 Too Large).
        if WinHttpReceiveResponse(req, std::ptr::null_mut()) == 0 {
            return fail_net(if sent_ok { GetLastError() } else { send_err });
        }

        let mut status: u32 = 0;
        let mut sz: u32 = 4;
        WinHttpQueryHeaders(
            req,
            QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut sz,
            std::ptr::null_mut(),
        );

        let mut body: Vec<u8> = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req, &mut avail) == 0 || avail == 0 {
                break;
            }
            let old = body.len();
            body.resize(old + avail as usize, 0);
            let mut rd: u32 = 0;
            if WinHttpReadData(req, body[old..].as_mut_ptr() as *mut c_void, avail, &mut rd) == 0 {
                body.truncate(old);
                break;
            }
            body.truncate(old + rd as usize);
            if rd == 0 || body.len() > (1 << 20) {
                break;
            }
        }

        let link = match site.reply {
            Reply::Text => first_line(&body),
            Reply::Json => match json::parse(&body) {
                Some(v) => {
                    let l = json::get_string(&v, site.json_path);
                    if !l.is_empty() && !site.json_prefix.is_empty() {
                        format!("{}{}", site.json_prefix, l)
                    } else {
                        l
                    }
                }
                None => String::new(),
            },
        };

        if (200..300).contains(&status) && link.starts_with("http") && sent_ok {
            return Outcome { link, ..Default::default() };
        }
        fail(
            format!("HTTP {} - {}", status, short_body(&body)),
            retryable_status(status) || (!sent_ok && retryable_net(send_err)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls() {
        assert_eq!(split_url("https://uguu.se/upload?output=text").unwrap(), (true, "uguu.se".into(), 443, "/upload?output=text".into()));
        assert_eq!(split_url("https://x0.at/").unwrap(), (true, "x0.at".into(), 443, "/".into()));
        assert_eq!(split_url("https://x0.at").unwrap().3, "/");
        assert_eq!(split_url("http://h:8080/a").unwrap(), (false, "h".into(), 8080, "/a".into()));
        assert!(split_url("ftp://x").is_none());
    }
    #[test]
    fn text_helpers() {
        assert_eq!(first_line(b"  \r\nhttps://a/b \r\nmore"), "https://a/b");
        assert_eq!(short_body(b"  a \n\n b  "), "a b");
        assert_eq!(short_body(b""), "empty reply");
        assert_eq!(url_encode("a b/\u{e9}.txt"), "a%20b%2F%C3%A9.txt");
        assert_eq!(header_safe("a\"b\r\nc"), "a%22bc");
        assert_eq!(make_pad().len(), 18);
    }
}

/// End-to-end tests against a local HTTP server (set MU_TEST_SERVER=http://127.0.0.1:8765).
#[cfg(test)]
mod net_tests {
    use super::*;
    use crate::sites::{Body, Reply, Site};
    use std::os::windows::ffi::OsStrExt;

    fn server() -> Option<String> {
        std::env::var("MU_TEST_SERVER").ok()
    }
    fn site(url: &'static str, key: &'static str, body: Body, reply: Reply, path: &'static str, prefix: &'static str) -> Site {
        Site { key, name: "t", tag: "", note: "", max_bytes: 0, known_dup: false, url, field: "files[]",
               extra: &[("reqtype", "fileupload")], body, reply, json_path: path, json_prefix: prefix }
    }
    fn make_file(name: &str, len: usize) -> (Vec<u16>, u64) {
        let p = std::env::temp_dir().join(name);
        let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        (p.as_os_str().encode_wide().collect(), len as u64)
    }
    fn leak(s: String) -> &'static str { Box::leak(s.into_boxed_str()) }

    #[test]
    fn uploads_against_local_server() {
        let Some(srv) = server() else { eprintln!("MU_TEST_SERVER not set, skipping"); return };
        let session = open_session();
        assert!(!session.is_null());
        let cancel = CancelToken::default();
        let progress = AtomicI64::new(0);
        let opt = Options { litter_time: "12h".into() };

        // 1) multipart + text reply, 3 MB (several 1 MB chunks), litterbox "time" field, unicode name with quote
        let (path, size) = make_file("big t\u{e9}st.bin", 3 * 1024 * 1024 + 123);
        let s1 = site(leak(format!("{srv}/text?x=1")), "litterbox", Body::Multipart, Reply::Text, "", "");
        let mut added = 0;
        let o = upload_once(session, &s1, &path, "big \"t\u{e9}st\".bin", size, &opt, "", &progress, &mut added, &cancel);
        assert!(o.link.starts_with("https://example.test/"), "got: {} / {}", o.link, o.error);
        assert_eq!(added as u64, size);
        println!("TEXT_LINK {}", o.link);

        // 2) multipart + JSON reply (files.0.url) with unique-copy padding
        let (path2, size2) = make_file("small.txt", 1000);
        let s2 = site(leak(format!("{srv}/json")), "pomf", Body::Multipart, Reply::Json, "files.0.url", "");
        let mut added = 0;
        let o2 = upload_once(session, &s2, &path2, "small.txt", size2, &opt, &make_pad(), &progress, &mut added, &cancel);
        assert!(o2.link.starts_with("https://example.test/"), "got: {} / {}", o2.link, o2.error);
        println!("JSON_LINK {}", o2.link);

        // 3) raw body (Filebin style) + JSON reply with prefix
        let s3 = site(leak(format!("{srv}/raw/")), "filebin", Body::RawFilebin, Reply::Json, "bin.id", "https://filebin.net/");
        let mut added = 0;
        let o3 = upload_once(session, &s3, &path2, "small file.txt", size2, &opt, "", &progress, &mut added, &cancel);
        assert!(o3.link.starts_with("https://filebin.net/mu"), "got: {} / {}", o3.link, o3.error);
        println!("RAW_LINK {}", o3.link);

        // 4) HTTP 503 is reported as retryable
        let s4 = site(leak(format!("{srv}/fail503")), "x", Body::Multipart, Reply::Text, "", "");
        let mut added = 0;
        let o4 = upload_once(session, &s4, &path2, "small.txt", size2, &opt, "", &progress, &mut added, &cancel);
        assert!(o4.link.is_empty() && o4.retryable && o4.error.starts_with("HTTP 503"), "got: {:?}", o4.error);
        println!("FAIL_ERR {}", o4.error);

        // 5) connection refused is a clean, retryable network error
        let s5 = site("http://127.0.0.1:1/", "x", Body::Multipart, Reply::Text, "", "");
        let mut added = 0;
        let o5 = upload_once(session, &s5, &path2, "small.txt", size2, &opt, "", &progress, &mut added, &cancel);
        assert!(o5.link.is_empty() && o5.retryable, "got: {:?}", o5.error);
        println!("REFUSED_ERR {}", o5.error);

        // 6) cancelled token -> no request
        cancel.cancel_all();
        let mut added = 0;
        let o6 = upload_once(session, &s1, &path2, "small.txt", size2, &opt, "", &progress, &mut added, &cancel);
        assert!(o6.cancelled);
        close_handle(session);
    }
}
