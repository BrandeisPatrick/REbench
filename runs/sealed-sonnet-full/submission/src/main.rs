use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
use std::rc::Rc;
use std::cmp::Ordering;
use std::io::{Read, Write};

mod fmt;
mod parse;
mod eval;
mod funcs;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Er { Div0, Value, Na, Num, Ref, Name, Null }
impl Er {
    pub fn s(self) -> &'static str {
        match self { Er::Div0 => "#DIV/0!", Er::Value => "#VALUE!", Er::Na => "#N/A", Er::Num => "#NUM!", Er::Ref => "#REF!", Er::Name => "#NAME?", Er::Null => "#NULL!" }
    }
}
#[derive(Clone, Debug)]
pub enum Value { Empty, EmptyF, Num(f64), Str(Rc<str>), Bool(bool), Err(Er) }

pub struct Sheet {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Value>,
    pub state: Vec<u8>, // 0 const, 1 formula pending, 2 in progress, 3 done
    pub fid: Vec<u32>,
}

pub fn col_to_idx(s: &[u8]) -> usize {
    let mut c = 0usize;
    for &b in s { c = c * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize; }
    c - 1
}
pub fn parse_a1(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_alphabetic() { i += 1; }
    if i == 0 || i == b.len() { return None; }
    let mut r = 0usize;
    for &d in &b[i..] { if !d.is_ascii_digit() { return None; } r = r * 10 + (d - b'0') as usize; }
    if r == 0 { return None; }
    Some((r - 1, col_to_idx(&b[..i])))
}

struct RawCell { r: usize, c: usize, v: Value, f: Option<String> }

fn read_zip(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    let mut v = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

fn attr_val(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key { return Some(a.unescape_value().ok()?.into_owned()); }
    }
    None
}

fn unesc(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    if !s.contains('&') { return s.into_owned(); }
    let mut o = String::with_capacity(s.len());
    let mut rest: &str = &s;
    while let Some(p) = rest.find('&') {
        o.push_str(&rest[..p]);
        rest = &rest[p..];
        if let Some(e) = rest.find(';') {
            let ent = &rest[1..e];
            match ent {
                "lt" => o.push('<'), "gt" => o.push('>'), "amp" => o.push('&'), "quot" => o.push('"'), "apos" => o.push('\''),
                _ => {
                    if let Some(h) = ent.strip_prefix("#x") { if let Some(c) = u32::from_str_radix(h, 16).ok().and_then(char::from_u32) { o.push(c); } }
                    else if let Some(d) = ent.strip_prefix('#') { if let Some(c) = d.parse::<u32>().ok().and_then(char::from_u32) { o.push(c); } }
                }
            }
            rest = &rest[e + 1..];
        } else { o.push('&'); rest = &rest[1..]; }
    }
    o.push_str(rest);
    o
}
fn find(h: &[u8], pat: &[u8], from: usize) -> Option<usize> {
    memchr_find(&h[from..], pat).map(|p| p + from)
}
fn memchr_find(h: &[u8], pat: &[u8]) -> Option<usize> {
    let first = pat[0];
    let mut i = 0;
    while i + pat.len() <= h.len() {
        match h[i..h.len() - pat.len() + 1].iter().position(|&b| b == first) {
            None => return None,
            Some(p) => { i += p; if &h[i..i + pat.len()] == pat { return Some(i); } i += 1; }
        }
    }
    None
}

