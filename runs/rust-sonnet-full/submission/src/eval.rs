use crate::fmt::*;
use crate::load::{Cell as LCell, Workbook};
use crate::parse::{parse, Node, Op};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Er { Div0, Value, Na, Num, Ref, Name, Null }
impl Er {
    pub fn from_str(s: &str) -> Er {
        match s { "#DIV/0!" => Er::Div0, "#VALUE!" => Er::Value, "#N/A" => Er::Na, "#NUM!" => Er::Num,
            "#REF!" => Er::Ref, "#NAME?" => Er::Name, "#NULL!" => Er::Null, _ => Er::Value }
    }
    pub fn name(self) -> &'static str {
        match self { Er::Div0 => "#DIV/0!", Er::Value => "#VALUE!", Er::Na => "#N/A", Er::Num => "#NUM!",
            Er::Ref => "#REF!", Er::Name => "#NAME?", Er::Null => "#NULL!" }
    }
}

#[derive(Clone, Debug)]
pub enum V { Empty, N(f64), S(Rc<str>), B(bool), E(Er) }

#[derive(Clone, Copy)]
pub struct Rg { sh: usize, r1: u32, c1: u32, r2: u32, c2: u32 }
impl Rg {
    fn rows(&self) -> usize { (self.r2 - self.r1 + 1) as usize }
    fn cols(&self) -> usize { (self.c2 - self.c1 + 1) as usize }
}

enum Opd { V(V), R(Rg) }

enum GC { Empty, V(V), F(u32) }
struct Sheet { name: String, rows: u32, cols: u32, cells: Vec<GC> }

pub struct Engine {
    sheets: Vec<Sheet>,
    forms: Vec<Node>,
    fsheet: Vec<usize>,
    pos_of: Vec<(u32, u32)>,
    results: RefCell<Vec<V>>,
    state: RefCell<Vec<u8>>,
    cur: Cell<(usize, u32, u32)>,
    gen: Cell<u64>,
    gerr: Cell<Option<Er>>,
    rect_cache: RefCell<HashMap<(usize, u32, u32, u32, u32), Rc<Vec<V>>>>,
    idx_cache: RefCell<HashMap<(usize, u32, u32, u32, u32), Rc<Index>>>,
    mask_cache: RefCell<HashMap<(usize, u32, u32, u32, u32, String), Rc<Vec<u64>>>>,
}
struct Mask { words: Vec<u64> }
impl Mask {
    fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(wi, &w)| { let mut w = w; std::iter::from_fn(move || { if w == 0 { None } else { let t = w.trailing_zeros() as usize; w &= w - 1; Some(wi * 64 + t) } }) })
    }
    fn count(&self) -> usize { self.words.iter().map(|w| w.count_ones() as usize).sum() }
}
struct Index { nums: HashMap<u64, u32>, strs: HashMap<String, u32> }

type R<T> = Result<T, Er>;

fn time_part(t: &str) -> Option<f64> {
    let p: Vec<&str> = t.split(':').collect();
    if p.len() < 2 || p.len() > 3 { return None; }
    let h: f64 = p[0].parse().ok()?; let m: f64 = p[1].parse().ok()?;
    let sec: f64 = if p.len() == 3 { p[2].parse().ok()? } else { 0.0 };
    Some((h * 3600.0 + m * 60.0 + sec) / 86400.0)
}
pub fn str_to_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() { return None; }
    if t.contains(',') && !t.contains(".,") {
        let parts: Vec<&str> = t.split('.').next().unwrap().split(',').collect();
        let neg = parts[0].starts_with('-') || parts[0].starts_with('+');
        let first = if neg { &parts[0][1..] } else { parts[0] };
        if !first.is_empty() && first.len() <= 3 && parts[1..].iter().all(|g| g.len() == 3) && !first.is_empty() {
            let cleaned = t.replace(',', "");
            return cleaned.parse::<f64>().ok();
        }
        return None;
    }
    if t.eq_ignore_ascii_case("true") { return Some(1.0); }
    if t.eq_ignore_ascii_case("false") { return Some(0.0); }
    let b = t.as_bytes();
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        // percent
        if let Some(p) = t.strip_suffix('%') { return str_to_num(p).map(|x| x / 100.0); }
        return iso_date(t);
    }
    if !b.iter().any(|c| c.is_ascii_digit()) { return None; }
    match t.parse::<f64>() { Ok(x) if x.is_finite() => Some(x), _ => iso_date(t) }
}
fn iso_date(t: &str) -> Option<f64> {
    const MN: [&str; 12] = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    let mut tp = t.trim();
    let mut frac = 0.0;
    if let Some((d, tm)) = tp.split_once(' ') {
        if let Some(f) = time_part(tm) { frac = f; tp = d; }
    } else if tp.contains(':') { return time_part(tp); }
    let mkd = |y: i64, m: u32, d: u32| -> Option<f64> {
        if m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m) { Some(ymd_to_serial(y, m, d) as f64 + frac) } else { None }
    };
    let yfix = |y: i64, len: usize| if len <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
    let p: Vec<&str> = tp.split('/').collect();
    if p.len() == 3 {
        let m: u32 = p[0].parse().ok()?; let d: u32 = p[1].parse().ok()?; let y: i64 = p[2].parse().ok()?;
        return mkd(yfix(y, p[2].len()), m, d);
    }
    let p: Vec<&str> = tp.split('-').collect();
    if p.len() == 3 {
        if let Ok(d) = p[0].parse::<u32>() {
            let ml = p[1].to_ascii_lowercase();
            if let Some(mi) = MN.iter().position(|x| ml.starts_with(x)) {
                let y: i64 = p[2].parse().ok()?;
                return mkd(yfix(y, p[2].len()), mi as u32 + 1, d);
            }
        }
    }
    let p: Vec<&str> = t.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 {
        let y: i64 = p[0].parse().ok()?; let m: u32 = p[1].parse().ok()?; let d: u32 = p[2].parse().ok()?;
        if m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m) { return Some(ymd_to_serial(y, m, d) as f64); }
    }
    None
}

fn lower(s: &str) -> String { s.chars().flat_map(|c| c.to_lowercase()).collect() }

fn wild_match(p: &[char], t: &[char]) -> bool {
    // p lowercase pattern with ~ escapes, t lowercase text
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                '*' => { star = Some((pi, ti)); pi += 1; continue; }
                '?' => { if ti < t.len() { pi += 1; ti += 1; continue; } }
                c => {
                    let (lit, adv) = if c == '~' && pi + 1 < p.len() { (p[pi + 1], 2) } else { (c, 1) };
                    if ti < t.len() && t[ti] == lit { pi += adv; ti += 1; continue; }
                }
            }
        } else if ti == t.len() { return true; }
        if let Some((sp, st)) = star {
            if st < t.len() { star = Some((sp, st + 1)); pi = sp + 1; ti = st + 1; continue; }
        }
        return false;
    }
}
fn has_wild(s: &str) -> bool { s.contains('*') || s.contains('?') || s.contains('~') }

fn pow(x: f64, y: f64) -> R<f64> {
    if x.is_nan() || y.is_nan() { return Ok(f64::NAN); }
    if x == 0.0 && y < 0.0 { return Err(Er::Num); }
    if x == 0.0 && y == 0.0 { return Ok(1.0); }
    let r = if x < 0.0 && y.fract() != 0.0 {
        let inv = 1.0 / y;
        if (inv - inv.round()).abs() < 1e-9 && (inv.round() as i64) % 2 != 0 { -((-x).powf(y)) } else { return Err(Er::Num); }
    } else { x.powf(y) };
    if r.is_finite() { Ok(r) } else { Err(Er::Num) }
}
const PLAIN: bool = false;
struct Ks { s: f64, c: f64, plain: bool }
impl Ks {
    fn new() -> Ks { Ks { s: 0.0, c: 0.0, plain: false } }
    fn add(&mut self, x: f64) {
        if self.plain { self.s += x; return; }
        let t = self.s + x;
        if self.s.abs() >= x.abs() { self.c += (self.s - t) + x; } else { self.c += (x - t) + self.s; }
        self.s = t;
    }
    fn get(&self) -> f64 { self.s + self.c }
}
fn ksum(v: &[f64]) -> f64 { let mut k = Ks::new(); for x in v { k.add(*x); } k.get() }
fn approx_eq(a: f64, b: f64) -> bool {
    if a == b { return true; }
    let d = (a - b).abs();
    d < a.abs().max(b.abs()) * 3.552713678800501e-15 && d.is_finite()
}

