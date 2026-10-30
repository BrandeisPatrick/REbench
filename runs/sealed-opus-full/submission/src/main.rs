#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
mod io;
mod parse;
mod funcs;
mod fmt;

use parse::*;
use std::rc::Rc;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum V { N(f64), S(Rc<str>), B(bool), E(u8), Empty, R(u16, u32, u32, u32, u32), A(Rc<Arr>) }

#[derive(Debug)]
pub struct Arr { pub rows: usize, pub cols: usize, pub d: Vec<V> }

#[derive(Clone)]
pub enum Cell { Empty, V(V), F(u32) }

pub struct Sheet { pub nrows: u32, pub ncols: u32, pub cells: Vec<Cell> }

pub struct Formula { pub sheet: u16, pub row: u32, pub col: u32, pub expr: Expr }

pub struct Eng {
    pub sheets: Vec<Sheet>,
    pub forms: Vec<Formula>,
    pub res: Vec<V>,
    pub state: Vec<u8>,
    pub cur: (u16, u32, u32),
    pub allbool: bool,
    pub last_ref: bool,
    pub fempty: Vec<bool>,
    pub range_cache: HashMap<(u16, u32, u32, u32, u32), Rc<Arr>>,
    pub logical_seen: bool,
    pub crit_cache: HashMap<((u16, u32, u32, u32, u32), String), Rc<Vec<u64>>>,
    pub lookup_cache: HashMap<(u16, u32, u32, u32, u32), Rc<HashMap<LKey, u32>>>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub enum LKey { N(u64), S(String), B(bool) }

pub const ERR_NAMES: [&str; 8] = ["", "#DIV/0!", "#VALUE!", "#N/A", "#NUM!", "#REF!", "#NAME?", "#NULL!"];

impl Eng {
    #[inline]
    pub fn cell(&mut self, sh: u16, r: u32, c: u32) -> V {
        let s = match self.sheets.get(sh as usize) { Some(s) => s, None => return V::E(5) };
        if r >= s.nrows || c >= s.ncols { return V::Empty; }
        match &s.cells[(r * s.ncols + c) as usize] {
            Cell::Empty => V::Empty,
            Cell::V(v) => v.clone(),
            Cell::F(i) => { let i = *i as usize; self.eval_formula(i) }
        }
    }
    pub fn eval_formula(&mut self, i: usize) -> V {
        match self.state[i] {
            2 => return self.res[i].clone(),
            1 => return V::E(2),
            _ => {}
        }
        self.state[i] = 1;
        let save = self.cur;
        let save_l = self.logical_seen;
        self.logical_seen = false;
        let f = &self.forms[i];
        self.cur = (f.sheet, f.row, f.col);
        let e: *const Expr = &f.expr;
        let v = self.eval(unsafe { &*e });
        let v = self.deref_scalar(v);
        if matches!(v, V::Empty) { self.fempty[i] = true; }
        let v = match v { V::Empty => V::N(0.0), V::N(x) if !x.is_finite() => V::E(4), x => x };
        self.cur = save;
        self.logical_seen = save_l;
        self.res[i] = v.clone();
        self.state[i] = 2;
        v
    }
    // convert R/A to scalar (implicit intersection)
    pub fn deref_scalar(&mut self, v: V) -> V {
        match v {
            V::R(sh, r1, c1, r2, c2) => {
                if r1 == r2 && c1 == c2 { return self.cell(sh, r1, c1); }
                let (_, cr, cc) = self.cur;
                if c1 == c2 && cr >= r1 && cr <= r2 { return self.cell(sh, cr, c1); }
                if r1 == r2 && cc >= c1 && cc <= c2 { return self.cell(sh, r1, cc); }
                V::E(2)
            }
            V::A(a) => a.d.first().cloned().unwrap_or(V::E(2)),
            x => x,
        }
    }
    pub fn cell_fe(&self, sh: u16, r: u32, c: u32) -> bool {
        if let Some(s) = self.sheets.get(sh as usize) {
            if r < s.nrows && c < s.ncols { if let Cell::F(i) = &s.cells[(r * s.ncols + c) as usize] { return self.state[*i as usize] == 2 && self.fempty[*i as usize]; } }
        }
        false
    }
    pub fn is_fe(&self, e: &Expr) -> bool {
        if let Expr::Ref(sh, r, c) = e { self.cell_fe(*sh, *r, *c) } else { false }
    }
    pub fn sc(&mut self, e: &Expr) -> V { let v = self.eval(e); self.deref_scalar(v) }

