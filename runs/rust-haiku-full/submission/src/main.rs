mod eval;
mod parse;

use eval::{parse_num, ErrK, Book, Form, Sheet, Slot, Val};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn col_index(s: &str) -> u32 {
    let mut col: u32 = 0;
    for ch in s.bytes() {
        if ch.is_ascii_alphabetic() {
            col = col * 26 + (ch.to_ascii_uppercase() - b'A' + 1) as u32;
        }
    }
    col.max(1) - 1
}

fn parse_ref_attr(r: &str) -> (u32, u32) {
    let split = r.find(|c: char| c.is_ascii_digit()).unwrap_or(r.len());
    let col = col_index(&r[..split]);
    let row: u32 = r[split..].parse().unwrap_or(1);
    (row.max(1) - 1, col)
}

fn norm_target(t: &str) -> String {
    if let Some(x) = t.strip_prefix('/') {
        x.to_string()
    } else {
        format!("xl/{}", t)
    }
}

fn run_eval(inp: &str, outp: &str) -> Result<(), String> {
    let file = std::fs::File::open(inp).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    // shared strings
    let mut sst: Vec<Rc<str>> = Vec::new();
    if let Some(x) = read_entry(&mut zip, "xl/sharedStrings.xml") {
        let doc = roxmltree::Document::parse(&x).map_err(|e| e.to_string())?;
        for si in doc.descendants().filter(|n| n.has_tag_name("si")) {
            let mut s = String::new();
            for t in si.descendants().filter(|n| n.has_tag_name("t")) {
                if let Some(tx) = t.text() {
                    s.push_str(tx);
                }
            }
            sst.push(Rc::from(s.as_str()));
        }
    }

    // workbook & rels
    let wb = read_entry(&mut zip, "xl/workbook.xml").ok_or("no workbook")?;
    let wbdoc = roxmltree::Document::parse(&wb).map_err(|e| e.to_string())?;
    let rels = read_entry(&mut zip, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let reldoc = roxmltree::Document::parse(&rels).map_err(|e| e.to_string())?;
    let mut relmap: HashMap<String, String> = HashMap::new();
    for n in reldoc.descendants().filter(|n| n.has_tag_name("Relationship")) {
        if let (Some(id), Some(t)) = (n.attribute("Id"), n.attribute("Target")) {
            relmap.insert(id.to_string(), norm_target(t));
        }
    }
    let mut sheet_defs: Vec<(String, String)> = Vec::new();
    for n in wbdoc.descendants().filter(|n| n.has_tag_name("sheet")) {
        let name = n.attribute("name").unwrap_or("Sheet").to_string();
        let mut rid = None;
        for a in n.attributes() {
            if a.name() == "id" {
                rid = Some(a.value().to_string());
            }
        }
        let path = rid.and_then(|r| relmap.get(&r).cloned()).unwrap_or_default();
        sheet_defs.push((name, path));
    }
    let mut names: HashMap<String, usize> = HashMap::new();
    for (i, (n, _)) in sheet_defs.iter().enumerate() {
        names.insert(n.to_uppercase(), i);
    }

    // read cells
    struct RawCell {
        r: u32,
        c: u32,
        kind: Slot,
        formula: Option<String>,
    }
    let mut raw_sheets: Vec<(String, Vec<RawCell>, usize, usize)> = Vec::new();
    for (name, path) in &sheet_defs {
        let xml = read_entry(&mut zip, path).unwrap_or_default();
        let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
        let mut cells = Vec::new();
        let (mut mr, mut mc) = (0usize, 0usize);
        for cn in doc.descendants().filter(|n| n.has_tag_name("c")) {
            let rf = cn.attribute("r").unwrap_or("A1");
            let (r, c) = parse_ref_attr(rf);
            let t = cn.attribute("t").unwrap_or("n");
            let mut ftext: Option<String> = None;
            let mut has_f = false;
            let mut vtext: Option<&str> = None;
            let mut istext = String::new();
            for ch in cn.children() {
                if ch.has_tag_name("f") {
                    has_f = true;
                    ftext = Some(ch.text().unwrap_or("").to_string());
                } else if ch.has_tag_name("v") {
                    vtext = ch.text();
                } else if ch.has_tag_name("is") {
                    for tt in ch.descendants().filter(|n| n.has_tag_name("t")) {
                        if let Some(x) = tt.text() {
                            istext.push_str(x);
                        }
                    }
                }
            }
            mr = mr.max(r as usize + 1);
            mc = mc.max(c as usize + 1);
            if has_f {
                cells.push(RawCell { r, c, kind: Slot::Empty, formula: ftext });
                continue;
            }
            let kind = match t {
                "s" => {
                    let idx: usize = vtext.and_then(|v| v.trim().parse().ok()).unwrap_or(usize::MAX);
                    match sst.get(idx) {
                        Some(s) if s.is_empty() => Slot::Empty,
                        Some(s) => Slot::Val(Val::Str(s.clone())),
                        None => Slot::Empty,
                    }
                }
                "str" | "inlineStr" => {
                    let s = if t == "str" { vtext.unwrap_or("").to_string() } else { istext };
                    if s.is_empty() { Slot::Empty } else { Slot::Val(Val::Str(Rc::from(s.as_str()))) }
                }
                "b" => Slot::Val(Val::Bool(vtext.map(|v| v.trim() == "1").unwrap_or(false))),
                "e" => match vtext.and_then(|v| ERR_FROM(v)) {
                    Some(e) => Slot::Val(Val::Err(e)),
                    None => Slot::Empty,
                },
                _ => match vtext.and_then(parse_num) {
                    Some(n) => Slot::Val(Val::Num(n)),
                    None => Slot::Empty,
                },
            };
            cells.push(RawCell { r, c, kind, formula: None });
        }
        raw_sheets.push((name.clone(), cells, mr, mc));
    }

    // build book
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut forms: Vec<Form> = Vec::new();
    let mut form_meta: Vec<(usize, u32, u32)> = Vec::new();
    for (si, (name, cells, mr, mc)) in raw_sheets.iter().enumerate() {
        let _ = si;
        let _ = name;
        let _ = (cells, mr, mc);
    }
    // first pass to allocate sheets with dims
    let mut dense: Vec<Vec<Slot>> = Vec::new();
    for (name, cells, mr, mc) in raw_sheets.iter() {
        let mut v: Vec<Slot> = Vec::with_capacity(mr * mc);
        for _ in 0..(mr * mc) {
            v.push(Slot::Empty);
        }
        for rc in cells.iter() {
            let idx = rc.r as usize * mc + rc.c as usize;
            if rc.formula.is_none() {
                // slot content placed below
            }
            let _ = idx;
            let _ = name;
        }
        dense.push(v);
    }
    let _ = &mut sheets;
    for (si, (name, cells, mr, mc)) in raw_sheets.into_iter().enumerate() {
        let mut slots: Vec<Slot> = Vec::with_capacity(mr * mc);
        for _ in 0..(mr * mc) {
            slots.push(Slot::Empty);
        }
        for rc in cells {
            let idx = rc.r as usize * mc + rc.c as usize;
            match rc.formula {
                Some(f) => {
                    let expr = parse::parse_formula(&f, si, &names);
                    let id = forms.len();
                    forms.push(Form { expr: Rc::new(expr), state: 0, cache: Val::Empty });
                    form_meta.push((si, rc.r, rc.c));
                    slots[idx] = Slot::F(id);
                }
                None => {
                    slots[idx] = rc.kind;
                }
            }
        }
        sheets.push(Sheet { name, rows: mr, cols: mc, cells: slots });
    }
    drop(dense);

    let mut book = Book::new(sheets, forms);
    for id in 0..book.forms.len() {
        book.form_val(id);
    }

    // output
    let mut out = String::with_capacity(book.forms.len() * 40 + 64);
    out.push('{');
    let mut first_sheet = true;
    for si in 0..book.sheets.len() {
        if !first_sheet {
            out.push(',');
        }
        first_sheet = false;
        out.push('"');
        json_escape(&book.sheets[si].name, &mut out);
        out.push_str("\":{");
        let mut first = true;
        for (id, (fsi, r, c)) in form_meta.iter().enumerate() {
            if *fsi != si {
                continue;
            }
            if !first {
                out.push(',');
            }
            first = false;
            out.push('"');
            out.push_str(&parse::cell_name(*r, *c));
            out.push_str("\":");
            write_val(&book.forms[id].cache, &mut out);
        }
        out.push('}');
    }
    out.push('}');
    let mut f = std::fs::File::create(outp).map_err(|e| e.to_string())?;
    f.write_all(out.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

#[allow(non_snake_case)]
fn ERR_FROM(v: &str) -> Option<ErrK> {
    match v {
        "#DIV/0!" => Some(ErrK::Div0),
        "#VALUE!" => Some(ErrK::Value),
        "#N/A" => Some(ErrK::NA),
        "#NUM!" => Some(ErrK::Num),
        "#REF!" => Some(ErrK::Ref),
        "#NAME?" => Some(ErrK::Name),
        "#NULL!" => Some(ErrK::Null),
        _ => None,
    }
}

fn json_escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

fn write_val(v: &Val, out: &mut String) {
    match v {
        Val::Empty => out.push_str("{\"t\":\"n\",\"v\":0}"),
        Val::Num(n) => {
            let n = if n.is_finite() { *n } else { 0.0 };
            out.push_str("{\"t\":\"n\",\"v\":");
            let mut b = ryu_like(n);
            if b == "-0" {
                b = "0".into();
            }
            out.push_str(&b);
            out.push('}');
        }
        Val::Str(s) => {
            out.push_str("{\"t\":\"s\",\"v\":\"");
            json_escape(s, out);
            out.push_str("\"}");
        }
        Val::Bool(b) => out.push_str(if *b { "{\"t\":\"b\",\"v\":true}" } else { "{\"t\":\"b\",\"v\":false}" }),
        Val::Err(e) => {
            out.push_str("{\"t\":\"e\",\"v\":\"");
            out.push_str(e.text());
            out.push_str("\"}");
        }
    }
}

fn ryu_like(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{:?}", n)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut inp = None;
    let mut outp = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--in" => {
                inp = args.get(i + 1).cloned();
                i += 1;
            }
            "--out" => {
                outp = args.get(i + 1).cloned();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let (inp, outp) = match (inp, outp) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            eprintln!("usage: calc eval --in book.xlsx --out values.json");
            std::process::exit(2);
        }
    };
    std::panic::set_hook(Box::new(|_| {}));
    let h = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || run_eval(&inp, &outp))
        .unwrap();
    match h.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("panic");
            std::process::exit(1);
        }
    }
}