fn parse_sheet(data: &[u8], sst: &[Rc<str>]) -> Vec<RawCell> {
    let mut out = Vec::new();
    let mut pos = match find(data, b"<sheetData", 0) { Some(p) => p, None => return out };
    loop {
        // find next "<c " or "<c>"
        let st = match find(data, b"<c ", pos) { Some(p) => p, None => break };
        let gt = match find(data, b">", st) { Some(p) => p, None => break };
        let tag = &data[st + 3..gt];
        let selfclose = tag.ends_with(b"/");
        pos = gt + 1;
        // attributes
        let mut r = 0; let mut c = 0; let mut ctype: &[u8] = b"n";
        {
            let mut k = 0;
            while k < tag.len() {
                while k < tag.len() && tag[k] == b' ' { k += 1; }
                let ns = k;
                while k < tag.len() && tag[k] != b'=' { k += 1; }
                if k >= tag.len() { break; }
                let name = &tag[ns..k];
                k += 1;
                let q = tag[k]; k += 1;
                let vs = k;
                while k < tag.len() && tag[k] != q { k += 1; }
                let val = &tag[vs..k];
                k += 1;
                if name == b"r" { if let Some((rr, cc)) = parse_a1(std::str::from_utf8(val).unwrap_or("")) { r = rr; c = cc; } }
                else if name == b"t" { ctype = val; }
            }
        }
        if selfclose { continue; }
        let end = match find(data, b"</c>", pos) { Some(p) => p, None => break };
        let body = &data[pos..end];
        pos = end + 4;
        let mut f: Option<String> = None;
        let mut vtxt: &[u8] = b"";
        let mut itxt: Vec<u8> = Vec::new();
        if let Some(fs) = find(body, b"<f", 0) {
            if let Some(g) = find(body, b">", fs) {
                if body[g - 1] != b'/' {
                    if let Some(fe) = find(body, b"</f>", g) { let t = unesc(&body[g + 1..fe]); if !t.is_empty() { f = Some(t); } }
                }
            }
        }
        if let Some(vs) = find(body, b"<v>", 0) { if let Some(ve) = find(body, b"</v>", vs) { vtxt = &body[vs + 3..ve]; } }
        if ctype == b"inlineStr" {
            let mut p = 0;
            while let Some(ts) = find(body, b"<t", p) {
                let g = match find(body, b">", ts) { Some(g) => g, None => break };
                if body[g - 1] == b'/' { p = g; continue; }
                let te = match find(body, b"</t>", g) { Some(t) => t, None => break };
                itxt.extend_from_slice(&body[g + 1..te]);
                p = te;
            }
        }
        let vs = std::str::from_utf8(vtxt).unwrap_or("").trim();
        let v = match ctype {
            b"s" => vs.parse::<usize>().ok().and_then(|i| sst.get(i)).map(|s| Value::Str(s.clone())).unwrap_or(Value::Empty),
            b"inlineStr" => { let t = unesc(&itxt); if t.is_empty() { Value::Empty } else { Value::Str(Rc::from(t.as_str())) } }
            b"str" => if vs.is_empty() { Value::Empty } else { Value::Str(Rc::from(unesc(vtxt).as_str())) },
            b"b" => Value::Bool(vs == "1"),
            b"e" => Value::Err(match vs { "#DIV/0!" => Er::Div0, "#N/A" => Er::Na, "#NUM!" => Er::Num, "#REF!" => Er::Ref, "#NAME?" => Er::Name, "#NULL!" => Er::Null, _ => Er::Value }),
            _ => if vs.is_empty() { Value::Empty } else { vs.parse::<f64>().map(Value::Num).unwrap_or(Value::Empty) },
        };
        out.push(RawCell { r, c, v, f });
    }
    out
}