    pub fn eval(&mut self, e: &Expr) -> V {
        let v = self.eval_inner(e);
        match e { Expr::Ref(..) | Expr::Func(..) => {}, _ => self.last_ref = false }
        v
    }
    fn eval_inner(&mut self, e: &Expr) -> V {
        match e {
            Expr::Num(x) => V::N(*x),
            Expr::Str(s) => V::S(Rc::from(&**s)),
            Expr::Bool(b) => V::B(*b),
            Expr::Err(c) => V::E(*c),
            Expr::Missing => V::Empty,
            Expr::Ref(sh, r, c) => { let v = self.cell(*sh, *r, *c); self.last_ref = true; v }
            Expr::Range(sh, r1, c1, r2, c2) => V::R(*sh, *r1, *c1, *r2, *c2),
            Expr::Neg(x) => {
                let v = self.eval(x);
                self.map1(v, &|v| match to_num(&v) { Ok(n) => V::N(-n), Err(e) => V::E(e) })
            }
            Expr::Pct(x) => {
                let v = self.eval(x);
                self.map1(v, &|v| match to_num(&v) { Ok(n) => V::N(n / 100.0), Err(e) => V::E(e) })
            }
            Expr::Bin(op, a, b) => {
                let x = self.eval(a);
                let lref = self.last_ref;
                let y = self.eval(b);
                self.last_ref = false;
                let arr = matches!(x, V::A(_)) || matches!(y, V::A(_)) || (matches!(x, V::R(..)) && !is_single(&x)) || (matches!(y, V::R(..)) && !is_single(&y));
                if arr {
                    let xa = self.to_arr(x);
                    let ya = self.to_arr(y);
                    let rows = if xa.rows == 1 { ya.rows } else if ya.rows == 1 { xa.rows } else { xa.rows.min(ya.rows) };
                    let cols = if xa.cols == 1 { ya.cols } else if ya.cols == 1 { xa.cols } else { xa.cols.min(ya.cols) };
                    let mut d = Vec::with_capacity(rows * cols);
                    for r in 0..rows { for c in 0..cols {
                        let xv = &xa.d[(if xa.rows == 1 { 0 } else { r }) * xa.cols + if xa.cols == 1 { 0 } else { c }];
                        let yv = &ya.d[(if ya.rows == 1 { 0 } else { r }) * ya.cols + if ya.cols == 1 { 0 } else { c }];
                        d.push(binop(*op, xv, yv));
                    }}
                    return V::A(Rc::new(Arr { rows, cols, d }));
                }
                let mut x = self.deref_scalar(x);
                let mut y = self.deref_scalar(y);
                if *op >= EQ {
                    if self.is_fe(a) { x = V::Empty; }
                    if self.is_fe(b) { y = V::Empty; }
                }
                self.logical_seen = *op >= EQ;
                if let V::E(ey) = &y { if matches!(&**b, Expr::Func(n, _) if *n == "XLOOKUP") { return V::E(*ey); } }
                if let V::E(ex) = &x { if !lref { return V::E(*ex); } }
                binop(*op, &x, &y)
            }
            Expr::Func(name, args) => match { let r = self.call(name, args); if *name != "IF" && *name != "CHOOSE" { self.last_ref = false; } r } { V::N(x) if !x.is_finite() => V::E(4), V::B(b) => { self.logical_seen = true; V::B(b) } v => v },
        }
    }
    pub fn map1(&mut self, v: V, f: &dyn Fn(V) -> V) -> V {
        if matches!(v, V::A(_)) || (matches!(v, V::R(..)) && !is_single(&v)) {
            let a = self.to_arr(v);
            return V::A(Rc::new(Arr { rows: a.rows, cols: a.cols, d: a.d.iter().map(|x| f(x.clone())).collect() }));
        }
        let v = self.deref_scalar(v);
        f(v)
    }
    pub fn to_arr(&mut self, v: V) -> Rc<Arr> {
        match v {
            V::A(a) => a,
            V::R(sh, r1, c1, r2, c2) => {
                let mut d = Vec::with_capacity(((r2 - r1 + 1) * (c2 - c1 + 1)) as usize);
                for r in r1..=r2 { for c in c1..=c2 { d.push(self.cell(sh, r, c)); } }
                Rc::new(Arr { rows: (r2 - r1 + 1) as usize, cols: (c2 - c1 + 1) as usize, d })
            }
            x => Rc::new(Arr { rows: 1, cols: 1, d: vec![x] }),
        }
    }
}

pub fn is_single(v: &V) -> bool { matches!(v, V::R(_, r1, c1, r2, c2) if r1 == r2 && c1 == c2) }

pub fn parse_num_str(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() { return None; }
    if t.eq_ignore_ascii_case("TRUE") { return Some(1.0); }
    if t.eq_ignore_ascii_case("FALSE") { return Some(0.0); }
    let b = t.as_bytes();
    // reject things like "inf", "nan"
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        // percent
        if let Some(p) = t.strip_suffix('%') { return parse_num_str(p).map(|x| x / 100.0); }
        if let Some(p) = t.strip_prefix('$') { return parse_num_str(p); }
        if t.contains(',') && t.bytes().all(|c| c.is_ascii_digit() || c == b',' || c == b'.' || c == b'-') {
            let parts: Vec<&str> = t.split('.').next().unwrap().split(',').collect();
            if parts.len() > 1 && parts[1..].iter().all(|p| p.len() == 3) && !parts[0].is_empty() { return t.replace(',', "").parse().ok(); }
        }
        return fmt::parse_date_str(t);
    }
    match t.parse::<f64>() { Ok(x) => Some(x), Err(_) => fmt::parse_date_str(t) }
}

