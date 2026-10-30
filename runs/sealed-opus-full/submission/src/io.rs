use std::collections::HashMap;
use std::io::Read;

fn u16le(b: &[u8], i: usize) -> usize { (b[i] as usize) | ((b[i + 1] as usize) << 8) }
fn u32le(b: &[u8], i: usize) -> usize { u16le(b, i) | (u16le(b, i + 2) << 16) }

pub fn read_zip(data: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut out = HashMap::new();
    // find EOCD
    let mut e = data.len().saturating_sub(22);
    loop {
        if u32le(data, e) == 0x06054b50 { break; }
        if e == 0 { return out; }
        e -= 1;
    }
    let n = u16le(data, e + 10);
    let mut p = u32le(data, e + 16);
    for _ in 0..n {
        if u32le(data, p) != 0x02014b50 { break; }
        let method = u16le(data, p + 10);
        let csize = u32le(data, p + 20);
        let usize_ = u32le(data, p + 24);
        let fnl = u16le(data, p + 28);
        let exl = u16le(data, p + 30);
        let cml = u16le(data, p + 32);
        let lho = u32le(data, p + 42);
        let name = String::from_utf8_lossy(&data[p + 46..p + 46 + fnl]).to_string();
        p += 46 + fnl + exl + cml;
        let lfnl = u16le(data, lho + 26);
        let lexl = u16le(data, lho + 28);
        let start = lho + 30 + lfnl + lexl;
        let comp = &data[start..start + csize];
        let content = if method == 0 { comp.to_vec() } else {
            let mut d = flate2::read::DeflateDecoder::new(comp);
            let mut v = Vec::with_capacity(usize_);
            d.read_to_end(&mut v).unwrap();
            v
        };
        out.insert(name, content);
    }
    out
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') { return s.to_string(); }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(j) = rest.find(';') {
            let ent = &rest[1..j];
            match ent {
                "lt" => o.push('<'), "gt" => o.push('>'), "amp" => o.push('&'),
                "quot" => o.push('"'), "apos" => o.push('\''),
                _ => {
                    if let Some(h) = ent.strip_prefix("#x") {
                        if let Some(c) = u32::from_str_radix(h, 16).ok().and_then(char::from_u32) { o.push(c); }
                    } else if let Some(d) = ent.strip_prefix('#') {
                        if let Some(c) = d.parse::<u32>().ok().and_then(char::from_u32) { o.push(c); }
                    } else { o.push_str(&rest[..=j]); }
                }
            }
            rest = &rest[j + 1..];
        } else { o.push_str(rest); rest = ""; }
    }
    o.push_str(rest);
    o
}

// get attribute value from a tag string
pub fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut start = 0;
    while let Some(i) = tag[start..].find(name) {
        let i = start + i;
        let before_ok = i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' || bytes[i - 1] == b'\n');
        let after = i + name.len();
        if before_ok && after + 1 < bytes.len() && bytes[after] == b'=' && (bytes[after + 1] == b'"' || bytes[after + 1] == b'\'') {
            let q = bytes[after + 1] as char;
            let vs = after + 2;
            let ve = tag[vs..].find(q).map(|x| x + vs)?;
            return Some(&tag[vs..ve]);
        }
        start = i + name.len();
    }
    None
}

// extract text of all <t> elements inside s
pub fn collect_t(s: &str) -> String {
    let mut o = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<t") {
        let after = &rest[i + 2..];
        let c = after.as_bytes().first().copied().unwrap_or(b' ');
        if c != b'>' && c != b' ' && c != b'/' { rest = after; continue; }
        let gt = after.find('>').unwrap();
        if gt > 0 && after.as_bytes()[gt - 1] == b'/' { rest = &after[gt + 1..]; continue; }
        let body = &after[gt + 1..];
        let end = body.find("</t>").unwrap_or(body.len());
        o.push_str(&unescape(&body[..end]));
        rest = &body[end..];
    }
    o
}