fn parse_sst(data: &[u8]) -> Vec<Rc<str>> {
    use quick_xml::events::Event;
    let mut rd = quick_xml::Reader::from_reader(data);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_t = false;
    loop {
        match rd.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => { match e.name().as_ref() { b"si" => cur.clear(), b"t" => in_t = true, _ => {} } }
            Ok(Event::Text(t)) => { if in_t { if let Ok(s) = t.unescape() { cur.push_str(&s); } } }
            Ok(Event::End(e)) => { match e.name().as_ref() { b"t" => in_t = false, b"si" => out.push(Rc::from(cur.as_str())), _ => {} } }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn run(inp: String, outp: String) {
    let t0 = std::time::Instant::now(); let dbg = std::env::var("CALC_T").is_ok();
    let f = std::fs::File::open(&inp).expect("open");
    let mut z = zip::ZipArchive::new(f).expect("zip");
    let wb = read_zip(&mut z, "xl/workbook.xml").unwrap_or_default();
    let rels = read_zip(&mut z, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let sst = read_zip(&mut z, "xl/sharedStrings.xml").map(|d| parse_sst(&d)).unwrap_or_default();
    // sheets
    let mut sheets_meta: Vec<(String, String)> = Vec::new(); // name, rid
    {
        use quick_xml::events::Event;
        let mut rd = quick_xml::Reader::from_reader(&wb[..]);
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"sheet" {
                        let name = attr_val(&e, b"name").unwrap_or_default();
                        let mut rid = String::new();
                        for a in e.attributes().flatten() {
                            if a.key.as_ref().ends_with(b":id") { rid = a.unescape_value().unwrap().into_owned(); }
                        }
                        sheets_meta.push((name, rid));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut relmap: Vec<(String, String)> = Vec::new();
    {
        use quick_xml::events::Event;
        let mut rd = quick_xml::Reader::from_reader(&rels[..]);
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"Relationship" {
                        relmap.push((attr_val(&e, b"Id").unwrap_or_default(), attr_val(&e, b"Target").unwrap_or_default()));
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut raws: Vec<Vec<RawCell>> = Vec::new();
    for (_, rid) in &sheets_meta {
        let t = relmap.iter().find(|(i, _)| i == rid).map(|x| x.1.clone()).unwrap_or_default();
        let path = if let Some(s) = t.strip_prefix('/') { s.to_string() } else { format!("xl/{}", t) };
        let d = read_zip(&mut z, &path).unwrap_or_default();
        raws.push(parse_sheet(&d, &sst));
    }
    if dbg { eprintln!("parsed xml {:?}", t0.elapsed()); }
    let names: Vec<String> = sheets_meta.iter().map(|x| x.0.clone()).collect();
    let mut sheets: Vec<Sheet> = Vec::new();
    for (i, rc) in raws.iter().enumerate() {
        let mut mr = 0; let mut mc = 0;
        for c in rc { if c.r + 1 > mr { mr = c.r + 1; } if c.c + 1 > mc { mc = c.c + 1; } }
        sheets.push(Sheet { name: names[i].clone(), rows: mr, cols: mc, cells: vec![Value::Empty; mr * mc], state: vec![0; mr * mc], fid: vec![0; mr * mc] });
    }
    let mut formulas: Vec<eval::Formula> = Vec::new();
    let mut fsrc: Vec<(usize, usize, usize, String)> = Vec::new();
    for (i, rc) in raws.into_iter().enumerate() {
        for c in rc {
            let idx = c.r * sheets[i].cols + c.c;
            if let Some(f) = c.f {
                fsrc.push((i, c.r, c.c, f));
                sheets[i].state[idx] = 1;
                sheets[i].fid[idx] = fsrc.len() as u32;
            } else {
                sheets[i].cells[idx] = c.v;
            }
        }
    }
    {
        struct SB(Vec<parse::Node>);
        unsafe impl Send for SB {}
        let nthreads = if fsrc.len() > 4000 { 4 } else { 1 };
        let chunk = (fsrc.len() + nthreads - 1) / nthreads.max(1);
        let mut parts: Vec<SB> = Vec::new();
        if nthreads == 1 {
            parts.push(SB(fsrc.iter().map(|(_, _, _, f)| parse::parse_formula(f, &names)).collect()));
        } else {
            std::thread::scope(|sc| {
                let hs: Vec<_> = fsrc.chunks(chunk).map(|ch| { let names = &names; sc.spawn(move || SB(ch.iter().map(|(_, _, _, f)| parse::parse_formula(f, names)).collect())) }).collect();
                for h in hs { parts.push(h.join().unwrap()); }
            });
        }
        let mut it = fsrc.iter();
        for p in parts { for ast in p.0 { let (s, r, c, _) = it.next().unwrap(); formulas.push(eval::Formula { sheet: *s, row: *r, col: *c, ast }); } }
    }
    if dbg { eprintln!("parsed formulas {:?}", t0.elapsed()); }
    let eng = eval::Engine::new(sheets, formulas);
    let mut out = String::with_capacity(fsrc.len() * 40);
    out.push('{');
    for (si, name) in names.iter().enumerate() {
        if si > 0 { out.push(','); }
        out.push('"'); out.push_str(name); out.push_str("\":{");
        let mut first = true;
        for (fi, (s, r, c, _)) in fsrc.iter().enumerate() {
            if *s != si { continue; }
            let v = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| eng.formula_value(fi))).unwrap_or(Value::Err(Er::Value));
            if !first { out.push(','); }
            first = false;
            out.push('"');
            let mut cn = Vec::new(); let mut cc = *c + 1;
            while cc > 0 { cn.push(b'A' + ((cc - 1) % 26) as u8); cc = (cc - 1) / 26; }
            cn.reverse();
            out.push_str(std::str::from_utf8(&cn).unwrap());
            out.push_str(&(r + 1).to_string());
            out.push_str("\":");
            match v {
                Value::Num(n) if !n.is_finite() => out.push_str("{\"t\":\"e\",\"v\":\"#NUM!\"}"),
                Value::Num(n) => { let n = if n == 0.0 { 0.0 } else { n }; out.push_str(&format!("{{\"t\":\"n\",\"v\":{}}}", n)); }
                Value::Bool(b) => out.push_str(if b { "{\"t\":\"b\",\"v\":true}" } else { "{\"t\":\"b\",\"v\":false}" }),
                Value::Err(e) => { out.push_str("{\"t\":\"e\",\"v\":\""); out.push_str(e.s()); out.push_str("\"}"); }
                Value::Str(s) => {
                    out.push_str("{\"t\":\"s\",\"v\":\"");
                    for ch in s.chars() {
                        match ch { '"' => out.push_str("\\\""), '\\' => out.push_str("\\\\"), '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"),
                            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) }
                    }
                    out.push_str("\"}");
                }
                Value::Empty | Value::EmptyF => out.push_str("{\"t\":\"n\",\"v\":0}"),
            }
        }
        out.push('}');
    }
    out.push('}');
    if dbg { eprintln!("evaluated+json {:?}", t0.elapsed()); }
    let mut f = std::fs::File::create(&outp).expect("create");
    f.write_all(out.as_bytes()).unwrap();
    drop(f);
    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut inp = String::new(); let mut outp = String::new();
    let mut i = 2;
    while i + 1 < args.len() {
        match args[i].as_str() { "--in" => inp = args[i + 1].clone(), "--out" => outp = args[i + 1].clone(), _ => {} }
        i += 2;
    }
    std::panic::set_hook(Box::new(|_| {}));
    let mut sizes = vec![3usize << 30, 1 << 30, 256 << 20, 64 << 20];
    while let Some(sz) = sizes.first().copied() {
        sizes.remove(0);
        let (a, b) = (inp.clone(), outp.clone());
        match std::thread::Builder::new().stack_size(sz).spawn(move || run(a, b)) {
            Ok(h) => { let _ = h.join(); break; }
            Err(_) => continue,
        }
    }
}

pub fn cmp_ord(a: f64, b: f64) -> Ordering {
    if approx_eq(a, b) { Ordering::Equal } else if a < b { Ordering::Less } else { Ordering::Greater }
}
pub fn approx_eq(a: f64, b: f64) -> bool {
    if a == b { return true; }
    let x = a - b;
    x.abs() < a.abs() * (1.0 / 281474976710656.0)
}