pub fn to_num(v: &V) -> Result<f64, u8> {
    match v {
        V::N(x) => Ok(*x),
        V::B(b) => Ok(if *b { 1.0 } else { 0.0 }),
        V::Empty => Ok(0.0),
        V::S(s) => parse_num_str(s).ok_or(2),
        V::E(e) => Err(*e),
        _ => Err(2),
    }
}

pub fn to_str(v: &V) -> Result<Rc<str>, u8> {
    match v {
        V::N(x) => if x.is_finite() { Ok(fmt::num_to_str(*x).into()) } else { Err(4) },
        V::S(s) => Ok(s.clone()),
        V::B(b) => Ok(if *b { "1".into() } else { "0".into() }),
        V::Empty => Ok("".into()),
        V::E(e) => Err(*e),
        _ => Err(2),
    }
}

pub fn to_bool(v: &V) -> Result<bool, u8> {
    match v {
        V::N(x) => Ok(*x != 0.0),
        V::B(b) => Ok(*b),
        V::Empty => Ok(false),
        V::S(s) => match parse_num_str(s) { Some(x) => Ok(x != 0.0), None => Err(2) },
        V::E(e) => Err(*e),
        _ => Err(2),
    }
}

pub fn lower(s: &str) -> std::borrow::Cow<str> {
    if s.bytes().any(|b| b.is_ascii_uppercase() || b >= 128) {
        if s.is_ascii() { return std::borrow::Cow::Owned(s.to_ascii_lowercase()); }
        return std::borrow::Cow::Owned(s.to_lowercase());
    }
    if false { std::borrow::Cow::Owned(s.to_lowercase()) } else { std::borrow::Cow::Borrowed(s) }
}