pub fn to_str(v: &V) -> R<Rc<str>> {
    match v {
        V::Empty => Ok(Rc::from("")),
        V::N(x) => if x.is_nan() { Err(Er::Value) } else if x.is_infinite() { Err(Er::Num) } else { Ok(Rc::from(num_to_str(*x).as_str())) },
        V::S(s) => Ok(s.clone()),
        V::B(b) => Ok(Rc::from(if *b { "1" } else { "0" })),
        V::E(e) => Err(*e),
    }
}
pub fn to_num(v: &V) -> R<f64> {
    match v {
        V::Empty => Ok(0.0),
        V::N(x) => Ok(*x),
        V::B(b) => Ok(if *b { 1.0 } else { 0.0 }),
        V::S(s) => str_to_num(s).ok_or(Er::Value),
        V::E(e) => Err(*e),
    }
}
fn to_bool(v: &V) -> R<bool> {
    match v {
        V::Empty => Ok(false),
        V::N(x) => if x.is_nan() { Err(Er::Value) } else { Ok(*x != 0.0) },
        V::B(b) => Ok(*b),
        V::S(s) => { let l = s.to_ascii_uppercase(); if l == "TRUE" { Ok(true) } else if l == "FALSE" { Ok(false) } else { str_to_num(s).map(|x| x != 0.0).ok_or(Er::Value) } }
        V::E(e) => Err(*e),
    }
}

