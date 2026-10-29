//! Tiny JSON reader for host replies (no dependencies).

#[allow(dead_code)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn lit(&mut self, w: &[u8]) -> bool {
        if self.s[self.i..].starts_with(w) {
            self.i += w.len();
            true
        } else {
            false
        }
    }
    fn hex4(&mut self) -> Option<u32> {
        let h = self.s.get(self.i..self.i + 4)?;
        let v = u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?;
        self.i += 4;
        Some(v)
    }
    fn string(&mut self) -> Option<String> {
        // at opening quote
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                let save = self.i;
                                self.i += 2;
                                match self.hex4() {
                                    Some(lo) if (0xDC00..0xE000).contains(&lo) => {
                                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    }
                                    _ => self.i = save,
                                }
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut b = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                        }
                        _ => return None,
                    }
                }
                _ => out.push(c),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }
    fn value(&mut self) -> Option<Value> {
        self.ws();
        self.depth += 1;
        if self.depth > 200 {
            return None;
        }
        let v = match self.peek()? {
            b'{' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek()? == b'}' {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.peek()? != b'"' {
                            return None;
                        }
                        let k = self.string()?;
                        self.ws();
                        if self.peek()? != b':' {
                            return None;
                        }
                        self.i += 1;
                        let v = self.value()?;
                        items.push((k, v));
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b'}' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Value::Obj(items)
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek()? == b']' {
                    self.i += 1;
                } else {
                    loop {
                        items.push(self.value()?);
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b']' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Value::Arr(items)
            }
            b'"' => Value::Str(self.string()?),
            b't' if self.lit(b"true") => Value::Bool(true),
            b'f' if self.lit(b"false") => Value::Bool(false),
            b'n' if self.lit(b"null") => Value::Null,
            _ => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[st..self.i]).ok()?;
                Value::Num(t.parse().ok()?)
            }
        };
        self.depth -= 1;
        Some(v)
    }
}

pub fn parse(s: &[u8]) -> Option<Value> {
    let mut p = P { s, i: 0, depth: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != s.len() {
        return None;
    }
    Some(v)
}

/// Follows a dotted path like "files.0.url". Numbers index arrays.
pub fn get_string(root: &Value, path: &str) -> String {
    let mut cur = root;
    for part in path.split('.') {
        cur = match cur {
            Value::Arr(items) => match part.parse::<usize>() {
                Ok(i) if i < items.len() => &items[i],
                _ => return String::new(),
            },
            Value::Obj(items) => match items.iter().find(|(k, _)| k == part) {
                Some((_, v)) => v,
                None => return String::new(),
            },
            _ => return String::new(),
        };
    }
    match cur {
        Value::Str(s) => s.clone(),
        Value::Num(n) => format!("{:.0}", n),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths() {
        let v = parse(br#"{"success":true,"files":[{"url":"https:\/\/a.b\/c.png","n":5}],"data":{"url":"x\u00e9"}}"#).unwrap();
        assert_eq!(get_string(&v, "files.0.url"), "https://a.b/c.png");
        assert_eq!(get_string(&v, "files.0.n"), "5");
        assert_eq!(get_string(&v, "data.url"), "x\u{e9}");
        assert_eq!(get_string(&v, "files.1.url"), "");
        assert_eq!(get_string(&v, "nope"), "");
        assert!(parse(b"{bad").is_none());
    }
}
