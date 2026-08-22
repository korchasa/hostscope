//! A JSON reader just large enough for the Docker API. Written here rather
//! than taken as a dependency: the binary has to be a static musl file with no
//! external parts (section 7), and the shapes it has to read are three.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn str_of(&self, key: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

pub fn parse(text: &str) -> Option<Json> {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let value = parse_value(bytes, &mut pos)?;
    Some(value)
}

fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && matches!(b[*p], b' ' | b'\t' | b'\n' | b'\r') {
        *p += 1;
    }
}

fn parse_value(b: &[u8], p: &mut usize) -> Option<Json> {
    skip_ws(b, p);
    match *b.get(*p)? {
        b'{' => parse_obj(b, p),
        b'[' => parse_arr(b, p),
        b'"' => parse_str(b, p).map(Json::Str),
        b't' => lit(b, p, "true", Json::Bool(true)),
        b'f' => lit(b, p, "false", Json::Bool(false)),
        b'n' => lit(b, p, "null", Json::Null),
        _ => parse_num(b, p),
    }
}

fn lit(b: &[u8], p: &mut usize, word: &str, value: Json) -> Option<Json> {
    if b[*p..].starts_with(word.as_bytes()) {
        *p += word.len();
        Some(value)
    } else {
        None
    }
}

fn parse_num(b: &[u8], p: &mut usize) -> Option<Json> {
    let start = *p;
    while *p < b.len() && matches!(b[*p], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
        *p += 1;
    }
    std::str::from_utf8(&b[start..*p])
        .ok()?
        .parse()
        .ok()
        .map(Json::Num)
}

fn parse_str(b: &[u8], p: &mut usize) -> Option<String> {
    if b.get(*p)? != &b'"' {
        return None;
    }
    *p += 1;
    let mut out = String::new();
    loop {
        let c = *b.get(*p)?;
        *p += 1;
        match c {
            b'"' => return Some(out),
            b'\\' => {
                let e = *b.get(*p)?;
                *p += 1;
                match e {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'u' => {
                        let hex = std::str::from_utf8(b.get(*p..*p + 4)?).ok()?;
                        *p += 4;
                        let unit = u16::from_str_radix(hex, 16).ok()?;
                        // A surrogate pair arrives as two escapes in a row.
                        if (0xD800..0xDC00).contains(&unit) && b.get(*p) == Some(&b'\\') {
                            let hex2 = std::str::from_utf8(b.get(*p + 2..*p + 6)?).ok()?;
                            if let Ok(low) = u16::from_str_radix(hex2, 16) {
                                *p += 6;
                                if let Some(c) =
                                    char::decode_utf16([unit, low]).next().and_then(|r| r.ok())
                                {
                                    out.push(c);
                                    continue;
                                }
                            }
                        }
                        out.push(char::from_u32(unit as u32).unwrap_or('\u{fffd}'));
                    }
                    other => out.push(other as char),
                }
            }
            _ => {
                // Collect the whole UTF-8 sequence untouched.
                let len = utf8_len(c);
                let end = *p - 1 + len;
                let slice = b.get(*p - 1..end)?;
                out.push_str(&String::from_utf8_lossy(slice));
                *p = end;
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn parse_arr(b: &[u8], p: &mut usize) -> Option<Json> {
    *p += 1;
    let mut items = Vec::new();
    loop {
        skip_ws(b, p);
        if b.get(*p)? == &b']' {
            *p += 1;
            return Some(Json::Arr(items));
        }
        items.push(parse_value(b, p)?);
        skip_ws(b, p);
        if b.get(*p)? == &b',' {
            *p += 1;
        }
    }
}

fn parse_obj(b: &[u8], p: &mut usize) -> Option<Json> {
    *p += 1;
    let mut items = Vec::new();
    loop {
        skip_ws(b, p);
        if b.get(*p)? == &b'}' {
            *p += 1;
            return Some(Json::Obj(items));
        }
        let key = parse_str(b, p)?;
        skip_ws(b, p);
        if b.get(*p)? != &b':' {
            return None;
        }
        *p += 1;
        let value = parse_value(b, p)?;
        items.push((key, value));
        skip_ws(b, p);
        if b.get(*p)? == &b',' {
            *p += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shape_the_docker_api_returns() {
        let text = r#"[{"Id":"abc","Names":["/hs-net"],"Image":"nginx:1","State":"running",
            "Status":"Up 3 days","Created":1723600000,"Labels":{"a":"b"},
            "Ports":[{"PrivatePort":80,"PublicPort":8080,"Type":"tcp"}]}]"#;
        let v = parse(text).unwrap();
        let first = &v.as_arr().unwrap()[0];
        assert_eq!(first.str_of("Id"), "abc");
        assert_eq!(
            first.get("Names").unwrap().as_arr().unwrap()[0].as_str(),
            Some("/hs-net")
        );
        assert_eq!(first.get("Created").unwrap().as_f64(), Some(1723600000.0));
    }

    #[test]
    fn keeps_non_ascii_intact_for_the_escaper_to_handle() {
        let v = parse(r#"{"n":"hs-имя"}"#).unwrap();
        assert_eq!(v.str_of("n"), "hs-\u{438}\u{43c}\u{44f}");
    }
}