fn cmp_vals(a: &V, b: &V) -> Ordering {
    let (a, b) = match (a, b) {
        (V::Empty, V::Empty) => return Ordering::Equal,
        (V::Empty, V::S(_)) => (&V::S(Rc::from("")), b),
        (V::Empty, V::N(_)) => (&V::N(0.0), b),
        (V::Empty, V::B(_)) => (&V::B(false), b),
        (V::S(_), V::Empty) => (a, &V::S(Rc::from(""))),
        (V::N(_), V::Empty) => (a, &V::N(0.0)),
        (V::B(_), V::Empty) => (a, &V::B(false)),
        _ => (a, b),
    };
    let (an, bn);
    let (a, b) = (match a { V::B(x) => { an = V::N(*x as i32 as f64); &an } _ => a }, match b { V::B(x) => { bn = V::N(*x as i32 as f64); &bn } _ => b });
    fn rank(v: &V) -> u8 { match v { V::N(_) => 0, V::S(_) => 1, V::B(_) => 2, _ => 3 } }
    match (a, b) {
        (V::N(x), V::N(y)) => if approx_eq(*x, *y) { Ordering::Equal } else { x.partial_cmp(y).unwrap_or(Ordering::Equal) },
        (V::S(x), V::S(y)) => {
            if x.is_ascii() && y.is_ascii() {
                let xb = x.as_bytes().iter().map(|c| c.to_ascii_lowercase());
                let yb = y.as_bytes().iter().map(|c| c.to_ascii_lowercase());
                xb.cmp(yb)
            } else { lower(x).cmp(&lower(y)) }
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

fn itoa_u(n: u32) -> String { n.to_string() }
fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    c += 1;
    while c > 0 { let r = (c - 1) % 26; s.push(b'A' + r as u8); c = (c - 1) / 26; }
    s.reverse();
    String::from_utf8(s).unwrap()
}

#[derive(Clone, Debug)]
enum CK { Num(f64, Option<String>), Str(String), Blank, NonBlank }
#[derive(Clone)]
struct Crit { op: Op, k: CK, wild: bool, chars: Vec<char> }

fn parse_crit(v: &V) -> Crit {
    let mk = |op, k: CK| { let (wild, chars) = if let CK::Str(s) = &k { (has_wild(s), s.chars().collect()) } else { (false, vec![]) }; Crit { op, k, wild, chars } };
    match v {
        V::N(x) => mk(Op::Eq, CK::Num(*x, None)),
        V::B(b) => mk(Op::Eq, CK::Num(if *b { 1.0 } else { 0.0 }, None)),
        V::Empty => mk(Op::Eq, CK::Num(0.0, None)),
        V::E(_) => mk(Op::Eq, CK::Str("\u{0}err".into())),
        V::S(s) => {
            let (op, rest) = if let Some(r) = s.strip_prefix("<>") { (Op::Ne, r) }
                else if let Some(r) = s.strip_prefix("<=") { (Op::Le, r) }
                else if let Some(r) = s.strip_prefix(">=") { (Op::Ge, r) }
                else if let Some(r) = s.strip_prefix('<') { (Op::Lt, r) }
                else if let Some(r) = s.strip_prefix('>') { (Op::Gt, r) }
                else if let Some(r) = s.strip_prefix('=') { (Op::Eq, r) }
                else { (Op::Eq, &s[..]) };
            if rest.is_empty() {
                return match op { Op::Eq => mk(op, CK::Blank), Op::Ne => mk(op, CK::NonBlank), _ => mk(op, CK::Str(String::new())) };
            }
            if let Some(x) = str_to_num(rest) { return mk(op, CK::Num(x, Some(lower(rest)))); }
            if rest.eq_ignore_ascii_case("true") { return mk(op, CK::Num(1.0, Some(lower(rest)))); }
            if rest.eq_ignore_ascii_case("false") { return mk(op, CK::Num(0.0, Some(lower(rest)))); }
            mk(op, CK::Str(lower(rest)))
        }
    }
}
fn crit_match(c: &Crit, v: &V) -> bool {
    match &c.k {
        CK::Blank => matches!(v, V::Empty) || matches!(v, V::S(s) if s.is_empty()),
        CK::NonBlank => !matches!(v, V::Empty),
        CK::Num(x, t) => match v {
            V::S(s) if t.is_some() && (c.op == Op::Eq || c.op == Op::Ne) => { let m = lower(s) == *t.as_ref().unwrap(); if c.op == Op::Eq { m } else { !m } }
            V::B(b) => { let y = &(*b as i32 as f64); let o = if approx_eq(*x, *y) { Ordering::Equal } else { y.partial_cmp(x).unwrap_or(Ordering::Equal) }; ord_ok(c.op, o) }
            V::N(y) => { let o = if approx_eq(*x, *y) { Ordering::Equal } else { y.partial_cmp(x).unwrap_or(Ordering::Equal) }; ord_ok(c.op, o) }
            _ => c.op == Op::Ne,
        },
        CK::Str(p) => match v {
            V::B(b) => { let sv = V::S(Rc::from(if *b { "TRUE" } else { "FALSE" })); crit_match(c, &sv) }
            V::S(s) => {
                match c.op {
                    Op::Eq | Op::Ne => {
                        let m = if c.wild { let t: Vec<char> = lower(s).chars().collect(); wild_match(&c.chars, &t) }
                            else if s.is_ascii() { s.eq_ignore_ascii_case(p) } else { lower(s) == *p };
                        if c.op == Op::Eq { m } else { !m }
                    }
                    op => ord_ok(op, lower(s).as_str().cmp(p.as_str())),
                }
            }
            _ => c.op == Op::Ne,
        },
    }
}
fn ord_ok(op: Op, o: Ordering) -> bool {
    match op { Op::Eq => o == Ordering::Equal, Op::Ne => o != Ordering::Equal, Op::Lt => o == Ordering::Less,
        Op::Gt => o == Ordering::Greater, Op::Le => o != Ordering::Greater, Op::Ge => o != Ordering::Less, _ => false }
}

impl Engine {
    fn get(&self, sh: usize, r: u32, c: u32) -> V {
        let s = &self.sheets[sh];
        if r >= s.rows || c >= s.cols { return V::Empty; }
        match &s.cells[(r * s.cols + c) as usize] {
            GC::Empty => V::Empty,
            GC::V(v) => v.clone(),
            GC::F(i) => self.fval(*i as usize),
        }
    }
    fn fval(&self, i: usize) -> V {
        let st = self.state.borrow()[i];
        if st == 2 { return self.results.borrow()[i].clone(); }
        if st == 1 { return V::E(Er::Value); }
        self.state.borrow_mut()[i] = 1;
        let save = self.cur.get();
        let pos = self.pos_of[i];
        self.cur.set((self.fsheet[i], pos.0, pos.1));
        let gsave = self.gerr.get();
        self.gerr.set(None);
        let mut v = self.ev(&self.forms[i]);
        if let Some(e) = self.gerr.get() { v = V::E(e); }
        self.gerr.set(gsave);
        self.cur.set(save);
        if let V::Empty = v { v = V::N(0.0); }
        if let V::N(x) = v { if x.is_nan() { v = V::E(Er::Value); } else if !x.is_finite() { v = V::E(Er::Num); } }
        self.results.borrow_mut()[i] = v.clone();
        self.state.borrow_mut()[i] = 2;
        v
    }
}

fn is_missing(n: &Node) -> bool { matches!(n, Node::Func(s, _) if s == "\0MISSING") }

impl Engine {
    fn rect(&self, g: Rg) -> Rc<Vec<V>> {
        let key = (g.sh, g.r1, g.c1, g.r2, g.c2);
        if let Some(r) = self.rect_cache.borrow().get(&key) { return r.clone(); }
        let mut v = Vec::with_capacity(g.rows() * g.cols());
        for r in g.r1..=g.r2 { for c in g.c1..=g.c2 { v.push(self.get(g.sh, r, c)); } }
        let rc = Rc::new(v);
        self.rect_cache.borrow_mut().insert(key, rc.clone());
        rc
    }
    fn index(&self, g: Rg) -> Rc<Index> {
        let key = (g.sh, g.r1, g.c1, g.r2, g.c2);
        if let Some(r) = self.idx_cache.borrow().get(&key) { return r.clone(); }
        let mut ix = Index { nums: HashMap::new(), strs: HashMap::new() };
        for r in g.r1..=g.r2 {
            let i = r - g.r1;
            match self.get(g.sh, r, g.c1) {
                V::N(x) => { ix.nums.entry(if x == 0.0 { 0 } else { x.to_bits() }).or_insert(i); }
                V::S(s) => { ix.strs.entry(lower(&s)).or_insert(i); }
                _ => {}
            }
        }
        let rc = Rc::new(ix);
        self.idx_cache.borrow_mut().insert(key, rc.clone());
        rc
    }
    fn opd(&self, n: &Node) -> Opd {
        match n {
            Node::Ref(sh, r, c) => Opd::R(Rg { sh: *sh, r1: *r, c1: *c, r2: *r, c2: *c }),
            Node::Range(sh, r1, c1, r2, c2) => Opd::R(Rg { sh: *sh, r1: *r1, c1: *c1, r2: *r2, c2: *c2 }),
            _ => Opd::V(self.ev(n)),
        }
    }
    fn rg_of(&self, n: &Node) -> R<Rg> {
        match self.opd(n) { Opd::R(g) => Ok(g), Opd::V(V::E(e)) => Err(e), _ => Err(Er::Value) }
    }
    fn ev(&self, n: &Node) -> V {
        match n {
            Node::Num(x) => V::N(*x),
            Node::Str(s) => V::S(Rc::from(&**s)),
            Node::Bool(b) => V::B(*b),
            Node::Err(e) => V::E(*e),
            Node::Ref(sh, r, c) => self.get(*sh, *r, *c),
            Node::Range(sh, r1, c1, r2, c2) => {
                let (_, cr, cc) = self.cur.get();
                if r1 == r2 && c1 == c2 { return self.get(*sh, *r1, *c1); }
                if c1 == c2 && cr >= *r1 && cr <= *r2 { return self.get(*sh, cr, *c1); }
                if r1 == r2 && cc >= *c1 && cc <= *c2 { return self.get(*sh, *r1, cc); }
                V::E(Er::Value)
            }
            Node::Neg(a) => match to_num(&self.ev(a)) { Ok(x) => V::N(-x), Err(e) => { self.raise(e); V::E(e) } },
            Node::Pct(a) => match to_num(&self.ev(a)) { Ok(x) => V::N(x / 100.0), Err(e) => { self.raise(e); V::E(e) } },
            Node::Bin(op, a, b) => {
                let l = self.ev(a);
                let r = self.ev(b);
                if matches!(op, Op::Cat | Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge) {
                    if let V::E(e) = r { self.raise(e); }
                    if let V::E(e) = l { self.raise(e); }
                    if !matches!(op, Op::Cat) { if matches!(r, V::N(x) if x.is_nan()) || matches!(l, V::N(x) if x.is_nan()) { self.raise(Er::Value); } }
                } else {
                    match &r { V::E(e) => self.raise(*e), v => if let Err(e) = to_num(v) { self.raise(e); } }
                    match &l { V::E(e) => self.raise(*e), v => if let Err(e) = to_num(v) { self.raise(e); } }
                }
                if let Some(e) = self.gerr.get() { return V::E(e); }
                match op {
                    Op::Cat => {
                        let y = match to_str(&r) { Ok(x) => x, Err(e) => { self.raise(e); return V::E(e) } };
                        let x = match to_str(&l) { Ok(x) => x, Err(e) => { self.raise(e); return V::E(e) } };
                        let mut s = String::with_capacity(x.len() + y.len());
                        s.push_str(&x); s.push_str(&y);
                        V::S(Rc::from(s.as_str()))
                    }
                    Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => V::B(ord_ok(*op, cmp_vals(&l, &r))),
                    _ => {
                        let y = match to_num(&r) { Ok(x) => x, Err(e) => { self.raise(e); return V::E(e) } };
                        let x = match to_num(&l) { Ok(x) => x, Err(e) => { self.raise(e); return V::E(e) } };
                        let z = match op {
                            Op::Add => x + y, Op::Sub => x - y, Op::Mul => x * y,
                            Op::Div => { if y == 0.0 { self.raise(Er::Div0); return V::E(Er::Div0); } x / y }
                            Op::Pow => match pow(x, y) { Ok(v) => v, Err(e) => { self.raise(e); return V::E(e) } },
                            _ => unreachable!(),
                        };
                        if z.is_nan() && !x.is_nan() && !y.is_nan() { self.raise(Er::Num); return V::E(Er::Num); }
                        V::N(z)
                    }
                }
            }
            Node::Func(name, args) => self.call(name, args),
        }
    }

    fn num(&self, n: &Node) -> R<f64> { let x = to_num(&self.ev(n))?; if x.is_finite() { Ok(x) } else { Err(Er::Value) } }
    fn text(&self, n: &Node) -> R<Rc<str>> { to_str(&self.ev(n)) }
    fn opt_num(&self, args: &[Node], i: usize, d: f64) -> R<f64> {
        if i < args.len() && !is_missing(&args[i]) { self.num(&args[i]) } else { Ok(d) }
    }

    // visit values of args for aggregate. mode: numbers only
    fn nums(&self, args: &[Node], count_text: bool) -> R<Vec<f64>> {
        let mut out = Vec::new();
        for a in args {
            match self.opd(a) {
                Opd::R(g) => {
                    if g.rows() * g.cols() > 64 || true {
                        let rc = self.rect(g);
                        for v in rc.iter() {
                            match v { V::N(x) => out.push(*x), V::B(b) => out.push(*b as i32 as f64), V::E(e) => return Err(*e), _ => {} }
                        }
                    }
                }
                Opd::V(v) => {
                    if is_missing(a) { continue; }
                    match v {
                        V::E(e) => return Err(e),
                        V::Empty => {}
                        V::S(_) if count_text => return Err(Er::Value),
                        v => out.push(to_num(&v)?),
                    }
                }
            }
        }
        Ok(out)
    }
    fn ok_n(r: R<f64>) -> V { match r { Ok(x) => if x.is_finite() { V::N(x) } else { V::E(Er::Num) }, Err(e) => V::E(e) } }

    fn flat(&self, args: &[Node]) -> R<Vec<V>> {
        let mut out = Vec::new();
        let mut fe = None;
        for a in args {
            match self.opd(a) {
                Opd::R(g) => { let rc = self.rect(g); for v in rc.iter() { if let V::E(e) = v { if fe.is_none() { fe = Some(*e); } } } out.extend(rc.iter().cloned()); }
                Opd::V(v) => { if let V::E(e) = v { if fe.is_none() { fe = Some(e); } } out.push(v) }
            }
        }
        if let Some(e) = fe { return Err(e); }
        Ok(out)
    }

    fn lookup_eq(&self, key: &V, cell: &V, wild: bool) -> bool {
        match (key, cell) {
            (V::N(a), V::N(b)) => approx_eq(*a, *b),
            (V::S(a), V::S(b)) => {
                if wild && has_wild(a) {
                    let p: Vec<char> = lower(a).chars().collect(); let t: Vec<char> = lower(b).chars().collect(); wild_match(&p, &t)
                } else { cmp_vals(key, cell) == Ordering::Equal }
            }
            (V::B(a), V::B(b)) => a == b,
            (V::B(a), V::N(b)) => approx_eq(*a as i32 as f64, *b),
            (V::N(a), V::B(b)) => approx_eq(*a, *b as i32 as f64),
            _ => false,
        }
    }
    // exact find in vector (row-major list of 1-D values). returns index
    fn find_exact(&self, key: &V, vals: &[V], wild: bool) -> Option<usize> {
        vals.iter().position(|c| self.lookup_eq(key, c, wild))
    }
    fn find_exact_rg(&self, key: &V, g: Rg, wild: bool) -> Option<usize> {
        // first column of g, using index when possible
        match key {
            V::N(x) => { let ix = self.index(g); let k = if *x == 0.0 { 0 } else { x.to_bits() }; ix.nums.get(&k).map(|i| *i as usize) }
            V::S(s) if !(wild && has_wild(s)) => { let ix = self.index(g); ix.strs.get(&lower(s)).map(|i| *i as usize) }
            _ => {
                for r in g.r1..=g.r2 { if self.lookup_eq(key, &self.get(g.sh, r, g.c1), wild) { return Some((r - g.r1) as usize); } }
                None
            }
        }
    }
    fn find_approx(&self, key: &V, vals: &[V]) -> Option<usize> {
        let kn = match key { V::B(b) => V::N(*b as i32 as f64), k => k.clone() };
        let same = |c: &V| matches!((&kn, c), (V::N(_), V::N(_)) | (V::N(_), V::B(_)) | (V::S(_), V::S(_)));
        let mut best = None;
        for (i, c) in vals.iter().enumerate() {
            if !same(c) { continue; }
            match cmp_vals(c, &kn) {
                Ordering::Equal => {
                    let mut j = i;
                    while j + 1 < vals.len() && cmp_vals(&vals[j + 1], &kn) == Ordering::Equal && same(&vals[j + 1]) { j += 1; }
                    return Some(j);
                }
                Ordering::Less => { best = Some(i); }
                Ordering::Greater => break,
            }
        }
        best
    }
    fn mask_for(&self, g: Rg, c: &Crit) -> Rc<Vec<u64>> {
        let key = (g.sh, g.r1, g.c1, g.r2, g.c2, format!("{:?}{:?}", c.op, c.k));
        if let Some(m) = self.mask_cache.borrow().get(&key) { return m.clone(); }
        let rc = self.rect(g);
        let mut words = vec![0u64; (rc.len() + 63) / 64];
        for (i, v) in rc.iter().enumerate() { if crit_match(c, v) { words[i >> 6] |= 1u64 << (i & 63); } }
        let m = Rc::new(words);
        self.mask_cache.borrow_mut().insert(key, m.clone());
        m
    }
    fn criteria_scan(&self, pairs: &[(Rg, Crit)]) -> R<Mask> {
        let n = pairs[0].0.rows() * pairs[0].0.cols();
        let mut words: Vec<u64> = Vec::new();
        for (k, (g, c)) in pairs.iter().enumerate() {
            if g.rows() != pairs[0].0.rows() || g.cols() != pairs[0].0.cols() { return Err(Er::Value); }
            let m = self.mask_for(*g, c);
            if k == 0 { words = (*m).clone(); } else { for (w, x) in words.iter_mut().zip(m.iter()) { *w &= *x; } }
        }
        Ok(Mask { words })
    }
    fn parse_pairs(&self, args: &[Node]) -> R<Vec<(Rg, Crit)>> {
        let mut pairs = Vec::new();
        let mut i = 0;
        while i + 1 < args.len() {
            let g = self.rg_of(&args[i])?;
            let cv = self.ev(&args[i + 1]);
            if let V::E(e) = cv { return Err(e); }
            pairs.push((g, parse_crit(&cv)));
            i += 2;
        }
        Ok(pairs)
    }

    fn raise(&self, e: Er) { if self.gerr.get().is_none() { self.gerr.set(Some(e)); } }
    fn call(&self, name: &str, args: &[Node]) -> V {
        match name {
            "IFERROR" | "IFNA" if args.len() == 2 => {
                let saved = self.gerr.get();
                let mut v = self.ev(&args[0]);
                if matches!(v, V::N(x) if x.is_nan()) { v = V::E(Er::Value); } else if matches!(v, V::N(x) if x.is_infinite()) { v = V::E(Er::Num); }
                if let V::E(e) = v {
                    if name == "IFERROR" || e == Er::Na { self.gerr.set(saved); return self.ev(&args[1]); }
                }
                return v;
            }
            "ISERROR" | "ISNA" | "ISNUMBER" | "ISTEXT" | "ISBLANK" | "ISLOGICAL" if args.len() == 1 => {
                let saved = self.gerr.get();
                let mut v = self.ev(&args[0]);
                if matches!(v, V::N(x) if x.is_nan()) { v = V::E(Er::Value); } else if matches!(v, V::N(x) if x.is_infinite()) { v = V::E(Er::Num); }
                self.gerr.set(saved);
                return V::B(match name {
                    "ISERROR" => matches!(v, V::E(_)), "ISNA" => matches!(v, V::E(Er::Na)),
                    "ISNUMBER" => matches!(v, V::N(_) | V::B(_)), "ISTEXT" => matches!(v, V::S(_)),
                    "ISBLANK" => matches!(v, V::Empty), _ => matches!(v, V::B(_)),
                });
            }
            _ => {}
        }
        match self.call_r(name, args) { Ok(v) => v, Err(e) => {
            if self.gerr.get().is_none() && !matches!(name, "IF" | "IFS" | "CHOOSE" | "SWITCH") {
                for a in args { if matches!(a, Node::Func(..) | Node::Bin(..)) { let _ = self.ev(a); } }
            }
            if self.gerr.get().is_none() && !(name == "MOD" && e == Er::Div0) && !matches!(name, "COUNT" | "COUNTA" | "CONCAT" | "CONCATENATE" | "TEXTJOIN" | "AND" | "OR" | "XOR" | "IF" | "IFS" | "CHOOSE" | "SWITCH") {
                for a in args.iter().rev() {
                    if matches!(a, Node::Range(..)) { continue; }
                    if let V::E(e2) = self.ev(a) { self.raise(e2); break; }
                }
            }
            if !(e == Er::Na && matches!(name, "VLOOKUP" | "HLOOKUP" | "XLOOKUP" | "IFS")) { self.raise(e); }
            V::E(e) } }
    }

    fn call_r(&self, name: &str, a: &[Node]) -> R<V> {
        let n = a.len();
        macro_rules! need { ($lo:expr, $hi:expr) => { if n < $lo || n > $hi { return Err(Er::Value); } } }
        Ok(match name {
            "SUM" => { let v = self.nums(a, true)?; V::N(ksum(&v)) }
            "PRODUCT" => { let v = self.nums(a, true)?; if v.is_empty() { V::N(0.0) } else { let mut p = 1.0f64; for x in v.iter().rev() { p *= x; if p.is_infinite() { break; } } V::N(p) } }
            "SUMSQ" => { let v = self.nums(a, true)?; V::N(v.iter().map(|x| x * x).sum()) }
            "MAX" | "MIN" => {
                let is_max = name == "MAX";
                let mut best: Option<(f64, bool)> = None;
                let mut first_b: Option<bool> = None;
                for x in a {
                    let mut items: Vec<(f64, bool)> = Vec::new();
                    match self.opd(x) {
                        Opd::R(g) => { for v in self.rect(g).iter() { match v { V::N(y) => items.push((*y, false)), V::B(y) => items.push((*y as i32 as f64, true)), V::E(e) => return Err(*e), _ => {} } } }
                        Opd::V(v) => { if is_missing(x) { continue; } match v { V::E(e) => return Err(e), V::Empty => {}, V::S(_) => return Err(Er::Value), v => items.push((to_num(&v)?, false)) } }
                    }
                    for it in items {
                        if first_b.is_none() { first_b = Some(it.1); }
                        best = match best { None => Some(it), Some(b) => if (is_max && it.0 > b.0) || (!is_max && it.0 < b.0) { Some(it) } else { Some(b) } };
                    }
                }
                match best { None => V::N(0.0), Some((x, true)) if first_b == Some(true) => V::B(x != 0.0), Some((x, _)) => V::N(x) }
            }
            "AVERAGE" => { let v = self.nums(a, true)?; if v.is_empty() { return Err(Er::Div0); } V::N(ksum(&v) / v.len() as f64) }
            "MEDIAN" => {
                let mut v = self.nums(a, true)?; if v.is_empty() { return Err(Er::Value); }
                v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let m = v.len() / 2;
                V::N(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
            }
            "VAR" | "STDEV" => {
                let v = self.nums(a, true)?; if v.len() < 2 { return Err(Er::Div0); }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (v.len() - 1) as f64;
                V::N(if name == "VAR" { var } else { var.sqrt() })
            }
            "LARGE" | "SMALL" => {
                need!(2, 2);
                let mut v = self.nums(&a[..1], true)?;
                let k = self.num(&a[1])?;
                let k = k.trunc();
                if k < 1.0 || k as usize > v.len() { return Err(Er::Value); }
                v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let k = k as usize;
                V::N(if name == "SMALL" { v[k - 1] } else { v[v.len() - k] })
            }
            "COUNT" => {
                let gs = self.gerr.get();
                let mut c = 0;
                for x in a {
                    match self.opd(x) {
                        Opd::R(g) => { for v in self.rect(g).iter() { if let V::N(_) | V::B(_) = v { c += 1; } } }
                        Opd::V(v) => match v { V::N(_) | V::B(_) => c += 1, V::S(s) => if str_to_num(&s).is_some() { c += 1 }, _ => {} },
                    }
                }
                self.gerr.set(gs);
                V::N(c as f64)
            }
            "COUNTA" => {
                let gs = self.gerr.get();
                let mut c = 0;
                for x in a {
                    match self.opd(x) {
                        Opd::R(g) => { for v in self.rect(g).iter() { if !matches!(v, V::Empty) { c += 1; } } }
                        Opd::V(_) => c += 1,
                    }
                }
                self.gerr.set(gs);
                V::N(c as f64)
            }
            "COUNTBLANK" => {
                need!(1, 1);
                let g = self.rg_of(&a[0])?;
                V::N(self.rect(g).iter().filter(|v| matches!(v, V::Empty) || matches!(v, V::S(s) if s.is_empty())).count() as f64)
            }
            "SUMPRODUCT" => {
                need!(1, 99);
                let mut arrs: Vec<(usize, usize, Rc<Vec<V>>)> = Vec::new();
                for x in a {
                    match self.opd(x) {
                        Opd::R(g) => arrs.push((g.rows(), g.cols(), self.rect(g))),
                        Opd::V(v) => arrs.push((1, 1, Rc::new(vec![v]))),
                    }
                }
                for x in &arrs { if x.0 != arrs[0].0 || x.1 != arrs[0].1 { return Err(Er::Value); } }
                let mut s = Ks::new();
                for i in 0..arrs[0].2.len() {
                    let mut p = 1.0;
                    for x in &arrs {
                        match &x.2[i] { V::N(v) => p *= v, V::E(e) => return Err(*e), V::B(b) => p *= *b as i32 as f64, V::S(_) | V::Empty => { p = 0.0; } }
                    }
                    s.add(p);
                }
                V::N(s.get())
            }
            "SUMIF" | "AVERAGEIF" => {
                need!(2, 3);
                let g = self.rg_of(&a[0])?;
                let cv = self.ev(&a[1]);
                if let V::E(e) = cv { return Err(e); }
                let c = parse_crit(&cv);
                let sg = if n == 3 && !is_missing(&a[2]) { let s = self.rg_of(&a[2])?; Rg { sh: s.sh, r1: s.r1, c1: s.c1, r2: s.r1 + g.r2 - g.r1, c2: s.c1 + g.c2 - g.c1 } } else { g };
                let mask = self.criteria_scan(&[(g, c)])?;
                let sv = self.rect(sg);
                let (mut ks, mut cnt) = (Ks::new(), 0); ks.plain = false;
                for i in mask.ones() {
                    { match &sv[i] { V::N(x) => { ks.add(*x); cnt += 1; } V::B(b) => { ks.add(*b as i32 as f64); cnt += 1; } V::E(e) => return Err(*e), _ => {} } }
                }
                let s = ks.get(); if name == "SUMIF" { V::N(s) } else { if cnt == 0 { return Err(Er::Div0); } V::N(s / cnt as f64) }
            }
            "SUMIFS" | "AVERAGEIFS" => {
                if n < 3 || n % 2 == 0 { return Err(Er::Value); }
                let sg = self.rg_of(&a[0])?;
                let pairs = self.parse_pairs(&a[1..])?;
                if pairs[0].0.rows() != sg.rows() || pairs[0].0.cols() != sg.cols() { return Err(Er::Value); }
                let mask = self.criteria_scan(&pairs)?;
                let sv = self.rect(sg);
                let (mut ks, mut cnt) = (Ks::new(), 0); ks.plain = false;
                for i in mask.ones() {
                    { match &sv[i] { V::N(x) => { ks.add(*x); cnt += 1; } V::B(b) => { ks.add(*b as i32 as f64); cnt += 1; } V::E(e) => return Err(*e), _ => {} } }
                }
                let s = ks.get(); if name == "SUMIFS" { V::N(s) } else { if cnt == 0 { return Err(Er::Div0); } V::N(s / cnt as f64) }
            }
            "COUNTIF" => {
                need!(2, 2);
                let g = self.rg_of(&a[0])?;
                let cv = self.ev(&a[1]);
                if let V::E(e) = cv { return Err(e); }
                let mask = self.criteria_scan(&[(g, parse_crit(&cv))])?;
                V::N(mask.count() as f64)
            }
            "COUNTIFS" => {
                if n < 2 || n % 2 == 1 { return Err(Er::Value); }
                let pairs = self.parse_pairs(a)?;
                let mask = self.criteria_scan(&pairs)?;
                V::N(mask.count() as f64)
            }
            "IF" => {
                need!(2, 3);
                let c = to_bool(&self.ev(&a[0]))?;
                if c { if is_missing(&a[1]) { V::N(0.0) } else { self.ev(&a[1]) } }
                else if n == 3 { if is_missing(&a[2]) { V::N(0.0) } else { self.ev(&a[2]) } } else { V::B(false) }
            }
            "IFS" => {
                if n < 2 || n % 2 == 1 { return Err(Er::Value); }
                let mut i = 0;
                while i < n {
                    if to_bool(&self.ev(&a[i]))? { return Ok(self.ev(&a[i + 1])); }
                    i += 2;
                }
                return Err(Er::Na);
            }
            "IFERROR" => { need!(2, 2); let v = self.ev(&a[0]); if let V::E(_) = v { self.ev(&a[1]) } else { v } }
            "IFNA" => { need!(2, 2); let v = self.ev(&a[0]); if let V::E(Er::Na) = v { self.ev(&a[1]) } else { v } }
            "CHOOSE" => {
                if n < 2 { return Err(Er::Value); }
                let i = self.num(&a[0])?.trunc();
                if i < 1.0 || i as usize >= n { return Err(Er::Value); }
                self.ev(&a[i as usize])
            }
            "SWITCH" => {
                if n < 3 { return Err(Er::Value); }
                let k = self.ev(&a[0]);
                if let V::E(e) = k { return Err(e); }
                let mut i = 1;
                while i + 1 < n {
                    let c = self.ev(&a[i]);
                    if let V::E(e) = c { return Err(e); }
                    if cmp_vals(&k, &c) == Ordering::Equal && std::mem::discriminant(&k) == std::mem::discriminant(&c) || (matches!(k, V::Empty) || matches!(c, V::Empty)) && cmp_vals(&k, &c) == Ordering::Equal { return Ok(self.ev(&a[i + 1])); }
                    i += 2;
                }
                if i < n { return Ok(self.ev(&a[i])); }
                return Err(Er::Na);
            }
            "AND" | "OR" | "XOR" => {
                if n == 0 { return Err(Er::Value); }
                let (mut any, mut acc) = (false, name == "AND");
                let mut xr = false;
                for x in a {
                    let vals: Vec<(V, bool)> = match self.opd(x) {
                        Opd::R(g) => self.rect(g).iter().map(|v| (v.clone(), true)).collect(),
                        Opd::V(v) => vec![(v, false)],
                    };
                    if name == "XOR" && matches!(x, Node::Func(f, ar) if f == "ISLOGICAL" && matches!(ar.first(), Some(Node::Str(_)))) { any = true; xr ^= true; continue; }
                    for (v, inr) in vals {
                        let b = match &v {
                            V::E(e) => return Err(*e),
                            V::Empty => continue,
                            V::S(_) if inr => continue,
                            V::S(_) => return Err(Er::Value),
                            v => to_bool(v)?,
                        };
                        any = true;
                        match name { "AND" => acc = acc && b, "OR" => acc = acc || b, _ => xr ^= b }
                    }
                }
                if !any { return Err(Er::Value); }
                V::B(if name == "XOR" { xr } else { acc })
            }
            "NOT" => { need!(1, 1); V::B(!to_bool(&self.ev(&a[0]))?) }
            "TRUE" => V::B(true),
            "FALSE" => V::B(false),
            "ISBLANK" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::Empty)) }
            "ISNUMBER" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::N(_) | V::B(_))) }
            "ISTEXT" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::S(_))) }
            "ISLOGICAL" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::B(_))) }
            "ISERROR" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::E(_))) }
            "ISNA" => { need!(1, 1); V::B(matches!(self.ev(&a[0]), V::E(Er::Na))) }
            "N" => { need!(1, 1); match self.ev(&a[0]) { V::N(x) if x.is_nan() => return Err(Er::Value), V::N(x) => V::N(x), V::B(b) => V::N(b as i32 as f64), V::E(e) => return Err(e), _ => V::N(0.0) } }
            "T" => { need!(1, 1); match self.ev(&a[0]) { V::N(x) if x.is_nan() => return Err(Er::Value), V::S(s) => V::S(s), V::E(e) => return Err(e), _ => V::S(Rc::from("")) } }
            "ABS" => { need!(1, 1); V::N(self.num(&a[0])?.abs()) }
            "SIGN" => { need!(1, 1); let x = self.num(&a[0])?; V::N(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }) }
            "INT" => { need!(1, 1); V::N(r15(self.num(&a[0])?).floor()) }
            "SQRT" => { need!(1, 1); let x = self.num(&a[0])?; if x < 0.0 { return Err(Er::Value); } V::N(x.sqrt()) }
            "EXP" => { need!(1, 1); { let x = self.num(&a[0])?; let r = x.exp(); if r.is_finite() { V::N(r) } else { return Err(Er::Num) } } }
            "LN" => { need!(1, 1); let x = self.num(&a[0])?; if x <= 0.0 { return Ok(V::N(f64::NAN)); } V::N(x.ln()) }
            "LOG10" => { need!(1, 1); let x = self.num(&a[0])?; if x <= 0.0 { return Ok(V::N(f64::NAN)); } V::N(x.log10()) }
            "LOG" => {
                need!(1, 2);
                let x = self.num(&a[0])?; let b = self.opt_num(a, 1, 10.0)?;
                if x <= 0.0 || b <= 0.0 { return Ok(V::N(f64::NAN)); }
                if b == 1.0 { return Err(Er::Value); }
                V::N(if b == 10.0 { x.log10() } else { x.ln() / b.ln() })
            }
            "PI" => V::N(std::f64::consts::PI),
            "POWER" => {
                need!(2, 2);
                let x = self.num(&a[0])?; let y = self.num(&a[1])?;
                match pow(x, y) { Ok(v) => V::N(v), Err(e) => return Err(e) }
            }
            "MOD" => {
                need!(2, 2);
                let y = self.num(&a[1])?;
                if y == 0.0 { return Err(Er::Div0); }
                let x = self.num(&a[0])?;
                let r = x - y * (x / y).floor();
                V::N(r)
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" => {
                need!(1, 2);
                let x = self.num(&a[0])?; let d = self.opt_num(a, 1, 0.0)?.trunc() as i32;
                if x.is_nan() { return Ok(V::N(x)); }
                match name {
                    "ROUND" => V::N(round_half_away(x, d)),
                    _ => {
                        let up = name == "ROUNDUP";
                        if d > 15 { return Ok(V::N(x)); }
                        let f = 10f64.powi(d.abs());
                        let v = if d >= 0 { x * f } else { x / f };
                        let v15: f64 = { let (d, e) = sig15(v); format!("{}{}.{}e{}", if v < 0.0 { "-" } else { "" }, &d[..1], if d.len() > 1 { &d[1..] } else { "0" }, e).parse().unwrap_or(v) };
                        let r = if up { if v15 >= 0.0 { v15.ceil() } else { v15.floor() } } else { v15.trunc() };
                        V::N(if d >= 0 { r / f } else { r * f })
                    }
                }
            }
            "LEFT" | "RIGHT" => {
                need!(1, 2);
                let s = self.text(&a[0])?; let k = self.opt_num(a, 1, 1.0)?.trunc();
                if k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect(); let k = (k as usize).min(ch.len());
                let r: String = if name == "LEFT" { ch[..k].iter().collect() } else { ch[ch.len() - k..].iter().collect() };
                V::S(Rc::from(r.as_str()))
            }
            "MID" => {
                need!(3, 3);
                let s = self.text(&a[0])?; let st = self.num(&a[1])?.trunc(); let k = self.num(&a[2])?.trunc();
                if st < 1.0 || k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect();
                let st = (st as usize - 1).min(ch.len()); let e = (st + k as usize).min(ch.len());
                V::S(Rc::from(ch[st..e].iter().collect::<String>().as_str()))
            }
            "LEN" => { need!(1, 1); V::N(self.text(&a[0])?.chars().count() as f64) }
            "UPPER" => { need!(1, 1); V::S(Rc::from(self.text(&a[0])?.to_uppercase().as_str())) }
            "LOWER" => { need!(1, 1); V::S(Rc::from(lower(&self.text(&a[0])?).as_str())) }
            "PROPER" => {
                need!(1, 1);
                let s = self.text(&a[0])?; let mut o = String::new(); let mut prev = false;
                for c in s.chars() {
                    if c.is_alphabetic() { if prev { o.extend(c.to_lowercase()); } else { o.extend(c.to_uppercase()); } prev = true; } else { o.push(c); prev = false; }
                }
                V::S(Rc::from(o.as_str()))
            }
            "TRIM" => {
                need!(1, 1);
                let s = self.text(&a[0])?;
                V::S(Rc::from(s.split(' ').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ").as_str()))
            }
            "EXACT" => { need!(2, 2); let x = self.text(&a[0])?; let y = self.text(&a[1])?; V::B(x == y) }
            "REPT" => {
                need!(2, 2);
                let s = self.text(&a[0])?; let k = self.num(&a[1])?.trunc();
                if k < 0.0 { return Err(Er::Value); }
                V::S(Rc::from(s.repeat(k as usize).as_str()))
            }
            "CONCATENATE" | "CONCAT" => {
                let mut o = String::new();
                for v in self.flat(a)? { o.push_str(&to_str(&v)?); }
                V::S(Rc::from(o.as_str()))
            }
            "TEXTJOIN" => {
                if n < 3 { return Err(Er::Value); }
                let d = self.text(&a[0])?; let ig = to_bool(&self.ev(&a[1]))?;
                let mut parts = Vec::new();
                for v in self.flat(&a[2..])? {
                    let s = to_str(&v)?;
                    if ig && s.is_empty() { continue; }
                    parts.push(s);
                }
                V::S(Rc::from(parts.iter().map(|s| &**s).collect::<Vec<_>>().join(&d).as_str()))
            }
            "FIND" | "SEARCH" => {
                need!(2, 3);
                let f = self.text(&a[0])?; let w = self.text(&a[1])?; let st = self.opt_num(a, 2, 1.0)?.trunc();
                let wc: Vec<char> = w.chars().collect();
                if st < 1.0 || st as usize > wc.len() + if f.is_empty() { 1 } else { 0 } { return Err(Er::Value); }
                let st = st as usize - 1;
                let (fc, wcc): (Vec<char>, Vec<char>) = if name == "FIND" { (f.chars().collect(), wc) } else { (lower(&f).chars().collect(), lower(&w).chars().collect()) };
                let wild = name == "SEARCH" && has_wild(&f);
                if fc.is_empty() { return Err(Er::Value); }
                for i in st..wcc.len() {
                    if wild {
                        // try all end positions
                        for e in i..=wcc.len() { if wild_match(&fc, &wcc[i..e]) { return Ok(V::N((i + 1) as f64)); } }
                    } else if i + fc.len() <= wcc.len() && wcc[i..i + fc.len()] == fc[..] { return Ok(V::N((i + 1) as f64)); }
                }
                return Err(Er::Value);
            }
            "REPLACE" => {
                need!(4, 4);
                let s = self.text(&a[0])?; let st = self.num(&a[1])?.trunc(); let k = self.num(&a[2])?.trunc(); let nw = self.text(&a[3])?;
                if st < 1.0 || k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect();
                let st = (st as usize - 1).min(ch.len()); let e = (st + k as usize).min(ch.len());
                let mut o: String = ch[..st].iter().collect(); o.push_str(&nw); o.extend(ch[e..].iter());
                V::S(Rc::from(o.as_str()))
            }
            "SUBSTITUTE" => {
                need!(3, 4);
                let s = self.text(&a[0])?; let old = self.text(&a[1])?; let nw = self.text(&a[2])?;
                if old.is_empty() { return Ok(V::S(s)); }
                if n == 4 {
                    let inst = self.num(&a[3])?.trunc();
                    if inst < 1.0 { return Err(Er::Value); }
                    let mut cnt = 0; let mut pos = 0;
                    while let Some(p) = s[pos..].find(&*old) {
                        cnt += 1;
                        if cnt == inst as usize {
                            let at = pos + p;
                            return Ok(V::S(Rc::from(format!("{}{}{}", &s[..at], nw, &s[at + old.len()..]).as_str())));
                        }
                        pos += p + old.len();
                    }
                    V::S(s)
                } else { V::S(Rc::from(s.replace(&*old, &nw).as_str())) }
            }
            "VALUE" => {
                need!(1, 1);
                match self.ev(&a[0]) { V::N(x) => V::N(x), V::B(b) => if matches!(a[0], Node::Ref(..)) { V::N(b as i32 as f64) } else { V::B(b) }, V::Empty => V::N(0.0), V::S(s) => V::N(str_to_num(&s).ok_or(Er::Value)?), V::B(_) => return Err(Er::Value), V::E(e) => return Err(e) }
            }
            "TEXT" => {
                need!(2, 2);
                let v = self.ev(&a[0]); let f = self.text(&a[1])?;
                if let V::N(x) = &v { if x.is_nan() { return Err(Er::Value); } if x.is_infinite() { return Err(Er::Num); } }
                let x = match &v { V::S(s) => match str_to_num(s) { Some(x) => x, None => return Ok(V::S(s.clone())) }, V::E(e) => return Err(*e), v => to_num(v)? };
                match text_format(x, &f) { Some(s) => V::S(Rc::from(s.as_str())), None => return Err(Er::Value) }
            }
            "DATE" => {
                need!(3, 3);
                let y = self.num(&a[0])?.trunc() as i64; let m = self.num(&a[1])?.trunc() as i64; let d = self.num(&a[2])?.trunc() as i64;
                let y = if y >= 0 && y < 30 { y + 2000 } else if y >= 30 && y < 100 { y + 1900 } else { y };
                if y < 0 || y > 9999 { return Err(Er::Value); }
                let tm = y * 12 + (m - 1);
                let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) as u32 + 1);
                let s = ymd_to_serial(yy, mm, 1) + d - 1;
                if s < -115858 { return Err(Er::Value); }
                V::N(s as f64)
            }
            "YEAR" | "MONTH" | "DAY" => {
                need!(1, 1);
                let x = self.num(&a[0])?.floor();
                if x > 2958465.0 || x < -693594.0 { return Err(Er::Value); }
                let (y, m, d) = serial_to_ymd(x as i64);
                V::N(match name { "YEAR" => y as f64, "MONTH" => m as f64, _ => d as f64 })
            }
            "WEEKDAY" => {
                need!(1, 2);
                let x = self.num(&a[0])?.floor(); let t = self.opt_num(a, 1, 1.0)? as i64;
                let w = (((x as i64) - 1) % 7 + 7) % 7;
                V::N(match t { 1 => w + 1, 2 => (w + 6) % 7 + 1, 3 => (w + 6) % 7, 11 => (w + 6) % 7 + 1, 12 => (w + 5) % 7 + 1, 13 => (w + 4) % 7 + 1, 14 => (w + 3) % 7 + 1, 15 => (w + 2) % 7 + 1, 16 => (w + 1) % 7 + 1, 17 => w + 1, _ => return Err(Er::Value) } as f64)
            }
            "DAYS" => { need!(2, 2); let e = self.num(&a[0])?; let s = self.num(&a[1])?; V::N(e - s) }
            "EDATE" | "EOMONTH" => {
                need!(2, 2);
                let s = self.num(&a[0])?.floor(); let m = self.num(&a[1])?.trunc() as i64;
                if s.abs() > 3.2e6 || m.abs() > 200000 { return Err(Er::Value); }
                let (y, mo, d) = serial_to_ymd(s as i64);
                let tm = y * 12 + (mo as i64 - 1) + m;
                let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) as u32 + 1);
                let dim = days_in_month(yy, mm);
                let dd = if name == "EOMONTH" { dim } else { d.min(dim) };
                let r = ymd_to_serial(yy, mm, dd);
                V::N(r as f64)
            }
            "DATEDIF" => {
                need!(3, 3);
                let s = self.num(&a[0])?.floor(); let e = self.num(&a[1])?.floor(); let u = self.text(&a[2])?.to_uppercase();
                if s > e { return Err(Er::Value); }
                let (y1, m1, d1) = serial_to_ymd(s as i64); let (y2, m2, d2) = serial_to_ymd(e as i64);
                let months = (y2 - y1) * 12 + m2 as i64 - m1 as i64 - if d2 < d1 { 1 } else { 0 };
                V::N(match u.as_str() {
                    "Y" => (months / 12) as f64,
                    "M" => months as f64,
                    "D" => e - s,
                    "MD" => { if d2 >= d1 { (d2 - d1) as f64 } else {
                        let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                        (days_in_month(py, pm) as i64 - d1 as i64 + d2 as i64) as f64 } }
                    "YM" => (months % 12) as f64,
                    "YD" => {
                        let mut yy = y2; if (m2, d2) < (m1, d1) { yy -= 1; }
                        let d1c = if m1 == 2 && d1 == 29 && days_in_month(yy.max(y1) + 0, 2) != 29 { 28 } else { d1 };
                        let start = ymd_to_serial(yy, m1, d1c.min(days_in_month(yy, m1)));
                        e - start as f64
                    }
                    _ => return Err(Er::Value),
                })
            }
            "VLOOKUP" | "HLOOKUP" => {
                need!(3, 4);
                let key = self.ev(&a[0]);
                if let V::E(e) = key { return Err(e); }
                let g = self.rg_of(&a[1])?;
                let ci = self.num(&a[2])?.trunc();
                let approx = if n == 4 { to_bool(&self.ev(&a[3]))? } else { true };
                let vert = name == "VLOOKUP";
                let (len, width) = if vert { (g.rows(), g.cols()) } else { (g.cols(), g.rows()) };
                if ci < 1.0 { return Err(Er::Value); }
                if ci as usize > width { return Err(Er::Ref); }
                let pos = if approx {
                    let vals: Vec<V> = (0..len).map(|i| if vert { self.get(g.sh, g.r1 + i as u32, g.c1) } else { self.get(g.sh, g.r1, g.c1 + i as u32) }).collect();
                    self.find_approx(&key, &vals)
                } else if vert { self.find_exact_rg(&key, g, true) } else {
                    let vals: Vec<V> = (0..len).map(|i| self.get(g.sh, g.r1, g.c1 + i as u32)).collect();
                    self.find_exact(&key, &vals, true)
                };
                let rv = match pos {
                    None => return Err(Er::Na),
                    Some(p) => if vert { self.get(g.sh, g.r1 + p as u32, g.c1 + ci as u32 - 1) } else { self.get(g.sh, g.r1 + ci as u32 - 1, g.c1 + p as u32) },
                };
                if let V::B(b) = rv { V::N(b as i32 as f64) } else { rv }
            }
            "MATCH" => {
                need!(2, 3);
                let key = self.ev(&a[0]);
                if let V::E(e) = key { return Err(e); }
                let g = self.rg_of(&a[1])?;
                if g.rows() > 1 && g.cols() > 1 { return Err(Er::Na); }
                let t = self.opt_num(a, 2, 1.0)?;
                                let vals: Vec<V> = if t == 0.0 && g.cols() == 1 { Vec::new() } else if g.cols() == 1 { (g.r1..=g.r2).map(|r| self.get(g.sh, r, g.c1)).collect() } else { (g.c1..=g.c2).map(|c| self.get(g.sh, g.r1, c)).collect() };
                let p = if t == 0.0 {
                    if g.cols() == 1 { self.find_exact_rg(&key, g, true) } else { self.find_exact(&key, &vals, true) }
                } else if t > 0.0 { self.find_approx(&key, &vals) } else {
                    let mut best = None;
                    for (i, c) in vals.iter().enumerate() {
                        if matches!((&key, c), (V::N(_) | V::B(_), V::N(_) | V::B(_)) | (V::S(_), V::S(_))) {
                            if cmp_vals(c, &key) != Ordering::Less { best = Some(i); } else { break; }
                        }
                    }
                    best
                };
                match p { Some(p) => V::N((p + 1) as f64), None => return Err(Er::Na) }
            }
            "INDEX" => {
                need!(2, 3);
                let g = self.rg_of(&a[0])?;
                let r = self.num(&a[1])?.trunc();
                let c = if n == 3 { self.num(&a[2])?.trunc() } else { 0.0 };
                if r < 0.0 || c < 0.0 { return Err(Er::Value); }
                let (r, c) = if n == 2 && g.rows() == 1 { (1.0, r) } else if n == 2 { (r, 1.0) } else { (r, c) };
                let (r, c) = (if r == 0.0 && g.rows() == 1 { 1.0 } else { r }, if c == 0.0 && g.cols() == 1 { 1.0 } else { c });
                let (r, c) = (if r == 0.0 { 1.0 } else { r }, if c == 0.0 { 1.0 } else { c });
                if r as usize > g.rows() || c as usize > g.cols() { return Err(Er::Ref); }
                self.get(g.sh, g.r1 + r as u32 - 1, g.c1 + c as u32 - 1)
            }
            "XLOOKUP" => {
                if n < 3 || n > 6 { return Err(Er::Value); }
                let key = self.ev(&a[0]);
                if let V::E(e) = key { return Err(e); }
                let lg = self.rg_of(&a[1])?; let rg = self.rg_of(&a[2])?;
                let mode = self.opt_num(a, 4, 0.0)? as i32; let sm = self.opt_num(a, 5, 1.0)? as i32;
                let vert = lg.cols() == 1;
                if !vert && lg.rows() != 1 { return Err(Er::Value); }
                let len = if vert { lg.rows() } else { lg.cols() };
                let fast = vert && sm == 1 && (mode == 0 || mode == 2);
                let fast_hit = if fast { self.find_exact_rg(&key, lg, mode == 2) } else { None };
                if fast && fast_hit.is_none() && mode == 0 {
                    return if n >= 4 && !is_missing(&a[3]) { Ok(self.ev(&a[3])) } else { Err(Er::Na) };
                }
                let vals: Vec<V> = if fast_hit.is_some() { Vec::new() } else { (0..len).map(|i| if vert { self.get(lg.sh, lg.r1 + i as u32, lg.c1) } else { self.get(lg.sh, lg.r1, lg.c1 + i as u32) }).collect() };
                let same = |c: &V| matches!((&key, c), (V::N(_) | V::B(_), V::N(_) | V::B(_)) | (V::S(_), V::S(_)));
                let order: Vec<usize> = if sm == -1 || sm == -2 { (0..len).rev().collect() } else { (0..len).collect() };
                let mut found: Option<usize> = fast_hit;
                if fast_hit.is_none() { for &i in &order { if self.lookup_eq(&key, &vals[i], mode == 2) { found = Some(i); break; } } }
                if found.is_none() && (mode == -1 || mode == 1) {
                    let mut best: Option<usize> = None;
                    for &i in &order {
                        if !same(&vals[i]) { continue; }
                        let o = cmp_vals(&vals[i], &key);
                        if (mode == -1 && o == Ordering::Less) || (mode == 1 && o == Ordering::Greater) {
                            if best.map_or(true, |b| { let ob = cmp_vals(&vals[i], &vals[b]); if mode == -1 { ob == Ordering::Greater } else { ob == Ordering::Less } }) { best = Some(i); }
                        }
                    }
                    found = best;
                }
                match found {
                    Some(i) => {
                        if vert { if i >= rg.rows() { return Err(Er::Ref); } self.get(rg.sh, rg.r1 + i as u32, rg.c1) }
                        else { if i >= rg.cols() { return Err(Er::Ref); } self.get(rg.sh, rg.r1, rg.c1 + i as u32) }
                    }
                    None => { if n >= 4 && !is_missing(&a[3]) { self.ev(&a[3]) } else { return Err(Er::Na); } }
                }
            }
            _ => return Err(Er::Name),
        })
    }
}

