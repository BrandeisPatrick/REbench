use crate::*;
use quick_xml::events::Event;
use quick_xml::Reader;

fn read_zip_file(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = z.by_name(name).ok()?;
    let mut v = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

fn parse_cell_ref(s: &[u8]) -> (usize, usize) {
    let mut c = 0usize;
    let mut r = 0usize;
    for &b in s {
        if b.is_ascii_alphabetic() {
            c = c * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize;
        } else if b.is_ascii_digit() {
            r = r * 10 + (b - b'0') as usize;
        }
    }
    (r.saturating_sub(1), c.saturating_sub(1))
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        if let Some(e) = rest.find(';') {
            let ent = &rest[1..e];
            match ent {
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "amp" => out.push('&'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ => {
                    if let Some(h) = ent.strip_prefix("#x") {
                        if let Ok(n) = u32::from_str_radix(h, 16) {
                            if let Some(ch) = char::from_u32(n) {
                                out.push(ch);
                            }
                        }
                    } else if let Some(d) = ent.strip_prefix('#') {
                        if let Ok(n) = d.parse::<u32>() {
                            if let Some(ch) = char::from_u32(n) {
                                out.push(ch);
                            }
                        }
                    } else {
                        out.push_str(&rest[..e + 1]);
                    }
                }
            }
            rest = &rest[e + 1..];
        } else {
            out.push_str(rest);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

struct RawCell {
    r: usize,
    c: usize,
    val: Val,
    formula: Option<String>,
}

fn parse_shared_strings(data: &[u8]) -> Vec<Arc<str>> {
    let mut out = Vec::new();
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(false);
    let mut cur = String::new();
    let mut in_si = false;
    let mut in_t = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let n = e.local_name();
                let n = n.as_ref();
                if n == b"si" {
                    in_si = true;
                    cur.clear();
                } else if n == b"t" && in_si {
                    in_t = true;
                }
            }
            Ok(Event::End(e)) => {
                let n = e.local_name();
                let n = n.as_ref();
                if n == b"si" {
                    in_si = false;
                    out.push(Arc::from(cur.as_str()));
                } else if n == b"t" {
                    in_t = false;
                }
            }
            Ok(Event::Text(t)) => {
                if in_t {
                    let s = std::str::from_utf8(t.as_ref()).unwrap_or("");
                    cur.push_str(&unescape(s));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn parse_sheet(data: &[u8], shared: &[Arc<str>]) -> (Vec<RawCell>, usize, usize) {
    let mut cells = Vec::new();
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut maxr = 0usize;
    let mut maxc = 0usize;
    let mut cur_r = 0usize;
    let mut cur_c = 0usize;
    let mut cur_t: u8 = b'n';
    let mut in_c = false;
    let mut in_v = false;
    let mut in_f = false;
    let mut in_is_t = false;
    let mut vtext = String::new();
    let mut ftext: Option<String> = None;
    let mut istext = String::new();
    let mut has_is = false;
    let mut in_sheetdata = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let is_empty = matches!(reader.decoder(), _) && false;
                let _ = is_empty;
                let n = e.local_name();
                let n = n.as_ref();
                match n {
                    b"sheetData" => in_sheetdata = true,
                    b"c" if in_sheetdata => {
                        in_c = true;
                        cur_t = b'n';
                        vtext.clear();
                        ftext = None;
                        istext.clear();
                        has_is = false;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                b"r" => {
                                    let (r, c) = parse_cell_ref(&a.value);
                                    cur_r = r;
                                    cur_c = c;
                                }
                                b"t" => {
                                    cur_t = match a.value.as_ref() {
                                        b"s" => b's',
                                        b"str" => b'r',
                                        b"b" => b'b',
                                        b"e" => b'e',
                                        b"inlineStr" => b'i',
                                        _ => b'n',
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                    b"v" if in_c => in_v = true,
                    b"f" if in_c => {
                        in_f = true;
                        ftext = Some(String::new());
                    }
                    b"is" if in_c => has_is = true,
                    b"t" if has_is => in_is_t = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let n = e.local_name();
                let n = n.as_ref();
                match n {
                    b"c" if in_c => {
                        in_c = false;
                        let val = if has_is {
                            Val::S(Arc::from(istext.as_str()))
                        } else {
                            match cur_t {
                                b's' => {
                                    let i: usize = vtext.trim().parse().unwrap_or(0);
                                    shared.get(i).map(|s| Val::S(s.clone())).unwrap_or(Val::Empty)
                                }
                                b'r' => Val::S(Arc::from(vtext.as_str())),
                                b'b' => Val::B(vtext.trim() == "1"),
                                b'e' => Val::E(match vtext.trim() {
                                    "#DIV/0!" => Er::Div0,
                                    "#N/A" => Er::NA,
                                    "#NUM!" => Er::Num,
                                    "#REF!" => Er::Ref,
                                    "#NAME?" => Er::Name,
                                    "#NULL!" => Er::Null,
                                    _ => Er::Value,
                                }),
                                _ => {
                                    let t = vtext.trim();
                                    if t.is_empty() {
                                        Val::Empty
                                    } else {
                                        match t.parse::<f64>() {
                                            Ok(n) => Val::N(n),
                                            Err(_) => Val::Empty,
                                        }
                                    }
                                }
                            }
                        };
                        let formula = ftext.take();
                        if !(matches!(val, Val::Empty) && formula.is_none()) {
                            if cur_r + 1 > maxr {
                                maxr = cur_r + 1;
                            }
                            if cur_c + 1 > maxc {
                                maxc = cur_c + 1;
                            }
                            cells.push(RawCell { r: cur_r, c: cur_c, val, formula });
                        }
                    }
                    b"v" => in_v = false,
                    b"f" => in_f = false,
                    b"t" => in_is_t = false,
                    b"is" => {}
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if in_v || in_f || in_is_t {
                    let s = std::str::from_utf8(t.as_ref()).unwrap_or("");
                    let s = unescape(s);
                    if in_v {
                        vtext.push_str(&s);
                    } else if in_f {
                        if let Some(f) = ftext.as_mut() {
                            f.push_str(&s);
                        }
                    } else {
                        istext.push_str(&s);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    (cells, maxr, maxc)
}

fn attr<'a>(e: &'a quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref() == key || a.key.local_name().as_ref() == key {
            return Some(String::from_utf8_lossy(&a.value).to_string());
        }
    }
    None
}

pub fn load(path: &str) -> Book {
    let f = std::fs::File::open(path).expect("open xlsx");
    let mut z = zip::ZipArchive::new(f).expect("zip");
    // workbook sheets
    let wb = read_zip_file(&mut z, "xl/workbook.xml").expect("workbook.xml");
    let mut sheets_meta: Vec<(String, String)> = Vec::new(); // name, rid
    {
        let mut reader = Reader::from_reader(&wb[..]);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.local_name().as_ref() == b"sheet" {
                        let name = attr(&e, b"name").unwrap_or_default();
                        let rid = attr(&e, b"id").unwrap_or_default();
                        sheets_meta.push((unescape(&name), rid));
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let mut rels: HashMap<String, String> = HashMap::new();
    if let Some(rl) = read_zip_file(&mut z, "xl/_rels/workbook.xml.rels") {
        let mut reader = Reader::from_reader(&rl[..]);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.local_name().as_ref() == b"Relationship" {
                        let id = attr(&e, b"Id").unwrap_or_default();
                        let t = attr(&e, b"Target").unwrap_or_default();
                        rels.insert(id, t);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let shared = match read_zip_file(&mut z, "xl/sharedStrings.xml") {
        Some(d) => parse_shared_strings(&d),
        None => Vec::new(),
    };
    let mut book = Book { sheets: Vec::new(), formulas: Vec::new(), sheet_names: HashMap::new() };
    for (si, (name, rid)) in sheets_meta.iter().enumerate() {
        let target = rels.get(rid).cloned().unwrap_or_else(|| format!("worksheets/sheet{}.xml", si + 1));
        let path = if target.starts_with('/') {
            target[1..].to_string()
        } else if target.starts_with("xl/") {
            target.clone()
        } else {
            format!("xl/{}", target)
        };
        let data = read_zip_file(&mut z, &path).unwrap_or_default();
        let (raw, nr, nc) = parse_sheet(&data, &shared);
        let mut sh = Sheet { name: name.clone(), nrows: nr, ncols: nc, cells: vec![Val::Empty; nr * nc], fidx: vec![u32::MAX; nr * nc] };
        for rc in raw {
            let i = rc.r * nc + rc.c;
            sh.cells[i] = rc.val;
            if let Some(f) = rc.formula {
                sh.fidx[i] = book.formulas.len() as u32;
                book.formulas.push(Formula {
                    sheet: si,
                    row: rc.r as u32,
                    col: rc.c as u32,
                    text: f,
                    expr: None,
                    state: Cell::new(0),
                    val: RefCell::new(Val::Empty),
                });
            }
        }
        book.sheet_names.insert(name.to_lowercase(), si);
        book.sheets.push(sh);
    }
    book
}