// compare two scalars: returns ordering; Empty acts as 0 / "" / false depending on other
pub fn cmp_vals(a: &V, b: &V) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    fn rank(v: &V) -> u8 { match v { V::N(_) => 0, V::S(_) => 1, V::B(_) => 0, _ => 0 } }
    let (a2, b2);
    let a = if let V::Empty = a { a2 = match b { V::S(_) => V::S("".into()), V::B(_) => V::B(false), _ => V::N(0.0) }; &a2 } else { a };
    let b = if let V::Empty = b { b2 = match a { V::S(_) => V::S("".into()), V::B(_) => V::B(false), _ => V::N(0.0) }; &b2 } else { b };
    match (a, b) {
        (V::N(x), V::N(y)) => x.partial_cmp(y).unwrap_or(Equal),
        (V::S(x), V::S(y)) => coll_cmp(x, y),
        (V::B(x), V::B(y)) => x.cmp(y),
        (V::B(x), V::N(y)) => (if *x {1.0} else {0.0}).partial_cmp(y).unwrap_or(Equal),
        (V::N(x), V::B(y)) => x.partial_cmp(&(if *y {1.0} else {0.0})).unwrap_or(Equal),
        _ => rank(a).cmp(&rank(b)),
    }
}

pub fn binop(op: u8, x: &V, y: &V) -> V {
    if op >= EQ {
        if let V::E(e) = y { return V::E(*e); }
        if let V::E(e) = x { return V::E(*e); }
        let o = cmp_vals(x, y);
        use std::cmp::Ordering::*;
        let r = match op { EQ => o == Equal, NE => o != Equal, LT => o == Less, GT => o == Greater, LE => o != Greater, _ => o != Less };
        return V::B(r);
    }
    if op == CAT {
        let b = match to_str(y) { Ok(s) => s, Err(e) => return V::E(e) };
        let a = match to_str(x) { Ok(s) => s, Err(e) => return V::E(e) };
        let mut s = String::with_capacity(a.len() + b.len());
        s.push_str(&a); s.push_str(&b);
        return V::S(s.into());
    }
    let b = match to_num(y) { Ok(s) => s, Err(e) => return V::E(e) };
    let a = match to_num(x) { Ok(s) => s, Err(e) => return V::E(e) };
    let r = match op {
        ADD => a + b, SUB => a - b, MUL => a * b,
        DIV => { if b == 0.0 { return V::E(1); } a / b }
        _ => {
            if a == 0.0 && b < 0.0 { return V::E(4); }
            if a < 0.0 && b.fract() != 0.0 {
                let inv = 1.0 / b;
                if (inv - inv.round()).abs() < 1e-10 && (inv.round() as i64) % 2 != 0 { return V::N(-(-a).powf(b)); }
                return V::E(4);
            }
            let r = a.powf(b);
            if r.is_nan() { return V::E(4); }
            r
        }
    };
    if !r.is_finite() { return V::E(4); }
    V::N(r)
}