pub fn shared_strings(x: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = x;
    while let Some(i) = rest.find("<si") {
        let after = &rest[i + 3..];
        let end = after.find("</si>").unwrap_or(after.len());
        let body = &after[..end];
        // skip rPh phonetic
        v.push(collect_t(body));
        rest = &after[end..];
    }
    v
}

pub fn col_to_num(s: &[u8]) -> (u32, usize) {
    let mut c = 0u32;
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_alphabetic() {
        c = c * 26 + (s[i].to_ascii_uppercase() - b'A' + 1) as u32;
        i += 1;
    }
    (c, i)
}

pub fn parse_a1(s: &str) -> (u32, u32) {
    let b = s.as_bytes();
    let (c, i) = col_to_num(b);
    let r: u32 = s[i..].parse().unwrap_or(1);
    (r - 1, c - 1)
}

pub fn col_name(mut c: u32) -> String {
    c += 1;
    let mut v = Vec::new();
    while c > 0 { let m = (c - 1) % 26; v.push(b'A' + m as u8); c = (c - 1) / 26; }
    v.reverse();
    String::from_utf8(v).unwrap()
}

pub enum RawCell { Num(f64), Str(String), Bool(bool), Err(String), Formula(String) }

pub fn parse_sheet(x: &str, ss: &[String], cb: &mut dyn FnMut(u32, u32, RawCell)) {
    let mut rest = match x.find("<sheetData") { Some(i) => &x[i..], None => return };
    while let Some(i) = rest.find("<c") {
        let after = &rest[i + 2..];
        let c0 = after.as_bytes()[0];
        if c0 != b' ' && c0 != b'>' && c0 != b'/' { rest = after; continue; }
        let gt = after.find('>').unwrap();
        let tag = &after[..gt];
        let r = attr(tag, "r").unwrap_or("A1");
        let (row, col) = parse_a1(r);
        if tag.ends_with('/') { rest = &after[gt + 1..]; continue; }
        let body_all = &after[gt + 1..];
        let end = body_all.find("</c>").unwrap_or(body_all.len());
        let body = &body_all[..end];
        rest = &body_all[end..];
        let t = attr(tag, "t").unwrap_or("n");
        // formula?
        if let Some(fi) = body.find("<f") {
            let fa = &body[fi + 2..];
            let fgt = fa.find('>').unwrap();
            if !fa[..fgt].ends_with('/') {
                let fb = &fa[fgt + 1..];
                let fe = fb.find("</f>").unwrap_or(fb.len());
                cb(row, col, RawCell::Formula(unescape(&fb[..fe])));
                continue;
            }
        }
        let v = || -> Option<String> {
            let vi = body.find("<v")?;
            let va = &body[vi..];
            let g = va.find('>')?;
            if va[..g].ends_with('/') { return None; }
            let vb = &va[g + 1..];
            let ve = vb.find("</v>")?;
            Some(unescape(&vb[..ve]))
        };
        match t {
            "inlineStr" => { let t = collect_t(body); if !t.is_empty() { cb(row, col, RawCell::Str(t)); } }
            "s" => { if let Some(s) = v() { let i: usize = s.trim().parse().unwrap_or(0); cb(row, col, RawCell::Str(ss.get(i).cloned().unwrap_or_default())); } }
            "str" => { if let Some(s) = v() { cb(row, col, RawCell::Str(s)); } }
            "b" => { if let Some(s) = v() { cb(row, col, RawCell::Bool(s.trim() == "1" || s.trim() == "true")); } }
            "e" => { if let Some(s) = v() { cb(row, col, RawCell::Err(s)); } }
            _ => { if let Some(s) = v() { if let Ok(f) = s.trim().parse::<f64>() { cb(row, col, RawCell::Num(f)); } } }
        }
    }
}

pub fn push_col(o: &mut String, c: u32) {
    let mut buf = [0u8; 4]; let mut n = 0; let mut c = c + 1;
    while c > 0 { let m = (c - 1) % 26; buf[n] = b'A' + m as u8; n += 1; c = (c - 1) / 26; }
    for i in (0..n).rev() { o.push(buf[i] as char); }
}
