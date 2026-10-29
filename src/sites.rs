//! Host table. Add new hosts here.

pub const MB: u64 = 1024 * 1024;
pub const GB: u64 = 1024 * MB;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Multipart,
    RawFilebin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Text,
    Json,
}

pub struct Site {
    pub key: &'static str,
    pub name: &'static str,
    pub tag: &'static str,
    pub note: &'static str,
    /// 0 = no limit
    pub max_bytes: u64,
    /// host hands back the same link for identical bytes
    pub known_dup: bool,
    pub url: &'static str,
    /// multipart field name for the file
    pub field: &'static str,
    /// extra multipart fields
    pub extra: &'static [(&'static str, &'static str)],
    pub body: Body,
    pub reply: Reply,
    /// where the link lives in a JSON reply
    pub json_path: &'static str,
    /// prepended to the JSON value (Filebin returns only the bin id)
    pub json_prefix: &'static str,
}

pub static SITES: &[Site] = &[
    Site { key: "tempsh", name: "temp.sh", tag: "4 GB \u{00B7} 3 days", note: "Files expire after 3 days.",
        max_bytes: 4 * GB, known_dup: false, url: "https://temp.sh/upload", field: "file", extra: &[],
        body: Body::Multipart, reply: Reply::Text, json_path: "", json_prefix: "" },
    Site { key: "gofile", name: "Gofile", tag: "No limit \u{00B7} long-term", note: "Links are Gofile download pages.",
        max_bytes: 0, known_dup: false, url: "https://upload.gofile.io/uploadfile", field: "file", extra: &[],
        body: Body::Multipart, reply: Reply::Json, json_path: "data.downloadPage", json_prefix: "" },
    Site { key: "catbox", name: "Catbox", tag: "200 MB \u{00B7} permanent", note: "Catbox files are permanent.",
        max_bytes: 200 * MB, known_dup: true, url: "https://catbox.moe/user/api.php", field: "fileToUpload",
        extra: &[("reqtype", "fileupload")], body: Body::Multipart, reply: Reply::Text, json_path: "", json_prefix: "" },
    // the "time" field is added from the options
    Site { key: "litterbox", name: "Litterbox", tag: "1 GB \u{00B7} 1h to 72h", note: "Files expire after the time you picked.",
        max_bytes: GB, known_dup: true, url: "https://litterbox.catbox.moe/resources/internals/api.php", field: "fileToUpload",
        extra: &[("reqtype", "fileupload")], body: Body::Multipart, reply: Reply::Text, json_path: "", json_prefix: "" },
    Site { key: "uguu", name: "Uguu", tag: "128 MB \u{00B7} a few hours", note: "Uguu files are short-lived.",
        max_bytes: 128 * MB, known_dup: false, url: "https://uguu.se/upload?output=text", field: "files[]", extra: &[],
        body: Body::Multipart, reply: Reply::Text, json_path: "", json_prefix: "" },
    Site { key: "tmpfiles", name: "tmpfiles.org", tag: "100 MB \u{00B7} 48 hours", note: "Files expire after 48 hours.",
        max_bytes: 100 * MB, known_dup: false, url: "https://tmpfiles.org/api/v1/upload", field: "file",
        extra: &[("expire", "172800")], body: Body::Multipart, reply: Reply::Json, json_path: "data.url", json_prefix: "" },
    Site { key: "x0", name: "x0.at", tag: "1 GB \u{00B7} 3 to 100 days", note: "Kept 3 to 100 days (smaller files longer).",
        max_bytes: GB, known_dup: false, url: "https://x0.at/", field: "file", extra: &[],
        body: Body::Multipart, reply: Reply::Text, json_path: "", json_prefix: "" },
    Site { key: "tmpfilelink", name: "tmpfile.link", tag: "100 MB \u{00B7} 7 days", note: "Files expire after 7 days.",
        max_bytes: 100 * MB, known_dup: false, url: "https://tmpfile.link/api/upload", field: "file", extra: &[],
        body: Body::Multipart, reply: Reply::Json, json_path: "downloadLinkEncoded", json_prefix: "" },
    Site { key: "pomf", name: "pomf.lain.la", tag: "1 GB \u{00B7} permanent", note: "Pomf files are kept permanently.",
        max_bytes: GB, known_dup: true, url: "https://pomf.lain.la/upload.php", field: "files[]", extra: &[],
        body: Body::Multipart, reply: Reply::Json, json_path: "files.0.url", json_prefix: "" },
    Site { key: "quax", name: "qu.ax", tag: "256 MB \u{00B7} media / archives", note: "qu.ax accepts images, video, archives, pdf, txt.",
        max_bytes: 256 * MB, known_dup: true, url: "https://qu.ax/upload", field: "files[]", extra: &[],
        body: Body::Multipart, reply: Reply::Json, json_path: "files.0.url", json_prefix: "" },
    Site { key: "filebin", name: "Filebin", tag: "No limit \u{00B7} about 1 week", note: "Files expire after about 1 week.",
        max_bytes: 0, known_dup: false, url: "https://filebin.net/", field: "", extra: &[],
        body: Body::RawFilebin, reply: Reply::Json, json_path: "bin.id", json_prefix: "https://filebin.net/" },
];

pub fn site_index(key: &str) -> Option<usize> {
    SITES.iter().position(|s| s.key == key)
}

impl Site {
    /// Upload endpoint. The `local-test` feature (never used for release builds)
    /// points every host at a local test server so the whole app can be tested
    /// end-to-end without uploading anything to the internet.
    #[cfg(not(feature = "local-test"))]
    pub fn endpoint(&self) -> std::borrow::Cow<'static, str> {
        self.url.into()
    }
    #[cfg(feature = "local-test")]
    pub fn endpoint(&self) -> std::borrow::Cow<'static, str> {
        let base = "http://127.0.0.1:8765";
        match (self.body, self.reply) {
            (Body::RawFilebin, _) => format!("{base}/raw/"),
            (_, Reply::Json) => format!("{base}/json/{}", self.key),
            _ => format!("{base}/text/{}", self.key),
        }
        .into()
    }
}