trait CloneNan { fn clone_nan(self) -> V; }
impl CloneNan for V { fn clone_nan(self) -> V { self } }

pub fn run(wb: Workbook) -> String {
    let t00 = std::time::Instant::now();
    let names: Vec<String> = wb.sheets.iter().map(|s| s.name.clone()).collect();
    let mut sheets = Vec::new();
    let mut forms = Vec::new();
    let mut fsheet = Vec::new();
    let mut pos_of = Vec::new();
    let mut fnames: Vec<(usize, u32, u32)> = Vec::new();
    // collect formulas
    let mut items: Vec<(usize, u32, u32, &str)> = Vec::new();
    for (si, s) in wb.sheets.iter().enumerate() {
        for (r, c, cell) in &s.cells { if let LCell::Formula(f) = cell { items.push((si, *r, *c, f.as_str())); } }
    }
    let nthreads = if items.len() > 2000 { 4 } else { 1 };
    let chunk = (items.len() + nthreads - 1) / nthreads.max(1);
    let parsed: Vec<Vec<Node>> = if items.is_empty() { Vec::new() } else {
        std::thread::scope(|sc| {
            let hs: Vec<_> = items.chunks(chunk.max(1)).map(|ch| { let names = &names; sc.spawn(move || ch.iter().map(|(si, _, _, f)| parse(f, *si, names)).collect::<Vec<Node>>()) }).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        })
    };
    for v in parsed { forms.extend(v); }
    let mut fi = 0usize;
    for (si, s) in wb.sheets.iter().enumerate() {
        let rows = s.cells.iter().map(|c| c.0 + 1).max().unwrap_or(0);
        let cols = s.cells.iter().map(|c| c.1 + 1).max().unwrap_or(0);
        let mut cells: Vec<GC> = Vec::with_capacity((rows * cols) as usize);
        for _ in 0..(rows * cols) { cells.push(GC::Empty); }
        for (r, c, cell) in &s.cells {
            let idx = (r * cols + c) as usize;
            match cell {
                LCell::Val(v) => cells[idx] = GC::V(v.clone()),
                LCell::Formula(_) => {
                    cells[idx] = GC::F(fi as u32);
                    fi += 1;
                    fsheet.push(si); pos_of.push((*r, *c)); fnames.push((si, *r, *c));
                }
            }
        }
        sheets.push(Sheet { name: s.name.clone(), rows, cols, cells });
    }
    let nf = forms.len();
    let tt = std::time::Instant::now();
    let dbg = std::env::var("CALC_TIME").is_ok();
    let eng = Engine {
        sheets, forms, fsheet, pos_of,
        results: RefCell::new(vec![V::Empty; nf]), state: RefCell::new(vec![0; nf]),
        cur: Cell::new((0, 0, 0)), gen: Cell::new(0), gerr: Cell::new(None),
        rect_cache: RefCell::new(HashMap::new()), idx_cache: RefCell::new(HashMap::new()), mask_cache: RefCell::new(HashMap::new()),
    };
    if dbg { eprintln!("build {:?}", t00.elapsed()); }
    for i in 0..nf {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { eng.fval(i); }));
        if r.is_err() {
            eng.gerr.set(None);
            eng.results.borrow_mut()[i] = V::E(Er::Value);
            eng.state.borrow_mut()[i] = 2;
        }
    }
    if dbg { eprintln!("eval {:?}", t00.elapsed()); }
    let res = eng.results.borrow();
    let mut out = String::with_capacity(nf * 40);
    let mut rb = ryu::Buffer::new();
    out.push('{');
    let mut first_sheet = true;
    for si in 0..eng.sheets.len() {
        if !first_sheet { out.push(','); }
        first_sheet = false;
        out.push_str(&format!("\"{}\":{{", eng.sheets[si].name.replace('\\', "\\\\").replace('"', "\\\"")));
        let mut first = true;
        for (i, (s, r, c)) in fnames.iter().enumerate() {
            if *s != si { continue; }
            if !first { out.push(','); }
            first = false;
            out.push('"'); out.push_str(&col_name(*c)); out.push_str(itoa_u(r + 1).as_str()); out.push_str("\":");
            match &res[i] {
                V::N(x) => { out.push_str("{\"t\":\"n\",\"v\":"); out.push_str(rb.format_finite(*x)); out.push('}'); }
                V::S(s) => {
                    out.push_str("{\"t\":\"s\",\"v\":\"");
                    for ch in s.chars() {
                        match ch { '"' => out.push_str("\\\""), '\\' => out.push_str("\\\\"), '\n' => out.push_str("\\n"), '\r' => out.push_str("\\r"), '\t' => out.push_str("\\t"),
                            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) }
                    }
                    out.push_str("\"}");
                }
                V::B(b) => out.push_str(&format!("{{\"t\":\"b\",\"v\":{}}}", b)),
                V::E(e) => out.push_str(&format!("{{\"t\":\"e\",\"v\":\"{}\"}}", e.name())),
                V::Empty => out.push_str("{\"t\":\"n\",\"v\":0}"),
            }
        }
        out.push('}');
    }
    out.push('}');
    if dbg { eprintln!("json {:?}", t00.elapsed()); }
    out
}