fn json_str(o: &mut String, s: &str) {
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let mut inp = String::new(); let mut out = String::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--in" { inp = args[i + 1].clone(); i += 1; }
        else if args[i] == "--out" { out = args[i + 1].clone(); i += 1; }
        i += 1;
    }
    let t0 = std::time::Instant::now();
    let tm = std::env::var("CALC_TIME").is_ok();
    let data = std::fs::read(&inp).expect("read");
    let files = io::read_zip(&data);
    let gs = |n: &str| files.get(n).map(|b| String::from_utf8_lossy(b));
    let wb = gs("xl/workbook.xml").unwrap_or_default();
    let rels = gs("xl/_rels/workbook.xml.rels").unwrap_or_default();
    let mut relmap = HashMap::new();
    {
        let mut rest: &str = &rels;
        while let Some(p) = rest.find("<Relationship ") {
            let a = &rest[p..]; let e = a.find('>').unwrap(); let tag = &a[..e];
            if let (Some(id), Some(t)) = (io::attr(tag, "Id"), io::attr(tag, "Target")) {
                let t = t.trim_start_matches('/');
                let t = if t.starts_with("xl/") { t.to_string() } else { format!("xl/{}", t) };
                relmap.insert(id.to_string(), t);
            }
            rest = &a[e..];
        }
    }
    let mut names = Vec::new(); let mut paths = Vec::new();
    {
        let mut rest: &str = &wb;
        while let Some(p) = rest.find("<sheet ") {
            let a = &rest[p..]; let e = a.find('>').unwrap(); let tag = &a[..e];
            names.push(io::unescape(io::attr(tag, "name").unwrap_or("")));
            let rid = io::attr(tag, "r:id").unwrap_or("");
            paths.push(relmap.get(rid).cloned().unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", names.len())));
            rest = &a[e..];
        }
    }
    if tm { eprintln!("unzip {:?}", t0.elapsed()); }
    let ss = gs("xl/sharedStrings.xml").map(|s| io::shared_strings(&s)).unwrap_or_default();
    let mut eng = Eng { sheets: Vec::new(), forms: Vec::new(), res: Vec::new(), state: Vec::new(), cur: (0, 0, 0), allbool: false, last_ref: false, fempty: Vec::new(), range_cache: HashMap::new(), logical_seen: false, crit_cache: HashMap::new(), lookup_cache: HashMap::new() };
    for (si, p) in paths.iter().enumerate() {
        let x = gs(p).unwrap_or_default();
        let x: &str = &x;
        let mut raw: Vec<(u32, u32, io::RawCell)> = Vec::new();
        if x.len() > 2_000_000 {
            let xs = x;
            let mut cuts = vec![0usize];
            for k in 1..4 {
                let mut p = xs.len() * k / 4;
                while !xs.is_char_boundary(p) { p += 1; }
                if let Some(q) = xs[p..].find("<row ") { cuts.push(p + q); }
            }
            cuts.push(xs.len());
            cuts.dedup();
            let ssr = &ss;
            let parts: Vec<Vec<(u32, u32, io::RawCell)>> = std::thread::scope(|sc| {
                let hs: Vec<_> = cuts.windows(2).enumerate().map(|(k, w)| {
                    let seg = &xs[w[0]..w[1]];
                    sc.spawn(move || {
                        let mut v = Vec::new();
                        let seg2 = if k == 0 { seg.to_string() } else { format!("<sheetData>{}", seg) };
                        io::parse_sheet(&seg2, ssr, &mut |r, c, rc| v.push((r, c, rc)));
                        v
                    })
                }).collect();
                hs.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for p in parts { raw.extend(p); }
        } else {
            io::parse_sheet(x, &ss, &mut |r, c, rc| raw.push((r, c, rc)));
        }
        if tm { eprintln!("xml {} {:?}", si, t0.elapsed()); }
        let nrows = raw.iter().map(|x| x.0 + 1).max().unwrap_or(0);
        let ncols = raw.iter().map(|x| x.1 + 1).max().unwrap_or(0);
        let mut cells = vec![Cell::Empty; (nrows * ncols) as usize];
        let mut ftexts: Vec<(u32, u32, String)> = Vec::new();
        for (r, c, rc) in raw {
            let cell = match rc {
                io::RawCell::Num(x) => Cell::V(V::N(x)),
                io::RawCell::Str(s) => Cell::V(V::S(s.into())),
                io::RawCell::Bool(b) => Cell::V(V::B(b)),
                io::RawCell::Err(e) => Cell::V(V::E(err_code(&e))),
                io::RawCell::Formula(f) => { ftexts.push((r, c, f)); continue; }
            };
            cells[(r * ncols + c) as usize] = cell;
        }
        if tm { eprintln!("cells {} {:?}", si, t0.elapsed()); }
        let nth = if ftexts.len() > 4000 { 4 } else { 1 };
        let chunk = (ftexts.len() + nth - 1) / nth.max(1);
        let names_ref = &names;
        let parsed: Vec<Expr> = if nth == 1 {
            ftexts.iter().map(|(_, _, f)| parse::parse(f, si as u16, names_ref)).collect()
        } else {
            std::thread::scope(|sc| {
                let hs: Vec<_> = ftexts.chunks(chunk.max(1)).map(|ch| sc.spawn(move || ch.iter().map(|(_, _, f)| parse::parse(f, si as u16, names_ref)).collect::<Vec<Expr>>())).collect();
                hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
            })
        };
        if tm { eprintln!("parsed {} {:?}", si, t0.elapsed()); }
        for ((r, c, _), e) in ftexts.iter().zip(parsed.into_iter()) {
            eng.forms.push(Formula { sheet: si as u16, row: *r, col: *c, expr: e });
            cells[(r * ncols + c) as usize] = Cell::F(eng.forms.len() as u32 - 1);
        }
        eng.sheets.push(Sheet { nrows, ncols, cells });
    }
    if tm { eprintln!("load {:?}", t0.elapsed()); }
    let n = eng.forms.len();
    eng.res = vec![V::Empty; n];
    eng.state = vec![0; n];
    eng.fempty = vec![false; n];
    if std::env::var("CALC_PROF").is_ok() {
        let mut v: Vec<(f64, usize)> = Vec::new();
        for i in 0..n { let t = std::time::Instant::now(); eng.eval_formula(i); v.push((t.elapsed().as_secs_f64(), i)); }
        v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        for (t, i) in v.iter().take(15) { eprintln!("{:.6} {:?}", t, eng.forms[*i].expr); }
    }
    std::panic::set_hook(Box::new(|_| {}));
    for i in 0..n {
        if eng.state[i] == 2 { continue; }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { eng.eval_formula(i); }));
        if r.is_err() {
            for j in 0..n { if eng.state[j] == 1 { eng.state[j] = 2; eng.res[j] = V::E(2); } }
            eng.cur = (0, 0, 0);
        }
    }
    if tm { eprintln!("eval {:?}", t0.elapsed()); }
    // output
    let mut o = String::with_capacity(n * 40);
    o.push('{');
    for (si, name) in names.iter().enumerate() {
        if si > 0 { o.push(','); }
        json_str(&mut o, name);
        o.push_str(":{");
        let mut first = true;
        for (i, f) in eng.forms.iter().enumerate() {
            if f.sheet as usize != si { continue; }
            if !first { o.push(','); }
            first = false;
            o.push('"'); io::push_col(&mut o, f.col); o.push_str(itoa::Buffer::new().format(f.row + 1)); o.push_str("\":");
            match &eng.res[i] {
                V::N(x) => { o.push_str("{\"t\":\"n\",\"v\":"); if x.is_finite() { if x.fract() == 0.0 && x.abs() < 1e15 { o.push_str(itoa::Buffer::new().format(*x as i64)); } else { o.push_str(ryu::Buffer::new().format_finite(*x)); } } else { o.push('0'); } o.push('}'); }
                V::S(s) => { o.push_str("{\"t\":\"s\",\"v\":"); json_str(&mut o, s); o.push('}'); }
                V::B(b) => { o.push_str(if *b { "{\"t\":\"b\",\"v\":true}" } else { "{\"t\":\"b\",\"v\":false}" }); }
                V::E(e) => { o.push_str("{\"t\":\"e\",\"v\":\""); o.push_str(ERR_NAMES[*e as usize]); o.push_str("\"}"); }
                _ => { o.push_str("{\"t\":\"n\",\"v\":0}"); }
            }
        }
        o.push('}');
    }
    o.push('}');
    std::fs::write(&out, o).expect("write");
    if tm { eprintln!("write {:?}", t0.elapsed()); }
    std::process::exit(0);
}

