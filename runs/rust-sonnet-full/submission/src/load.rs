use crate::eval::{V, Er};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;
use std::rc::Rc;

pub struct SheetData {
    pub name: String,
    pub cells: Vec<(u32, u32, Cell)>,
}
pub enum Cell {
    Val(V),
    Formula(String),
}
pub struct Workbook {
    pub sheets: Vec<SheetData>,
}

fn read_zip(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

pub fn parse_ref(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut c: u32 = 0;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        c = c * 26 + (b[i].to_ascii_uppercase() - b'A' + 1) as u32;
        i += 1;
    }
    if i == 0 || i == b.len() { return None; }
    let mut r: u32 = 0;
    while i < b.len() && b[i].is_ascii_digit() { r = r * 10 + (b[i] - b'0') as u32; i += 1; }
    if i != b.len() || r == 0 { return None; }
    Some((r - 1, c - 1))
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key {
            return Some(a.unescape_value().ok()?.into_owned());
        }
    }
    None
}

fn parse_shared(data: &[u8]) -> Vec<String> {
    let mut rd = Reader::from_reader(data);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    let mut cur = String::new();
    let mut in_t = false;
    let mut in_si = false;
    loop {
        match rd.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"si" => { in_si = true; cur.clear(); }
                b"t" => in_t = true,
                _ => {}
            },
            Ok(Event::Text(t)) => { if in_t && in_si { cur.push_str(&t.unescape().unwrap_or_default()); } }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"si" => { in_si = false; out.push(std::mem::take(&mut cur)); }
                b"t" => in_t = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn unesc(s: &str) -> String {
    if !s.contains('&') { return s.to_string(); }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        if let Some(e) = rest.find(';') {
            let ent = &rest[1..e];
            match ent {
                "amp" => out.push('&'), "lt" => out.push('<'), "gt" => out.push('>'), "quot" => out.push('"'), "apos" => out.push('\''),
                _ => {
                    if let Some(h) = ent.strip_prefix("#x") { if let Some(c) = u32::from_str_radix(h, 16).ok().and_then(char::from_u32) { out.push(c); } }
                    else if let Some(d) = ent.strip_prefix('#') { if let Some(c) = d.parse::<u32>().ok().and_then(char::from_u32) { out.push(c); } }
                    else { out.push_str(&rest[..=e]); }
                }
            }
            rest = &rest[e + 1..];
        } else { out.push_str(rest); rest = ""; }
    }
    out.push_str(rest);
    out
}

fn find(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    memchr::memmem::find(&h[from..], pat).map(|p| p + from)
}

fn get_attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!(" {}=\"", key);
    let p = tag.find(&pat)? + pat.len();
    let e = tag[p..].find('"')? + p;
    Some(&tag[p..e])
}

fn parse_sheet(data: &[u8], shared: &[String]) -> Vec<(u32, u32, Cell)> {
    let mut out = Vec::new();
    let txt = match std::str::from_utf8(data) { Ok(t) => t, Err(_) => return out };
    let mut pos = match find(data, 0, b"<sheetData") { Some(p) => p, None => return out };
    loop {
        let st = match find(data, pos, b"<c ") { Some(p) => p, None => break };
        let tag_end = match find(data, st, b">") { Some(p) => p, None => break };
        let tag = &txt[st..tag_end];
        let selfclose = tag.ends_with('/');
        let r = get_attr(tag, "r").and_then(parse_ref);
        let ty = get_attr(tag, "t").unwrap_or("");
        if selfclose { pos = tag_end + 1; continue; }
        let end = match find(data, tag_end, b"</c>") { Some(p) => p, None => break };
        let body = &txt[tag_end + 1..end];
        pos = end + 4;
        let (r, c) = match r { Some(x) => x, None => continue };
        // formula
        if let Some(fp) = body.find("<f") {
            let after = &body[fp + 2..];
            let nb = after.as_bytes().first().copied().unwrap_or(b'>');
            if nb == b'>' || nb == b' ' {
                let gt = after.find('>').unwrap_or(0);
                if !after[..gt].ends_with('/') {
                    let content = &after[gt + 1..];
                    let fe = content.find("</f>").unwrap_or(content.len());
                    out.push((r, c, Cell::Formula(unesc(&content[..fe]))));
                    continue;
                }
            }
        }
        let vtxt = || -> &str {
            if let Some(vp) = body.find("<v>") { let rest = &body[vp + 3..]; let ve = rest.find("</v>").unwrap_or(rest.len()); &rest[..ve] } else { "" }
        };
        let v = match ty {
            "s" => { let i: usize = vtxt().trim().parse().unwrap_or(0); Some(V::S(Rc::from(shared.get(i).map(|s| s.as_str()).unwrap_or("")))) }
            "inlineStr" => {
                let mut t = String::new();
                let mut rest = body;
                while let Some(tp) = rest.find("<t") {
                    let after = &rest[tp + 2..];
                    let nb = after.as_bytes().first().copied().unwrap_or(0);
                    if nb != b'>' && nb != b' ' && nb != b'/' { rest = after; continue; }
                    let gt = match after.find('>') { Some(g) => g, None => break };
                    if after[..gt].ends_with('/') { rest = &after[gt..]; continue; }
                    let content = &after[gt + 1..];
                    let te = content.find("</t>").unwrap_or(content.len());
                    t.push_str(&unesc(&content[..te]));
                    rest = &content[te..];
                }
                Some(V::S(Rc::from(t.as_str())))
            }
            "str" => Some(V::S(Rc::from(unesc(vtxt()).as_str()))),
            "b" => Some(V::B(vtxt().trim() == "1")),
            "e" => Some(V::E(Er::from_str(vtxt().trim()))),
            _ => { let t = vtxt().trim(); if t.is_empty() { None } else { t.parse::<f64>().ok().map(V::N) } }
        };
        if let Some(v) = v { if let V::S(t) = &v { if t.is_empty() { continue; } } out.push((r, c, Cell::Val(v))); }
    }
    out
}

pub fn load(path: &str) -> Result<Workbook, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let wbx = read_zip(&mut z, "xl/workbook.xml").ok_or("no workbook")?;
    let rels = read_zip(&mut z, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let shared = read_zip(&mut z, "xl/sharedStrings.xml").map(|d| parse_shared(&d)).unwrap_or_default();
    // rels
    let mut relmap: Vec<(String, String)> = Vec::new();
    {
        let mut rd = Reader::from_reader(&rels[..]);
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"Relationship" {
                        relmap.push((attr(&e, b"Id").unwrap_or_default(), attr(&e, b"Target").unwrap_or_default()));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut names: Vec<(String, String)> = Vec::new();
    {
        let mut rd = Reader::from_reader(&wbx[..]);
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"sheet" {
                        names.push((attr(&e, b"name").unwrap_or_default(), attr(&e, b"r:id").unwrap_or_default()));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut sheets = Vec::new();
    for (i, (name, rid)) in names.iter().enumerate() {
        let target = relmap.iter().find(|(id, _)| id == rid).map(|(_, t)| t.clone())
            .unwrap_or(format!("worksheets/sheet{}.xml", i + 1));
        let p = if let Some(t) = target.strip_prefix('/') { t.to_string() } else { format!("xl/{}", target) };
        let data = read_zip(&mut z, &p).unwrap_or_default();
        sheets.push(SheetData { name: name.clone(), cells: parse_sheet(&data, &shared) });
    }
    Ok(Workbook { sheets })
}
