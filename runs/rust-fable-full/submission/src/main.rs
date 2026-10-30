#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::Arc;

mod xlsx;
mod parse;
mod eval;
mod fmt;
mod funcs;

pub use eval::*;
pub use parse::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Er {
    Div0,
    Value,
    NA,
    Num,
    Ref,
    Name,
    Null,
}

impl Er {
    pub fn code(self) -> &'static str {
        match self {
            Er::Div0 => "#DIV/0!",
            Er::Value => "#VALUE!",
            Er::NA => "#N/A",
            Er::Num => "#NUM!",
            Er::Ref => "#REF!",
            Er::Name => "#NAME?",
            Er::Null => "#NULL!",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Val {
    N(f64),
    S(Arc<str>),
    B(bool),
    E(Er),
    Empty,
}

pub struct Sheet {
    pub name: String,
    pub nrows: usize,
    pub ncols: usize,
    pub cells: Vec<Val>,
    pub fidx: Vec<u32>, // u32::MAX none
}

impl Sheet {
    #[inline]
    pub fn idx(&self, r: usize, c: usize) -> Option<usize> {
        if r < self.nrows && c < self.ncols {
            Some(r * self.ncols + c)
        } else {
            None
        }
    }
}

pub struct Formula {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
    pub text: String,
    pub expr: Option<Expr>,
    pub state: Cell<u8>, // 0 none, 1 computing, 2 done
    pub val: RefCell<Val>,
}

pub struct Book {
    pub sheets: Vec<Sheet>,
    pub formulas: Vec<Formula>,
    pub sheet_names: HashMap<String, usize>,
}

fn col_name(mut c: usize) -> String {
    let mut s = Vec::new();
    c += 1;
    while c > 0 {
        let r = (c - 1) % 26;
        s.push(b'A' + r as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

fn json_escape(s: &str, out: &mut String) {
    out.push('"');
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
    out.push('"');
}

fn run(inp: &str, outp: &str) {
    let t0 = std::time::Instant::now();
    let prof = std::env::var("CALC_PROF").is_ok();
    let mut book = xlsx::load(inp);
    if prof {
        eprintln!("load {:?}", t0.elapsed());
    }
    // parse formulas
    {
        use rayon::prelude::*;
        let names = &book.sheet_names;
        let items: Vec<(&str, usize)> = book.formulas.iter().map(|f| (f.text.as_str(), f.sheet)).collect();
        let parsed: Vec<Option<Expr>> = items.par_iter().map(|(t, sh)| parse::parse_formula(t, names, *sh)).collect();
        for (f, e) in book.formulas.iter_mut().zip(parsed) {
            f.expr = e;
        }
    }
    if prof {
        eprintln!("parse {:?}", t0.elapsed());
    }
    let ctx = Ctx { book: &book, idx: RefCell::new(HashMap::new()), crit_cache: RefCell::new(HashMap::new()), range_cache: RefCell::new(HashMap::new()) };
    let mut profmap: HashMap<String, (u32, f64)> = HashMap::new();
    let mut out = String::with_capacity(1 << 20);
    out.push('{');
    let mut first_sheet = true;
    for (si, sh) in book.sheets.iter().enumerate() {
        if !first_sheet {
            out.push(',');
        }
        first_sheet = false;
        json_escape(&sh.name, &mut out);
        out.push_str(":{");
        let mut first = true;
        for (fi, f) in book.formulas.iter().enumerate() {
            if f.sheet != si {
                continue;
            }
            let tf = std::time::Instant::now();
            let v = ctx.formula_value(fi);
            if prof {
                let key: String = f.text.split('(').next().unwrap_or("").chars().take(24).collect();
                let e = profmap.entry(key).or_insert((0u32, 0f64));
                e.0 += 1;
                e.1 += tf.elapsed().as_secs_f64();
            }
            if !first {
                out.push(',');
            }
            first = false;
            out.push('"');
            out.push_str(&col_name(f.col as usize));
            out.push_str(&(f.row + 1).to_string());
            out.push_str("\":");
            match v {
                Val::N(n) => {
                    if n.is_finite() {
                        out.push_str("{\"t\":\"n\",\"v\":");
                        if n == n.trunc() && n.abs() < 1e15 {
                            out.push_str(&format!("{}", n as i64));
                        } else {
                            out.push_str(&format!("{:e}", n));
                        }
                        out.push('}');
                    } else {
                        out.push_str("{\"t\":\"e\",\"v\":\"#NUM!\"}");
                    }
                }
                Val::S(s) => {
                    out.push_str("{\"t\":\"s\",\"v\":");
                    json_escape(&s, &mut out);
                    out.push('}');
                }
                Val::B(b) => {
                    out.push_str(if b { "{\"t\":\"b\",\"v\":true}" } else { "{\"t\":\"b\",\"v\":false}" });
                }
                Val::E(e) => {
                    out.push_str("{\"t\":\"e\",\"v\":\"");
                    out.push_str(e.code());
                    out.push_str("\"}");
                }
                Val::Empty => {
                    out.push_str("{\"t\":\"n\",\"v\":0}");
                }
            }
        }
        out.push('}');
    }
    out.push('}');
    if prof {
        eprintln!("calc {:?}", t0.elapsed());
        let mut v: Vec<_> = profmap.into_iter().collect();
        v.sort_by(|a, b| b.1 .1.partial_cmp(&a.1 .1).unwrap());
        for (k, (c, t)) in v.iter().take(12) {
            eprintln!("  {:24} n={:6} t={:.1}ms", k, c, t * 1000.0);
        }
    }
    let mut f = std::fs::File::create(outp).expect("create out");
    f.write_all(out.as_bytes()).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut inp = String::new();
    let mut outp = String::from("values.json");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--in" => {
                inp = args[i + 1].clone();
                i += 1;
            }
            "--out" => {
                outp = args[i + 1].clone();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let (i2, o2) = (inp.clone(), outp.clone());
    let stack_mb: usize = std::env::var("CALC_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
    match std::thread::Builder::new().stack_size(stack_mb << 20).spawn(move || run(&i2, &o2)) {
        Ok(child) => {
            child.join().unwrap();
        }
        Err(_) => run(&inp, &outp),
    }
}