fn main() {
    let h = std::thread::Builder::new().stack_size(1 << 30).spawn(run).unwrap();
    h.join().unwrap();
}

pub fn refish(e: &Expr) -> bool {
    match e {
        Expr::Ref(..) => true,
        Expr::Func(n, args) if matches!(*n, "IF" | "CHOOSE") => args.iter().skip(1).any(|x| matches!(x, Expr::Ref(..))),
        _ => false,
    }
}

pub fn refish_val(e: &Expr) -> bool {
    match e {
        Expr::Ref(..) => true,
        Expr::Func(n, args) if matches!(*n, "IF" | "CHOOSE" | "SWITCH" | "IFS" | "IFERROR" | "IFNA") => args.iter().skip(1).any(refish_val),
        Expr::Func(n, _) if matches!(*n, "INDEX" | "XLOOKUP") => true,
        _ => false,
    }
}

const COLL: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";
fn coll_key(c: char) -> u32 {
    let lc = c.to_ascii_lowercase();
    if lc.is_ascii() { if let Some(p) = COLL.find(lc) { return p as u32; } return 200 + lc as u32; }
    let l = c.to_lowercase().next().unwrap_or(c);
    1000 + l as u32
}
pub fn coll_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.chars(); let mut ib = b.chars();
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => { let o = coll_key(x).cmp(&coll_key(y)); if o != std::cmp::Ordering::Equal { return o; } }
        }
    }
}
