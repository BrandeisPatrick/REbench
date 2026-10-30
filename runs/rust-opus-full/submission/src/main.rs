use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc as Rc;

mod fmt;
use fmt::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Er { Div0, Value, NA, Num, Ref, Name, Null }
impl Er {
    fn s(self) -> &'static str {
        match self { Er::Div0 => "#DIV/0!", Er::Value => "#VALUE!", Er::NA => "#N/A", Er::Num => "#NUM!", Er::Ref => "#REF!", Er::Name => "#NAME?", Er::Null => "#NULL!" }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rf { s: u16, r1: u32, c1: u32, r2: u32, c2: u32 }

#[derive(Clone, Debug)]
pub enum V { Empty, N(f64), S(Rc<str>), B(bool), E(Er), R(Rf), A(Rc<Arr>) }
#[derive(Debug)]
pub struct Arr { rows: usize, cols: usize, d: Vec<V> }

#[derive(Debug)]
enum Ex {
    Lit(V),
    Ref(Rf, bool),
    Un(Box<Ex>),
    Pct(Box<Ex>),
    Bin(u8, Box<Ex>, Box<Ex>),
    F(&'static str, Vec<Ex>),
}

#[derive(Clone)]
enum Cell { Empty, V(V), F(u32) }

struct Sheet { name: String, ncols: usize, nrows: usize, cells: Vec<Cell> }

struct Formula { s: u16, r: u32, c: u32, ex: Ex, state: u8, val: V, er: bool }

struct Eng { lcache: HashMap<(u16, u32, u32, u32, u32), Rc<HashMap<LK, u32>>>, mcache: HashMap<(u16, u32, u32, u32, u32, String), Rc<Vec<u64>>>, sheets: Vec<Sheet>, fs: Vec<Formula>, arrctx: u32, cur: (u16, u32, u32), lastlog: bool }

type R<T> = Result<T, Er>;

// ---------------- parsing ----------------
fn col_to_n(s: &str) -> u32 { let mut n = 0u32; for b in s.bytes() { n = n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32; } n - 1 }
fn parse_a1(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes(); let mut i = 0;
    if i < b.len() && b[i] == b'$' { i += 1; }
    let cs = i; while i < b.len() && b[i].is_ascii_alphabetic() { i += 1; }
    if i == cs || i - cs > 3 { return None; }
    let col = col_to_n(&s[cs..i]);
    if i < b.len() && b[i] == b'$' { i += 1; }
    let rs = i; while i < b.len() && b[i].is_ascii_digit() { i += 1; }
    if i == rs || i != b.len() { return None; }
    let row: u32 = s[rs..i].parse().ok()?;
    if row == 0 { return None; }
    Some((row - 1, col))
}

struct P<'a> { b: &'a [u8], i: usize, names: &'a HashMap<String, u16>, cs: u16 }
impl<'a> P<'a> {
    fn ws(&mut self) { while self.i < self.b.len() && self.b[self.i] == b' ' { self.i += 1; } }
    fn peek(&mut self) -> u8 { self.ws(); if self.i < self.b.len() { self.b[self.i] } else { 0 } }
    fn expr(&mut self) -> Ex { self.cmp() }
    fn cmp(&mut self) -> Ex {
        let mut l = self.cat();
        loop {
            let c = self.peek();
            let n = if self.i + 1 < self.b.len() { self.b[self.i + 1] } else { 0 };
            let op = match (c, n) {
                (b'<', b'=') => { self.i += 2; 5 } (b'>', b'=') => { self.i += 2; 6 } (b'<', b'>') => { self.i += 2; 7 }
                (b'<', _) => { self.i += 1; 8 } (b'>', _) => { self.i += 1; 9 } (b'=', _) => { self.i += 1; 10 }
                _ => return l,
            };
            let r = self.cat(); l = Ex::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn cat(&mut self) -> Ex {
        let mut l = self.add();
        while self.peek() == b'&' { self.i += 1; let r = self.add(); l = Ex::Bin(11, Box::new(l), Box::new(r)); }
        l
    }
    fn add(&mut self) -> Ex {
        let mut l = self.mul();
        loop { let c = self.peek(); let op = match c { b'+' => 0, b'-' => 1, _ => return l }; self.i += 1; let r = self.mul(); l = Ex::Bin(op, Box::new(l), Box::new(r)); }
    }
    fn mul(&mut self) -> Ex {
        let mut l = self.pow();
        loop { let c = self.peek(); let op = match c { b'*' => 2, b'/' => 3, _ => return l }; self.i += 1; let r = self.pow(); l = Ex::Bin(op, Box::new(l), Box::new(r)); }
    }
    fn pow(&mut self) -> Ex {
        let mut l = self.unary();
        while self.peek() == b'^' { self.i += 1; let r = self.unary(); l = Ex::Bin(4, Box::new(l), Box::new(r)); }
        l
    }
    fn unary(&mut self) -> Ex {
        let c = self.peek();
        if c == b'-' { self.i += 1; let e = self.unary(); return Ex::Un(Box::new(e)); }
        if c == b'+' { self.i += 1; return self.unary(); }
        let mut e = self.prim();
        while self.peek() == b'%' { self.i += 1; e = Ex::Pct(Box::new(e)); }
        e
    }
    fn prim(&mut self) -> Ex {
        let c = self.peek();
        if c == b'(' { self.i += 1; let e = self.expr(); if self.peek() == b')' { self.i += 1; } return e; }
        if c == b'"' {
            self.i += 1; let mut s = Vec::new();
            while self.i < self.b.len() { if self.b[self.i] == b'"' { if self.i + 1 < self.b.len() && self.b[self.i + 1] == b'"' { s.push(b'"'); self.i += 2; continue; } self.i += 1; break; } s.push(self.b[self.i]); self.i += 1; }
            return Ex::Lit(V::S(Rc::from(String::from_utf8_lossy(&s).as_ref())));
        }
        if c.is_ascii_digit() || c == b'.' {
            let st = self.i;
            while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'.') { self.i += 1; }
            if self.i < self.b.len() && (self.b[self.i] == b'e' || self.b[self.i] == b'E') {
                let save = self.i; self.i += 1;
                if self.i < self.b.len() && (self.b[self.i] == b'+' || self.b[self.i] == b'-') { self.i += 1; }
                if self.i < self.b.len() && self.b[self.i].is_ascii_digit() { while self.i < self.b.len() && self.b[self.i].is_ascii_digit() { self.i += 1; } } else { self.i = save; }
            }
            let s = std::str::from_utf8(&self.b[st..self.i]).unwrap();
            return Ex::Lit(V::N(s.parse().unwrap_or(0.0)));
        }
        if c == b'#' {
            let st = self.i;
            for (t, e) in [("#DIV/0!", Er::Div0), ("#VALUE!", Er::Value), ("#N/A", Er::NA), ("#NUM!", Er::Num), ("#REF!", Er::Ref), ("#NAME?", Er::Name), ("#NULL!", Er::Null)] {
                if self.b[st..].starts_with(t.as_bytes()) { self.i += t.len(); return Ex::Lit(V::E(e)); }
            }
            self.i += 1; return Ex::Lit(V::E(Er::Name));
        }
        // identifier / sheet-qualified ref
        let mut sheet = self.cs;
        if c == b'\'' {
            self.i += 1; let st = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'\'' { self.i += 1; }
            let nm = std::str::from_utf8(&self.b[st..self.i]).unwrap().to_string(); self.i += 1;
            if self.peek() == b'!' { self.i += 1; }
            sheet = *self.names.get(&nm.to_lowercase()).unwrap_or(&u16::MAX);
            return self.refpart(sheet);
        }
        let st = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_' || self.b[self.i] == b'.' || self.b[self.i] == b'$') { self.i += 1; }
        let id: &str = std::str::from_utf8(&self.b[st..self.i]).unwrap();
        if self.i < self.b.len() && self.b[self.i] == b'!' {
            self.i += 1;
            sheet = *self.names.get(&id.to_lowercase()).unwrap_or(&u16::MAX);
            return self.refpart(sheet);
        }
        if self.i < self.b.len() && self.b[self.i] == b'(' {
            self.i += 1;
            let mut nm: &str = id;
            if nm.len() > 6 && (nm[..6].eq_ignore_ascii_case("_xlfn.") || nm[..6].eq_ignore_ascii_case("_xlws.")) { nm = &nm[6..]; }
            let name: &'static str = FUNCS.iter().find(|f| f.eq_ignore_ascii_case(nm)).copied().unwrap_or("#UNKNOWN");
            let mut args = Vec::new();
            if self.peek() == b')' { self.i += 1; return Ex::F(name, args); }
            loop {
                let c = self.peek();
                if c == b',' || c == b')' { args.push(Ex::Lit(V::Empty)); } else { args.push(self.expr()); }
                let c = self.peek(); self.i += 1;
                if c != b',' { break; }
            }
            return Ex::F(name, args);
        }
        if id.eq_ignore_ascii_case("TRUE") { return Ex::Lit(V::B(true)); }
        if id.eq_ignore_ascii_case("FALSE") { return Ex::Lit(V::B(false)); }
        self.i = st;
        self.refpart(sheet)
    }
    fn refpart(&mut self, sheet: u16) -> Ex {
        let st = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'$') { self.i += 1; }
        let a = std::str::from_utf8(&self.b[st..self.i]).unwrap();
        let Some((r1, c1)) = parse_a1(a) else { return Ex::Lit(V::E(Er::Name)); };
        if self.i < self.b.len() && self.b[self.i] == b':' {
            self.i += 1; let st2 = self.i;
            while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'$') { self.i += 1; }
            let b = std::str::from_utf8(&self.b[st2..self.i]).unwrap();
            if let Some((r2, c2)) = parse_a1(b) {
                if sheet == u16::MAX { return Ex::Lit(V::E(Er::Ref)); }
                return Ex::Ref(Rf { s: sheet, r1: r1.min(r2), c1: c1.min(c2), r2: r1.max(r2), c2: c1.max(c2) }, true);
            }
        }
        if sheet == u16::MAX { return Ex::Lit(V::E(Er::Ref)); }
        Ex::Ref(Rf { s: sheet, r1, c1, r2: r1, c2: c1 }, false)
    }
}

// ---------------- coercion ----------------
pub fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() { return None; }
    let (t, pct) = if let Some(x) = t.strip_suffix('%') { (x.trim(), true) } else { (t, false) };
    let ok = t.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'+' || b == b'e' || b == b'E');
    if ok {
        if let Ok(v) = t.parse::<f64>() { if v.is_finite() { return Some(if pct { v / 100.0 } else { v }); } }
    }
    // thousands separators
    if !pct && t.contains(',') {
        let u: String = t.chars().filter(|&c| c != ',').collect();
        if u.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-') { if let Ok(v) = u.parse::<f64>() { return Some(v); } }
    }
    if !pct { if let Some(d) = parse_date_str(t) { return Some(d); } }
    None
}

fn mon_idx(m: &str) -> Option<i64> {
    let l = m.to_lowercase();
    ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"].iter().position(|x| l.starts_with(x) && (l.len() == 3 || ["january","february","march","april","may","june","july","august","september","october","november","december"].iter().any(|f| *f == l))).map(|i| i as i64 + 1)
}
fn parse_date_str(t: &str) -> Option<f64> {
    {
        let p: Vec<&str> = t.split(|c| c == '-' || c == ' ' || c == '/').filter(|x| !x.is_empty()).collect();
        if p.len() == 3 { if let Some(m) = mon_idx(p[1]) { if let (Ok(d), Ok(y)) = (p[0].parse::<i64>(), p[2].parse::<i64>()) {
            let y = if p[2].len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            if d >= 1 && d <= days_in_month(y, m) { return Some(ymd_to_serial(y, m, d) as f64); }
        } } }
    }
    // yyyy-mm-dd or mm/dd/yyyy
    let parts: Vec<&str> = if t.contains('-') { t.split('-').collect() } else if t.contains('/') { t.split('/').collect() } else { return None };
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) { return None; }
    let n: Vec<i64> = parts.iter().map(|p| p.parse().unwrap()).collect();
    let (y, m, d) = if t.contains('-') { if parts[0].len() == 4 { (n[0], n[1], n[2]) } else { return None } } else { (n[2], n[0], n[1]) };
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) { return None; }
    Some(ymd_to_serial(y, m, d) as f64)
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, _ => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 } }
}
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
pub fn ymd_to_serial(y: i64, m: i64, d: i64) -> i64 { days_from_civil(y, m, d) - days_from_civil(1899, 12, 30) }
pub fn serial_to_ymd(s: i64) -> (i64, i64, i64) {
    let z = s + days_from_civil(1899, 12, 30) + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn to_num(v: &V) -> R<f64> {
    match v { V::N(n) => Ok(*n), V::B(b) => Ok(if *b { 1.0 } else { 0.0 }), V::Empty => Ok(0.0), V::S(s) => parse_num(s).or_else(|| if s.eq_ignore_ascii_case("TRUE") { Some(1.0) } else if s.eq_ignore_ascii_case("FALSE") { Some(0.0) } else { None }).ok_or(Er::Value), V::E(e) => Err(*e), _ => Err(Er::Value) }
}
fn to_str(v: &V) -> R<Rc<str>> {
    match v { V::S(s) => Ok(s.clone()), V::N(n) => Ok(Rc::from(num_to_str(*n).as_str())), V::B(b) => Ok(Rc::from(if *b { "1" } else { "0" })), V::Empty => Ok(Rc::from("")), V::E(e) => Err(*e), _ => Err(Er::Value) }
}
fn to_bool(v: &V) -> R<bool> {
    match v { V::N(n) => Ok(*n != 0.0), V::B(b) => Ok(*b), V::Empty => Ok(false), V::S(s) => { if s.eq_ignore_ascii_case("TRUE") { Ok(true) } else if s.eq_ignore_ascii_case("FALSE") { Ok(false) } else if let Some(x) = parse_num(s) { Ok(x != 0.0) } else { Err(Er::Value) } }, V::E(e) => Err(*e), _ => Err(Er::Value) }
}

fn cmp_vals(a: &V, b: &V) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    fn rank(v: &V) -> u8 { match v { V::N(_) | V::Empty | V::B(_) => 0, V::S(_) => 1, _ => 3 } }
    let (a2, b2) = match (a, b) {
        (V::Empty, V::S(_)) => (V::S(Rc::from("")), b.clone()),
        (V::S(_), V::Empty) => (a.clone(), V::S(Rc::from(""))),
        (V::Empty, V::B(_)) => (V::B(false), b.clone()),
        (V::B(_), V::Empty) => (a.clone(), V::B(false)),
        _ => (a.clone(), b.clone()),
    };
    let (ra, rb) = (rank(&a2), rank(&b2));
    if ra != rb { return ra.cmp(&rb); }
    match (&a2, &b2) {
        (V::S(x), V::S(y)) => cmp_str_ci(x, y),
        (V::B(x), V::B(y)) => x.cmp(y),
        _ => { let x = to_num(&a2).unwrap_or(0.0); let y = to_num(&b2).unwrap_or(0.0); if x < y { Less } else if x > y { Greater } else { Equal } }
    }
}
fn cmp_str_ci(x: &str, y: &str) -> std::cmp::Ordering { x.to_lowercase().cmp(&y.to_lowercase()) }

// ---------------- criteria ----------------
enum CK { Num(f64), Str(String, bool), Empty, Bool(bool), NumS(f64, String) }
struct Crit { op: u8, k: CK } // op: 0 = ,1 <>,2 <,3 <=,4 >,5 >=

fn make_crit(v: &V) -> Crit {
    match v {
        V::N(n) => Crit { op: 0, k: CK::Num(*n) },
        V::B(b) => Crit { op: 0, k: CK::Bool(*b) },
        V::Empty => Crit { op: 0, k: CK::Num(0.0) },
        V::E(_) => Crit { op: 0, k: CK::Empty },
        V::S(s) => {
            let s: &str = s;
            let (op, rest) = if let Some(r) = s.strip_prefix("<=") { (3, r) } else if let Some(r) = s.strip_prefix(">=") { (5, r) } else if let Some(r) = s.strip_prefix("<>") { (1, r) } else if let Some(r) = s.strip_prefix('<') { (2, r) } else if let Some(r) = s.strip_prefix('>') { (4, r) } else if let Some(r) = s.strip_prefix('=') { (0, r) } else { (0, s) };
            if rest.is_empty() { if op >= 2 { return Crit { op, k: CK::Str(String::new(), false) }; } return Crit { op, k: CK::Empty }; }
            if let Some(n) = parse_num(rest) { return Crit { op, k: CK::NumS(n, rest.to_lowercase()) }; }
            if rest.eq_ignore_ascii_case("TRUE") { return Crit { op, k: CK::Bool(true) }; }
            if rest.eq_ignore_ascii_case("FALSE") { return Crit { op, k: CK::Bool(false) }; }
            let wild = rest.contains('*') || rest.contains('?') || rest.contains('~');
            Crit { op, k: CK::Str(rest.to_lowercase(), wild) }
        }
        _ => Crit { op: 0, k: CK::Empty },
    }
}

pub fn wild_match(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0); let (mut star, mut ss) = (usize::MAX, 0);
    while si < s.len() {
        if pi < p.len() {
            let c = p[pi];
            if c == '*' { star = pi; pi += 1; ss = si; continue; }
            if c == '~' && pi + 1 < p.len() && (p[pi + 1] == '*' || p[pi + 1] == '?' || p[pi + 1] == '~') { if p[pi + 1] == s[si] { pi += 2; si += 1; continue; } }
            else if c == '?' || c == s[si] { pi += 1; si += 1; continue; }
        }
        if star != usize::MAX { pi = star + 1; ss += 1; si = ss; continue; }
        return false;
    }
    while pi < p.len() && p[pi] == '*' { pi += 1; }
    pi == p.len()
}

fn ord_ok(op: u8, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op { 0 => o == Equal, 1 => o != Equal, 2 => o == Less, 3 => o != Greater, 4 => o == Greater, _ => o != Less }
}

fn crit_match(c: &Crit, v: &V) -> bool {
    if let (CK::Str(p, true), V::B(b), 0 | 1) = (&c.k, v, c.op) {
        let pc: Vec<char> = p.chars().collect(); let sc: Vec<char> = (if *b { "true" } else { "false" }).chars().collect();
        let eq = wild_match(&pc, &sc); return if c.op == 0 { eq } else { !eq };
    }
    let tmp; let v = if let V::B(b) = v { tmp = V::N(*b as u8 as f64); &tmp } else { v };
    let tmpk; let ck = if let CK::Bool(b) = &c.k { tmpk = CK::Num(*b as u8 as f64); &tmpk } else { &c.k };
    match ck {
        CK::Empty => {
            let empty = matches!(v, V::Empty) || matches!(v, V::S(s) if s.is_empty());
            match c.op { 0 => empty, 1 => !empty, _ => false }
        }
        CK::NumS(n, st) => {
            let x = match v { V::N(x) => *x, V::S(s) if c.op <= 1 => { let eq = s.to_lowercase() == *st; return if c.op == 0 { eq } else { !eq }; } _ => return c.op == 1 };
            ord_ok(c.op, x.partial_cmp(n).unwrap())
        }
        CK::Num(n) => {
            let x = match v { V::N(x) => *x, _ => return c.op == 1 };
            ord_ok(c.op, x.partial_cmp(n).unwrap())
        }
        CK::Bool(b) => {
            let x = match v { V::B(x) => *x, _ => return c.op == 1 };
            ord_ok(c.op, x.cmp(b))
        }
        CK::Str(p, wild) => {
            if c.op <= 1 && !*wild { if let V::S(s) = v { let eq = if s.is_ascii() { s.eq_ignore_ascii_case(p) } else { s.to_lowercase() == *p }; return if c.op == 0 { eq } else { !eq }; } }
            let s = match v { V::S(s) => s.to_lowercase(), V::Empty if *wild && c.op <= 1 => return c.op == 1, V::Empty => String::new(), V::N(x) if *wild && c.op <= 1 => num_to_str(*x), _ => return c.op == 1 };
            if c.op <= 1 {
                let eq = if *wild { let pc: Vec<char> = p.chars().collect(); let sc: Vec<char> = s.chars().collect(); wild_match(&pc, &sc) } else { &s == p };
                if c.op == 0 { eq } else { !eq }
            } else {
                if matches!(v, V::Empty) { return false; }
                ord_ok(c.op, s.as_str().cmp(p.as_str()))
            }
        }
    }
}

// ---------------- engine ----------------
impl Eng {
    fn cell(&mut self, s: u16, r: u32, c: u32) -> V {
        let sh = &self.sheets[s as usize];
        if r as usize >= sh.nrows || c as usize >= sh.ncols { return V::Empty; }
        match &sh.cells[r as usize * sh.ncols + c as usize] {
            Cell::Empty => V::Empty,
            Cell::V(v) => v.clone(),
            Cell::F(i) => { let i = *i as usize; self.calc(i) }
        }
    }
    fn calc(&mut self, i: usize) -> V {
        match self.fs[i].state {
            2 => return self.fs[i].val.clone(),
            1 => return V::E(Er::Value), // cycle (Err:522) 
            _ => {}
        }
        self.fs[i].state = 1;
        let ex = std::mem::replace(&mut self.fs[i].ex, Ex::Lit(V::Empty));
        let save = self.cur; let sa = self.arrctx; let sl = self.lastlog;
        self.cur = (self.fs[i].s, self.fs[i].r, self.fs[i].c); self.arrctx = 0; self.lastlog = false;
        let v = self.ev(&ex);
        let v = self.top(v);
        let v = if let V::Empty = v { self.fs[i].er = true; V::N(0.0) } else { v };
        self.cur = save; self.arrctx = sa; self.lastlog = sl;
        self.fs[i].ex = ex;
        self.fs[i].state = 2; self.fs[i].val = v.clone();
        v
    }
    fn top(&mut self, v: V) -> V {
        match v {
            V::R(_) => self.scalar(v),
            V::A(a) => a.d.first().cloned().unwrap_or(V::Empty),
            V::N(n) if !n.is_finite() => V::E(Er::Num),
            x => x,
        }
    }
    // reference/array -> scalar via implicit intersection
    fn scalar(&mut self, v: V) -> V {
        match v {
            V::R(r) => {
                if r.r1 == r.r2 && r.c1 == r.c2 { return self.cell(r.s, r.r1, r.c1); }
                let (_, cr, cc) = self.cur;
                if r.c1 == r.c2 && cr >= r.r1 && cr <= r.r2 { return self.cell(r.s, cr, r.c1); }
                if r.r1 == r.r2 && cc >= r.c1 && cc <= r.c2 { return self.cell(r.s, r.r1, cc); }
                V::E(Er::Value)
            }
            V::A(a) => a.d.first().cloned().unwrap_or(V::Empty),
            x => x,
        }
    }
    fn evs(&mut self, e: &Ex) -> V { let v = self.ev(e); self.scalar(v) }
    fn num(&mut self, e: &Ex) -> R<f64> { let v = self.evs(e); to_num(&v) }
    fn st(&mut self, e: &Ex) -> R<Rc<str>> { let v = self.evs(e); to_str(&v) }
    fn bl(&mut self, e: &Ex) -> R<bool> { let v = self.evs(e); to_bool(&v) }

    fn to_arr(&mut self, v: V) -> Rc<Arr> {
        match v {
            V::A(a) => a,
            V::R(r) => {
                let mut d = Vec::with_capacity(((r.r2 - r.r1 + 1) * (r.c2 - r.c1 + 1)) as usize);
                for rr in r.r1..=r.r2 { for cc in r.c1..=r.c2 { d.push(self.cell(r.s, rr, cc)); } }
                Rc::new(Arr { rows: (r.r2 - r.r1 + 1) as usize, cols: (r.c2 - r.c1 + 1) as usize, d })
            }
            x => Rc::new(Arr { rows: 1, cols: 1, d: vec![x] }),
        }
    }

    fn ev(&mut self, e: &Ex) -> V {
        match e {
            Ex::Lit(v) => v.clone(),
            Ex::Ref(r, _) => V::R(*r),
            Ex::Un(x) => {
                let v = self.ev(x);
                self.map1(v, |v| to_num(v).map(|n| V::N(-n)).unwrap_or_else(V::E))
            }
            Ex::Pct(x) => { let v = self.ev(x); self.map1(v, |v| to_num(v).map(|n| V::N(n / 100.0)).unwrap_or_else(V::E)) }
            Ex::Bin(op, a, b) => {
                let mut va = self.ev(a); let mut vb = self.ev(b);
                let multi = |v: &V| match v { V::R(r) => r.r1 != r.r2 || r.c1 != r.c2, V::A(_) => true, _ => false };
                if self.arrctx > 0 && (multi(&va) || multi(&vb)) {
                    let aa = self.to_arr(va); let ab = self.to_arr(vb);
                    let rows = if aa.rows == 1 { ab.rows } else if ab.rows == 1 { aa.rows } else { aa.rows.min(ab.rows) };
                    let cols = if aa.cols == 1 { ab.cols } else if ab.cols == 1 { aa.cols } else { aa.cols.min(ab.cols) };
                    let mut d = Vec::with_capacity(rows * cols);
                    for r in 0..rows { for c in 0..cols {
                        let x = &aa.d[(if aa.rows == 1 { 0 } else { r }) * aa.cols + if aa.cols == 1 { 0 } else { c }];
                        let y = &ab.d[(if ab.rows == 1 { 0 } else { r }) * ab.cols + if ab.cols == 1 { 0 } else { c }];
                        d.push(binop(*op, x, y));
                    } }
                    return V::A(Rc::new(Arr { rows, cols, d }));
                }
                let aref = matches!(va, V::R(_));
                va = self.scalar(va); vb = self.scalar(vb);
                if !aref { if let V::E(e) = va { return V::E(e); } }
                if let V::E(e) = vb { return V::E(e); }
                if *op <= 4 { if let Err(e) = to_num(&vb) { return V::E(e); } }
                let r = binop(*op, &va, &vb); self.lastlog = matches!(r, V::B(_)); r
            }
            Ex::F(name, args) => self.func(name, args),
        }
    }
    fn map1(&mut self, v: V, f: impl Fn(&V) -> V) -> V {
        match v {
            V::R(r) if self.arrctx > 0 && (r.r1 != r.r2 || r.c1 != r.c2) => { let a = self.to_arr(V::R(r)); V::A(Rc::new(Arr { rows: a.rows, cols: a.cols, d: a.d.iter().map(&f).collect() })) }
            V::A(a) if self.arrctx > 0 => V::A(Rc::new(Arr { rows: a.rows, cols: a.cols, d: a.d.iter().map(&f).collect() })),
            v => { let s = self.scalar(v); f(&s) }
        }
    }

    // collect numbers for aggregate functions
    fn nums(&mut self, args: &[Ex], out: &mut Vec<f64>) -> R<()> {
        let mut referr: Option<Er> = None; let mut firsterr: Option<Er> = None;
        'arg: for a in args {
            let v = self.ev(a);
            match v {
                V::R(r) => {
                    for rr in r.r1..=r.r2 { for cc in r.c1..=r.c2 {
                        match self.cell(r.s, rr, cc) { V::N(n) => out.push(n), V::B(b) => out.push(b as u8 as f64), V::E(e) => { referr.get_or_insert(e); continue 'arg; } _ => {} }
                    } }
                }
                V::A(ar) => { for x in ar.d.iter() { match x { V::N(n) => out.push(*n), V::B(b) => out.push(*b as u8 as f64), V::E(e) => return Err(*e), _ => {} } } }
                V::Empty => {}
                x => match to_num(&x) { Ok(v) => out.push(v), Err(e) => { if matches!(a, Ex::F("SWITCH" | "IFS", _)) { return Err(e); } firsterr.get_or_insert(e); } },
            }
        }
        if let Some(e) = firsterr { return Err(e); }
        if let Some(e) = referr { return Err(e); }
        Ok(())
    }
    fn agg(&mut self, args: &[Ex], mut f: impl FnMut(f64)) -> R<()> {
        let mut referr: Option<Er> = None; let mut firsterr: Option<Er> = None;
        'arg: for a in args {
            let v = self.ev(a);
            match v {
                V::R(r) => {
                    for rr in r.r1..=r.r2 { for cc in r.c1..=r.c2 {
                        match self.cell(r.s, rr, cc) { V::N(n) => f(n), V::B(b) => f(b as u8 as f64), V::E(e) => { referr.get_or_insert(e); continue 'arg; } _ => {} }
                    } }
                }
                V::A(ar) => { for x in ar.d.iter() { match x { V::N(n) => f(*n), V::B(b) => f(*b as u8 as f64), V::E(e) => return Err(*e), _ => {} } } }
                V::Empty => {}
                x => match to_num(&x) { Ok(v) => f(v), Err(e) => { if matches!(a, Ex::F("SWITCH" | "IFS", _)) { return Err(e); } firsterr.get_or_insert(e); } },
            }
        }
        if let Some(e) = firsterr { return Err(e); }
        if let Some(e) = referr { return Err(e); }
        Ok(())
    }
    fn boolish(&mut self, a: &[Ex], r: f64) -> V {
        if let Some(Ex::Ref(rf, _)) = a.first() { if (r == 0.0 || r == 1.0) && matches!(self.cell(rf.s, rf.r1, rf.c1), V::B(_)) { return V::B(r == 1.0); } }
        V::N(r)
    }
    fn range_of(&mut self, e: &Ex) -> R<Rf> {
        match self.ev(e) { V::R(r) => Ok(r), V::E(e) => Err(e), _ => Err(Er::Value) }
    }

    fn ifs_core(&mut self, pairs: &[(Rf, Crit, String)], mut f: impl FnMut(&mut Eng, u32, u32)) -> R<()> {
        let (h, w) = (pairs[0].0.r2 - pairs[0].0.r1, pairs[0].0.c2 - pairs[0].0.c1);
        for (r, _, _) in pairs { if r.r2 - r.r1 != h || r.c2 - r.c1 != w { return Err(Er::Value); } }
        let mut masks: Vec<Rc<Vec<u64>>> = Vec::with_capacity(pairs.len());
        for (r, c, k) in pairs {
            let key = (r.s, r.r1, r.c1, r.r2, r.c2, k.clone());
            if let Some(m) = self.mcache.get(&key) { masks.push(m.clone()); continue; }
            let mut m = vec![0u64; (((h + 1) * (w + 1)) as usize + 63) / 64]; let mut idx = 0usize;
            for dr in 0..=h { for dc in 0..=w {
                let (rr, cc) = ((r.r1 + dr) as usize, (r.c1 + dc) as usize);
                let sh = &self.sheets[r.s as usize];
                let ok = if rr >= sh.nrows || cc >= sh.ncols { crit_match(c, &V::Empty) } else {
                    match &sh.cells[rr * sh.ncols + cc] { Cell::Empty => crit_match(c, &V::Empty), Cell::V(v) => crit_match(c, v), Cell::F(_) => { let v = self.cell(r.s, rr as u32, cc as u32); crit_match(c, &v) } }
                };
                if ok { m[idx >> 6] |= 1u64 << (idx & 63); } idx += 1;
            } }
            let m = Rc::new(m); self.mcache.insert(key, m.clone()); masks.push(m);
        }
        let ww = (w + 1) as usize;
        let n = ((h + 1) * (w + 1)) as usize;
        let nw = (n + 63) / 64;
        for wi in 0..nw {
            let mut bits = masks[0][wi];
            for m in &masks[1..] { bits &= m[wi]; }
            while bits != 0 { let t = bits.trailing_zeros() as usize; bits &= bits - 1; let i = wi * 64 + t; f(self, (i / ww) as u32, (i % ww) as u32); }
        }
        Ok(())
    }

    fn func(&mut self, name: &str, a: &[Ex]) -> V {
        let r = match self.func_r(name, a) { Ok(v) => v, Err(e) => V::E(e) };
        self.lastlog = matches!(r, V::B(_)); r
    }

    fn func_r(&mut self, name: &str, a: &[Ex]) -> R<V> {
        let n = a.len();
        macro_rules! num { ($i:expr) => { self.num(&a[$i])? } }
        macro_rules! st { ($i:expr) => { self.st(&a[$i])? } }
        Ok(match name {
            "IF" => {
                let c = self.bl(&a[0])?;
                if c { if n > 1 { if let Ex::Lit(V::Empty) = a[1] { V::N(0.0) } else { self.ev(&a[1]) } } else { V::B(true) } }
                else if n > 2 { if let Ex::Lit(V::Empty) = a[2] { V::N(0.0) } else { self.ev(&a[2]) } } else { V::B(false) }
            }
            "IFERROR" => { let v = self.evs(&a[0]); if let V::E(_) = v { self.ev(&a[1]) } else { v } }
            "IFNA" => { let v = self.evs(&a[0]); if let V::E(Er::NA) = v { self.ev(&a[1]) } else { v } }
            "IFS" => {
                let mut i = 0;
                while i + 1 < n { if self.bl(&a[i])? { return Ok(self.ev(&a[i + 1])); } i += 2; }
                return Err(Er::NA);
            }
            "SWITCH" => {
                let v = self.evs(&a[0]); if let V::E(e) = v { return Err(e); }
                let mut i = 1;
                while i + 1 < n { let w = self.evs(&a[i]); if let V::E(e) = w { return Err(e); } if eq_vals(&v, &w) { return Ok(self.ev(&a[i + 1])); } i += 2; }
                if i < n { self.ev(&a[i]) } else { return Err(Er::NA); }
            }
            "CHOOSE" => { let i = num!(0).trunc() as i64; if i < 1 || i as usize >= n { return Err(Er::Value); } self.ev(&a[i as usize]) }
            "AND" | "OR" | "XOR" => {
                let mut res: Option<bool> = None;
                for x in a {
                    let v = self.ev(x);
                    let mut push = |b: bool, res: &mut Option<bool>| { *res = Some(match (name, *res) { (_, None) => b, ("AND", Some(p)) => p && b, ("OR", Some(p)) => p || b, (_, Some(p)) => p ^ b }); };
                    match v {
                        V::R(_) | V::A(_) => { let ar = self.to_arr(v); for y in ar.d.iter() { match y { V::N(n) => push(*n != 0.0, &mut res), V::B(b) => push(*b, &mut res), V::E(e) => return Err(*e), _ => {} } } }
                        V::Empty => {}
                        V::S(s) => { push(to_bool(&V::S(s))?, &mut res) }
                        x => push(to_bool(&x)?, &mut res),
                    }
                }
                V::B(res.ok_or(Er::Value)?)
            }
            "NOT" => V::B(!self.bl(&a[0])?),
            "TRUE" => V::B(true), "FALSE" => V::B(false), "NA" => return Err(Er::NA),
            "SUM" => { let mut k = Kahan::default(); self.agg(a, |x| k.add(x))?; V::N(k.get()) }
            "PRODUCT" => { let mut s = 1.0; let mut any = false; self.agg(a, |x| { s *= x; any = true })?; if !s.is_finite() { return Err(Er::Num); } V::N(if any { s } else { 0.0 }) }
            "SUMSQ" => { let mut s = 0.0; self.agg(a, |x| s += x * x)?; V::N(s) }
            "COUNT" => {
                let mut c = 0.0;
                for x in a { let v = self.ev(x); match v { V::R(_) | V::A(_) => { let ar = self.to_arr(v); for y in ar.d.iter() { if let V::N(_) | V::B(_) = y { c += 1.0; } } } V::N(_) | V::B(_) => c += 1.0, V::S(s) => if parse_num(&s).is_some() { c += 1.0 }, _ => {} } }
                V::N(c)
            }
            "COUNTA" => {
                let mut c = 0.0;
                for x in a { let v = self.ev(x); match v { V::R(_) | V::A(_) => { let ar = self.to_arr(v); for y in ar.d.iter() { if !matches!(y, V::Empty) { c += 1.0; } } } V::Empty => {} _ => c += 1.0 } }
                V::N(c)
            }
            "COUNTBLANK" => {
                let v = self.ev(&a[0]); let ar = self.to_arr(v);
                V::N(ar.d.iter().filter(|y| matches!(y, V::Empty) || matches!(y, V::S(s) if s.is_empty())).count() as f64)
            }
            "AVERAGE" => { let mut k = Kahan::default(); let mut c = 0.0; self.agg(a, |x| { k.add(x); c += 1.0 })?; if c == 0.0 { return Err(Er::Div0); } V::N(k.get() / c) }
            "MAX" => { let mut m = f64::NEG_INFINITY; self.agg(a, |x| m = m.max(x))?; let r = if m == f64::NEG_INFINITY { 0.0 } else { m }; self.boolish(a, r) }
            "MIN" => { let mut m = f64::INFINITY; self.agg(a, |x| m = m.min(x))?; let r = if m == f64::INFINITY { 0.0 } else { m }; self.boolish(a, r) }
            "MEDIAN" => { let mut v = Vec::new(); self.nums(a, &mut v)?; if v.is_empty() { return Err(Er::Num); } v.sort_by(|x, y| x.partial_cmp(y).unwrap()); let k = v.len(); V::N(if k % 2 == 1 { v[k / 2] } else { (v[k / 2 - 1] + v[k / 2]) / 2.0 }) }
            "STDEV" | "VAR" => {
                let mut v = Vec::new(); self.nums(a, &mut v)?; if v.len() < 2 { return Err(Er::Div0); }
                let m = v.iter().sum::<f64>() / v.len() as f64; let s = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (v.len() - 1) as f64;
                V::N(if name == "VAR" { s } else { s.sqrt() })
            }
            "LARGE" | "SMALL" => {
                let mut v = Vec::new(); self.nums(&a[0..1], &mut v)?; let kf = num!(1);
                let k = if name == "SMALL" { kf.trunc() as i64 } else { (v.len() as f64 + 1.0 - kf).trunc() as i64 };
                if k < 1 || k as usize > v.len() { return Err(Er::Value); }
                v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                V::N(v[k as usize - 1])
            }
            "SUMPRODUCT" => {
                self.arrctx += 1;
                let mut arrs = Vec::new();
                for x in a { let v = self.ev(x); arrs.push(self.to_arr(v)); }
                self.arrctx -= 1;
                let (r0, c0) = (arrs[0].rows, arrs[0].cols);
                for ar in &arrs { if ar.rows != r0 || ar.cols != c0 { return Err(Er::Value); } }
                let mut s = Kahan::default();
                for i in 0..r0 * c0 {
                    let mut p = 1.0;
                    for ar in &arrs { match &ar.d[i] { V::N(x) => p *= x, V::B(b) => p *= *b as u8 as f64, V::E(e) => return Err(*e), _ => p = 0.0 } }
                    s.add(p);
                }
                V::N(s.get())
            }
            "SUMIF" | "AVERAGEIF" | "COUNTIF" => {
                let r = self.range_of(&a[0])?; let cv = self.evs(&a[1]); if let V::E(e) = cv { return Err(e); } let c = make_crit(&cv);
                let sr = if n > 2 { let s = self.range_of(&a[2])?; Rf { s: s.s, r1: s.r1, c1: s.c1, r2: s.r1 + (r.r2 - r.r1), c2: s.c1 + (r.c2 - r.c1) } } else { r };
                let (mut s, mut cnt) = (Kahan::default(), 0.0); let mut err = None;
                let k = crit_key(&cv); self.ifs_core(&[(r, c, k)], |e, dr, dc| { cnt += 1.0; if name != "COUNTIF" { match e.cell(sr.s, sr.r1 + dr, sr.c1 + dc) { V::N(x) => s.add(x), V::B(b) => s.add(b as u8 as f64), V::E(er) => { err.get_or_insert(er); } _ => { if name == "AVERAGEIF" { cnt -= 1.0; } } } } })?;
                if let Some(e) = err { return Err(e); }
                match name { "COUNTIF" => V::N(cnt), "SUMIF" => V::N(s.get()), _ => { if cnt == 0.0 { return Err(Er::Div0); } V::N(s.get() / cnt) } }
            }
            "SUMIFS" | "AVERAGEIFS" | "COUNTIFS" => {
                let st = if name == "COUNTIFS" { 0 } else { 1 };
                let mut pairs = Vec::new();
                let mut i = st; while i + 1 < n { let r = self.range_of(&a[i])?; let cv = self.evs(&a[i + 1]); if let V::E(e) = cv { return Err(e); } pairs.push((r, make_crit(&cv), crit_key(&cv))); i += 2; }
                let sr = if st == 1 { Some(self.range_of(&a[0])?) } else { None };
                if let Some(sr) = sr { if sr.r2 - sr.r1 != pairs[0].0.r2 - pairs[0].0.r1 || sr.c2 - sr.c1 != pairs[0].0.c2 - pairs[0].0.c1 { return Err(Er::Value); } }
                let (mut s, mut cnt) = (Kahan::default(), 0.0); let mut err = None;
                self.ifs_core(&pairs, |e, dr, dc| { if let Some(sr) = sr { match e.cell(sr.s, sr.r1 + dr, sr.c1 + dc) { V::N(x) => { s.add(x); cnt += 1.0 } V::B(b) => { s.add(b as u8 as f64); cnt += 1.0 } V::E(er) => { err.get_or_insert(er); } _ => {} } } else { cnt += 1.0 } })?;
                if let Some(e) = err { return Err(e); }
                match name { "COUNTIFS" => V::N(cnt), "SUMIFS" => V::N(s.get()), _ => { if cnt == 0.0 { return Err(Er::Div0); } V::N(s.get() / cnt) } }
            }
            "ABS" => V::N(num!(0).abs()),
            "INT" => V::N(num!(0).floor()),
            "TRUNC" => { let x = num!(0); let d = if n > 1 { num!(1).trunc() } else { 0.0 }; V::N(round_mode(x, d, 0)) }
            "ROUND" => { let x = num!(0); let d = if n > 1 { num!(1).trunc() } else { 0.0 }; V::N(round_mode(x, d, 1)) }
            "ROUNDUP" => { let x = num!(0); let d = if n > 1 { num!(1).trunc() } else { 0.0 }; V::N(round_mode(x, d, 2)) }
            "ROUNDDOWN" => { let x = num!(0); let d = if n > 1 { num!(1).trunc() } else { 0.0 }; V::N(round_mode(x, d, 0)) }
            "MOD" => { let y = num!(1); if y == 0.0 { return Err(Er::Div0); } let x = num!(0); if (x / y).abs() >= 9007199254740992.0 { return Err(Er::Value); } let r = x - y * (x / y).floor(); V::N(if r != 0.0 && (r.abs() < 1e-15 * x.abs().max(1.0)) { 0.0 } else { r }) }
            "SIGN" => { let x = num!(0); V::N(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }) }
            "SQRT" => { let x = num!(0); if x < 0.0 { return Err(Er::Value); } V::N(x.sqrt()) }
            "EXP" => V::N(num!(0).exp()),
            "LN" => { let x = num!(0); if x <= 0.0 { return Err(Er::Value); } V::N(x.ln()) }
            "LOG10" => { let x = num!(0); if x <= 0.0 { return Err(Er::Value); } V::N(x.log10()) }
            "LOG" => { let x = num!(0); let b = if n > 1 { num!(1) } else { 10.0 }; if x <= 0.0 || b <= 0.0 || b == 1.0 { return Err(Er::Value); } V::N(if b == 10.0 { x.log10() } else { x.ln() / b.ln() }) }
            "POWER" => { let x = num!(0); let y = num!(1); return pow(x, y); }
            "PI" => V::N(std::f64::consts::PI),
            "N" => { let v = self.evs(&a[0]); match v { V::N(x) => V::N(x), V::B(b) => V::N(if b { 1.0 } else { 0.0 }), V::E(e) => return Err(e), _ => V::N(0.0) } }
            "T" => { let v = self.evs(&a[0]); match v { V::S(s) => V::S(s), V::E(e) => return Err(e), _ => V::S(Rc::from("")) } }
            "VALUE" => { let v = self.evs(&a[0]); match v { V::N(x) => V::N(x), V::E(e) => return Err(e), V::Empty => V::N(0.0), V::B(b) => V::N(b as u8 as f64), V::S(s) => V::N(to_num(&V::S(s))?), _ => return Err(Er::Value) } }
            "ISBLANK" => { let v = self.ev(&a[0]);
                if let V::R(r) = &v { let sh = &self.sheets[r.s as usize]; if (r.r1 as usize) < sh.nrows && (r.c1 as usize) < sh.ncols { if let Cell::F(_) = sh.cells[r.r1 as usize * sh.ncols + r.c1 as usize] { return Ok(V::B(false)); } } }
                let v = self.scalar(v); V::B(matches!(v, V::Empty)) }
            "ISERROR" => { let v = self.evs(&a[0]); V::B(matches!(v, V::E(_))) }
            "ISNA" => { let v = self.evs(&a[0]); V::B(matches!(v, V::E(Er::NA))) }
            "ISNUMBER" => { let v = self.evs(&a[0]); V::B(matches!(v, V::N(_) | V::B(_))) }
            "ISTEXT" => { let v = self.evs(&a[0]); V::B(matches!(v, V::S(_))) }
            "ISLOGICAL" => { let pl = self.lastlog; let v = self.evs(&a[0]); V::B(matches!(v, V::B(_)) || (pl && matches!(a[0], Ex::Lit(_)))) }
            "LEN" => V::N(st!(0).chars().count() as f64),
            "LOWER" => V::S(Rc::from(st!(0).to_lowercase().as_str())),
            "UPPER" => V::S(Rc::from(st!(0).to_uppercase().as_str())),
            "TRIM" => { let s = st!(0); let t: Vec<&str> = s.split(' ').filter(|x| !x.is_empty()).collect(); V::S(Rc::from(t.join(" ").as_str())) }
            "PROPER" => {
                let s = st!(0); let mut o = String::new(); let mut prev = false;
                for ch in s.chars() { if ch.is_alphabetic() { if prev { o.extend(ch.to_lowercase()) } else { o.extend(ch.to_uppercase()) } prev = true; } else { o.push(ch); prev = false; } }
                V::S(Rc::from(o.as_str()))
            }
            "LEFT" | "RIGHT" => {
                let s = st!(0); let k = if n > 1 { num!(1) } else { 1.0 }; if k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect(); let k = (k as usize).min(ch.len());
                let r: String = if name == "LEFT" { ch[..k].iter().collect() } else { ch[ch.len() - k..].iter().collect() };
                V::S(Rc::from(r.as_str()))
            }
            "MID" => {
                let s = st!(0); let p = num!(1); let k = num!(2); if p < 1.0 || k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect(); let p = (p as usize - 1).min(ch.len()); let e = (p + k as usize).min(ch.len());
                V::S(Rc::from(ch[p..e].iter().collect::<String>().as_str()))
            }
            "REPLACE" => {
                let s = st!(0); let p = num!(1); let k = num!(2); let t = st!(3); if p < 1.0 || k < 0.0 { return Err(Er::Value); }
                let ch: Vec<char> = s.chars().collect(); let p = (p as usize - 1).min(ch.len()); let e = (p + k as usize).min(ch.len());
                let r: String = ch[..p].iter().collect::<String>() + &t + &ch[e..].iter().collect::<String>();
                V::S(Rc::from(r.as_str()))
            }
            "REPT" => { let s = st!(0); let k = num!(1); if k < 0.0 || (s.len() as f64) * k.trunc() > 1048576.0 { return Err(Er::Value); } V::S(Rc::from(s.repeat(k as usize).as_str())) }
            "CONCATENATE" | "CONCAT" => {
                let mut o = String::new();
                let mut err = None;
                for x in a { let v = self.ev(x); match v { V::R(_) | V::A(_) if name == "CONCAT" => { let ar = self.to_arr(v); for y in ar.d.iter() { match to_str(y) { Ok(t) => o.push_str(&t), Err(e) => { err.get_or_insert(e); break; } } } } v => { let v = self.scalar(v); match to_str(&v) { Ok(t) => o.push_str(&t), Err(e) => { err.get_or_insert(e); } } } } }
                if let Some(e) = err { return Err(e); }
                V::S(Rc::from(o.as_str()))
            }
            "TEXTJOIN" => {
                let d = st!(0); let skip = self.bl(&a[1])?; let mut parts: Vec<Rc<str>> = Vec::new();
                for x in &a[2..] { let v = self.ev(x); let rf = if let V::R(r) = v { Some(r) } else { None }; let ar = self.to_arr(v);
                    for (k, y) in ar.d.iter().enumerate() {
                        let fe = if let Some(r) = rf { let w = (r.c2 - r.c1 + 1) as usize; let (rr, cc) = (r.r1 as usize + k / w, r.c1 as usize + k % w); let sh = &self.sheets[r.s as usize]; if rr < sh.nrows && cc < sh.ncols { if let Cell::F(fi) = sh.cells[rr * sh.ncols + cc] { self.fs[fi as usize].er } else { false } } else { false } } else { false };
                        let s = if fe { Rc::from("") } else { to_str(y)? }; if skip && s.is_empty() { continue; } parts.push(s); } }
                V::S(Rc::from(parts.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(&d).as_str()))
            }
            "EXACT" => { let x = st!(0); let y = st!(1); V::B(x == y) }
            "FIND" | "SEARCH" => {
                let pat = st!(0); let s = st!(1); let start = if n > 2 { num!(2) } else { 1.0 };
                let sc: Vec<char> = s.chars().collect();
                if start < 1.0 || start as usize > sc.len() || pat.is_empty() { return Err(Er::Value); }
                let st0 = start as usize - 1;
                if name == "FIND" {
                    let pc: Vec<char> = pat.chars().collect();
                    for i in st0..=sc.len() { if i + pc.len() <= sc.len() && sc[i..i + pc.len()] == pc[..] { return Ok(V::N((i + 1) as f64)); } }
                    return Err(Er::Value);
                } else {
                    let pc: Vec<char> = pat.to_lowercase().chars().collect(); let sl: Vec<char> = s.to_lowercase().chars().collect();
                    let wild = pc.iter().any(|&c| c == '*' || c == '?' || c == '~');
                    for i in st0..=sl.len() {
                        if wild {
                            for j in i..=sl.len() { let mut p2 = pc.clone(); p2.push('*'); let _ = j; if wild_match(&p2, &sl[i..]) { return Ok(V::N((i + 1) as f64)); } break; }
                        } else if i + pc.len() <= sl.len() && sl[i..i + pc.len()] == pc[..] { return Ok(V::N((i + 1) as f64)); }
                    }
                    return Err(Er::Value);
                }
            }
            "SUBSTITUTE" => {
                let s = st!(0); let o = st!(1); let nw = st!(2);
                if o.is_empty() { return Ok(V::S(s)); }
                if n > 3 {
                    let k = num!(3) as i64; if k < 1 { return Err(Er::Value); }
                    let mut cnt = 0; let mut idx = None; let mut p = 0;
                    while let Some(f) = s[p..].find(o.as_ref()) { cnt += 1; if cnt == k { idx = Some(p + f); break; } p += f + o.len(); }
                    match idx { Some(i) => V::S(Rc::from(format!("{}{}{}", &s[..i], nw, &s[i + o.len()..]).as_str())), None => V::S(s) }
                } else { V::S(Rc::from(s.replace(o.as_ref(), &nw).as_str())) }
            }
            "TEXT" => { let v = self.evs(&a[0]); let f = st!(1); if let V::E(e) = v { return Err(e); } V::S(Rc::from(text_fmt(&v, &f)?.as_str())) }
            "DATE" => {
                let y = num!(0).trunc() as i64; let m = num!(1).trunc() as i64; let d = num!(2).trunc() as i64;
                if y < 0 { return Err(Er::Value); }
                let y = if y < 30 { y + 2000 } else if y < 100 { y + 1900 } else { y };
                if y < 1583 || y > 9999 { return Err(Er::Value); }
                if m.abs() > 32767 || d.abs() > 32767 { return Err(Er::Value); }
                let tm = y * 12 + (m - 1); let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) + 1);
                let s = ymd_to_serial(yy, mm, 1) + d - 1; if yy > 9999 || yy < 1583 { return Err(Er::Value); } V::N(s as f64)
            }
            "YEAR" | "MONTH" | "DAY" => { let x = num!(0); let (y, m, d) = serial_to_ymd(x.floor() as i64); V::N(match name { "YEAR" => y, "MONTH" => m, _ => d } as f64) }
            "WEEKDAY" => {
                let x = num!(0).floor() as i64; let t = if n > 1 { num!(1) as i64 } else { 1 };
                let w = (x + 6).rem_euclid(7); // 0=Sunday (serial 1 = 1899-12-31 Sunday)
                V::N(match t { 1 | 17 => w + 1, 2 | 11 => (w + 6) % 7 + 1, 3 => (w + 6) % 7, 12 => (w + 5) % 7 + 1, 13 => (w + 4) % 7 + 1, 14 => (w + 3) % 7 + 1, 15 => (w + 2) % 7 + 1, 16 => (w + 1) % 7 + 1, _ => return Err(Er::Value) } as f64)
            }
            "DAYS" => { let e = num!(0); let s = num!(1); V::N(e - s) }
            "EDATE" | "EOMONTH" => {
                let x = num!(0); let m = num!(1).trunc() as i64;
                let (y, mo, d) = serial_to_ymd(x.trunc() as i64);
                let tm = y * 12 + mo - 1 + m; let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) + 1);
                let dim = days_in_month(yy, mm);
                let dd = if name == "EOMONTH" { dim } else { d.min(dim) };
                V::N(ymd_to_serial(yy, mm, dd) as f64)
            }
            "DATEDIF" => {
                let s = num!(0).floor() as i64; let e = num!(1).floor() as i64; let u = st!(2).to_uppercase();
                if s == e && !["D","M","Y","YM","MD","YD"].contains(&u.as_str()) { return Ok(V::N(0.0)); }
                if s > e { return Err(Er::Num); }
                let (y1, m1, d1) = serial_to_ymd(s); let (y2, m2, d2) = serial_to_ymd(e);
                let months = (y2 * 12 + m2) - (y1 * 12 + m1) - if d2 < d1 { 1 } else { 0 };
                V::N(match u.as_str() {
                    "D" => (e - s) as f64,
                    "M" => months as f64,
                    "Y" => (months / 12) as f64,
                    "YM" => (months % 12) as f64,
                    "MD" => { if d2 >= d1 { (d2 - d1) as f64 } else { let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) }; (days_in_month(py, pm) - d1 + d2).max(d2 - d1 + days_in_month(py, pm)) as f64 } }
                    "YD" => { let mut ys = ymd_to_serial(y2, m1, d1.min(days_in_month(y2, m1))); if ys > e { ys = ymd_to_serial(y2 - 1, m1, d1.min(days_in_month(y2 - 1, m1))); } (e - ys) as f64 }
                    _ => return Err(Er::Value),
                })
            }
            "INDEX" => {
                let v = self.ev(&a[0]);
                let r = if n > 1 { num!(1).trunc() as i64 } else { 0 }; let c = if n > 2 { num!(2).trunc() as i64 } else { 0 };
                match v {
                    V::R(rf) => {
                        let h = (rf.r2 - rf.r1 + 1) as i64; let w = (rf.c2 - rf.c1 + 1) as i64;
                        let (r, c) = if n == 2 && h == 1 { (1.max(0), r) } else { (r, c) };
                        if r < 0 || c < 0 { return Err(Er::Value); } if r > h || c > w { return Err(Er::Ref); }
                        let (r1, r2) = if r == 0 { (rf.r1, rf.r2) } else { (rf.r1 + r as u32 - 1, rf.r1 + r as u32 - 1) };
                        let (c1, c2) = if c == 0 { (rf.c1, rf.c2) } else { (rf.c1 + c as u32 - 1, rf.c1 + c as u32 - 1) };
                        V::R(Rf { s: rf.s, r1, r2, c1, c2 })
                    }
                    V::E(e) => return Err(e),
                    other => { let ar = self.to_arr(other); let r = r.max(1) as usize; let c = c.max(1) as usize; if r > ar.rows || c > ar.cols { return Err(Er::Ref); } ar.d[(r - 1) * ar.cols + c - 1].clone() }
                }
            }
            "VLOOKUP" | "HLOOKUP" => {
                let key = self.evs(&a[0]); if let V::E(e) = key { return Err(e); }
                let rf = self.range_of(&a[1])?; let idx = num!(2).trunc() as i64;
                let approx = if n > 3 { if let Ex::Lit(V::Empty) = a[3] { false } else { self.bl(&a[3])? } } else { true };
                let v = name == "VLOOKUP";
                let len = if v { rf.r2 - rf.r1 + 1 } else { rf.c2 - rf.c1 + 1 };
                let width = if v { rf.c2 - rf.c1 + 1 } else { rf.r2 - rf.r1 + 1 };
                if idx < 1 || idx as u32 > width { return Err(Er::Value); }
                let get = |e: &mut Eng, i: u32| if v { e.cell(rf.s, rf.r1 + i, rf.c1) } else { e.cell(rf.s, rf.r1, rf.c1 + i) };
                let irf = if v { Rf { s: rf.s, r1: rf.r1, c1: rf.c1, r2: rf.r2, c2: rf.c1 } } else { Rf { s: rf.s, r1: rf.r1, c1: rf.c1, r2: rf.r1, c2: rf.c2 } };
                let pos = if approx { self.bsearch(len, &key, &get) } else { self.lin(len, &key, &get, irf) };
                let Some(p) = pos else { return Err(Er::NA) };
                let r = if v { self.cell(rf.s, rf.r1 + p, rf.c1 + idx as u32 - 1) } else { self.cell(rf.s, rf.r1 + idx as u32 - 1, rf.c1 + p) };
                r
            }
            "MATCH" => {
                let key = self.evs(&a[0]); if let V::E(e) = key { return Err(e); }
                let rf = self.range_of(&a[1])?; let t = if n > 2 { num!(2).trunc() } else { 1.0 }; if t != 0.0 && t != 1.0 && t != -1.0 { return Err(Er::Value); }
                let vert = rf.c1 == rf.c2; if !vert && rf.r1 != rf.r2 { return Err(Er::NA); }
                let len = if vert { rf.r2 - rf.r1 + 1 } else { rf.c2 - rf.c1 + 1 };
                let get = |e: &mut Eng, i: u32| if vert { e.cell(rf.s, rf.r1 + i, rf.c1) } else { e.cell(rf.s, rf.r1, rf.c1 + i) };
                let irf = if vert { Rf { s: rf.s, r1: rf.r1, c1: rf.c1, r2: rf.r2, c2: rf.c1 } } else { Rf { s: rf.s, r1: rf.r1, c1: rf.c1, r2: rf.r1, c2: rf.c2 } };
                let pos = if t == 0.0 { self.lin(len, &key, &get, irf) } else if t > 0.0 { self.bsearch(len, &key, &get) } else { self.bsearch_desc(len, &key, &get) };
                match pos { Some(p) => V::N((p + 1) as f64), None => return Err(Er::NA) }
            }
            "XLOOKUP" => {
                let key = self.evs(&a[0]); if let V::E(e) = key { return Err(e); }
                let lr = self.range_of(&a[1])?; let rr = self.range_of(&a[2])?;
                let mm = if n > 4 { if let Ex::Lit(V::Empty) = a[4] { 0 } else { num!(4) as i64 } } else { 0 };
                let sm = if n > 5 { num!(5) as i64 } else { 1 };
                let vert = lr.c1 == lr.c2;
                let len = if vert { lr.r2 - lr.r1 + 1 } else { lr.c2 - lr.c1 + 1 };
                let get = |e: &mut Eng, i: u32| if vert { e.cell(lr.s, lr.r1 + i, lr.c1) } else { e.cell(lr.s, lr.r1, lr.c1 + i) };
                let mut best: Option<(u32, V)> = None;
                if mm == 0 && sm == 1 {
                    let irf = if vert { Rf { s: lr.s, r1: lr.r1, c1: lr.c1, r2: lr.r2, c2: lr.c1 } } else { Rf { s: lr.s, r1: lr.r1, c1: lr.c1, r2: lr.r1, c2: lr.c2 } };
                    let has_wild = matches!(&key, V::S(s) if s.contains('*') || s.contains('?') || s.contains('~'));
                    if !has_wild { if let Some(p) = self.lin(len, &key, &get, irf) { best = Some((p, V::Empty)); } }
                }
                let order: Vec<u32> = if sm < 0 { (0..len).rev().collect() } else { (0..len).collect() };
                let kc: Vec<char> = if let V::S(s) = &key { s.to_lowercase().chars().collect() } else { vec![] };
                for i in order {
                    if best.is_some() && mm == 0 && sm == 1 { break; }
                    if mm == 0 && sm == 1 && !matches!(&key, V::S(s) if s.contains('*') || s.contains('?') || s.contains('~')) { break; }
                    let v = get(self, i);
                    if mm == 2 {
                        if let (V::S(s), V::S(_)) = (&v, &key) { let sc: Vec<char> = s.to_lowercase().chars().collect(); if wild_match(&kc, &sc) { best = Some((i, v)); break; } }
                        else if eq_vals(&v, &key) { best = Some((i, v)); break; }
                        continue;
                    }
                    if eq_vals(&v, &key) { best = Some((i, v)); break; }
                    if mm == 0 || matches!(v, V::Empty) { continue; }
                    if std::mem::discriminant(&v) != std::mem::discriminant(&key) { continue; }
                    let o = cmp_vals(&v, &key);
                    let better = |b: &V| { let ob = cmp_vals(&v, b); if mm == -1 { ob == std::cmp::Ordering::Greater } else { ob == std::cmp::Ordering::Less } };
                    if (mm == -1 && o == std::cmp::Ordering::Less) || (mm == 1 && o == std::cmp::Ordering::Greater) {
                        if best.as_ref().map_or(true, |(_, b)| better(b)) { best = Some((i, v)); }
                    }
                }
                match best {
                    Some((p, _)) => {
                        let rv = if vert { Rf { s: rr.s, r1: rr.r1 + p, r2: rr.r1 + p, c1: rr.c1, c2: rr.c2 } } else { Rf { s: rr.s, r1: rr.r1, r2: rr.r2, c1: rr.c1 + p, c2: rr.c1 + p } };
                        V::R(rv)
                    }
                    None => if n > 3 && !matches!(a[3], Ex::Lit(V::Empty)) { self.ev(&a[3]) } else { return Err(Er::NA) },
                }
            }
            _ => return Err(Er::Name),
        })
    }
    fn lin(&mut self, len: u32, key: &V, get: &dyn Fn(&mut Eng, u32) -> V, irf: Rf) -> Option<u32> {
        let (wild, kc) = match key { V::S(s) if s.contains('*') || s.contains('?') || s.contains('~') => (true, s.to_lowercase().chars().collect::<Vec<char>>()), _ => (false, vec![]) };
        if !wild {
            let k = match lk_key(key) { Some(k) => k, None => return None };
            let ck = (irf.s, irf.r1, irf.c1, irf.r2, irf.c2);
            if !self.lcache.contains_key(&ck) {
                let mut m: HashMap<LK, u32> = HashMap::with_capacity(len as usize);
                for i in 0..len { let v = get(self, i); if let Some(k) = lk_key(&v) { m.entry(k).or_insert(i); } }
                self.lcache.insert(ck, Rc::new(m));
            }
            return self.lcache[&ck].get(&k).copied();
        }
        for i in 0..len {
            let v = get(self, i);
            if wild { if let V::S(s) = &v { let sc: Vec<char> = s.to_lowercase().chars().collect(); if wild_match(&kc, &sc) { return Some(i); } } continue; }
            if eq_vals(&v, key) { return Some(i); }
        }
        None
    }
    fn bsearch(&mut self, len: u32, key: &V, get: &dyn Fn(&mut Eng, u32) -> V) -> Option<u32> {
        let (mut lo, mut hi) = (0i64, len as i64 - 1); let mut res = None;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let v = get(self, mid as u32);
            let o = if matches!(v, V::Empty) { std::cmp::Ordering::Less } else { cmp_vals(&v, key) };
            if o != std::cmp::Ordering::Greater { if rank_t(&v) == rank_t(key) { res = Some(mid as u32); } lo = mid + 1; } else { hi = mid - 1; }
        }
        res
    }
    fn bsearch_desc(&mut self, len: u32, key: &V, get: &dyn Fn(&mut Eng, u32) -> V) -> Option<u32> {
        use std::cmp::Ordering::*;
        let mut res = None;
        for i in 0..len {
            let v = get(self, i);
            if matches!(v, V::Empty) || rank_t(&v) != rank_t(key) { continue; }
            match cmp_vals(&v, key) { Equal => return Some(i), Greater => res = Some(i), Less => break }
        }
        res
    }
}
fn rank_t(v: &V) -> u8 { match v { V::N(_) | V::B(_) => 0, V::S(_) => 1, V::Empty => 4, _ => 3 } }

fn eq_vals(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::N(x), V::N(y)) => x == y,
        (V::S(x), V::S(y)) => x.to_lowercase() == y.to_lowercase(),
        (V::B(x), V::B(y)) => x == y,
        (V::B(x), V::N(y)) | (V::N(y), V::B(x)) => (*x as u8 as f64) == *y,
        _ => false,
    }
}

fn pow(x: f64, y: f64) -> R<V> {
    if x == 0.0 && y < 0.0 { return Err(Er::Num); }
    let r = x.powf(y); if !r.is_finite() { return Err(Er::Num); } Ok(V::N(r))
}

fn approx_eq(a: f64, b: f64) -> bool {
    if a == b { return true; }
    let d = (a - b).abs(); d < (a.abs().max(b.abs())) * 2.220446049250313e-16 * 8.0
}

fn binop(op: u8, a: &V, b: &V) -> V {
    if let V::E(e) = a { return V::E(*e); }
    if let V::E(e) = b { return V::E(*e); }
    match op {
        0..=4 => {
            let x = match to_num(a) { Ok(v) => v, Err(e) => return V::E(e) };
            let y = match to_num(b) { Ok(v) => v, Err(e) => return V::E(e) };
            match op {
                0 => { let r = x + y; V::N(if approx_eq(x, -y) { 0.0 } else { r }) }
                1 => { let r = x - y; V::N(if approx_eq(x, y) { 0.0 } else { r }) }
                2 => { let r = x * y; if r.is_finite() { V::N(r) } else { V::E(Er::Num) } }
                3 => if y == 0.0 { V::E(Er::Div0) } else { let r = x / y; if r.is_finite() { V::N(r) } else { V::E(Er::Num) } },
                _ => pow(x, y).unwrap_or_else(V::E),
            }
        }
        11 => { let x = match to_str(a) { Ok(s) => s, Err(e) => return V::E(e) }; let y = match to_str(b) { Ok(s) => s, Err(e) => return V::E(e) }; V::S(Rc::from(format!("{}{}", x, y).as_str())) }
        _ => {
            let o = match (a, b) { (V::N(x), V::N(y)) if approx_eq(*x, *y) => std::cmp::Ordering::Equal, _ => cmp_vals(a, b) };
            use std::cmp::Ordering::*;
            V::B(match op { 5 => o != Greater, 6 => o != Less, 7 => o != Equal, 8 => o == Less, 9 => o == Greater, _ => o == Equal })
        }
    }
}

fn round_mode(x: f64, d: f64, mode: u8) -> f64 {
    let d = d as i32;
    let f = 10f64.powi(d.abs());
    let y = if d >= 0 { x * f } else { x / f };
    // correct representation error
    let yr = { let s = format!("{:.12e}", y); s.parse::<f64>().unwrap_or(y) };
    let y2 = if (yr - y).abs() <= y.abs() * 1e-14 { yr } else { y };
    let r = match mode {
        0 => y2.trunc(),
        1 => { let t = y2.abs().floor(); let fr = y2.abs() - t; let v = if fr >= 0.5 - 1e-12 && fr >= 0.5 { t + 1.0 } else { t }; v.copysign(y2) }
        _ => { let t = y2.abs().ceil(); t.copysign(y2) }
    };
    let r = if d >= 0 { r / f } else { r * f };
    if r == 0.0 { 0.0 } else { r }
}

// ---------------- xlsx loading ----------------
fn unescape(s: &str) -> String {
    if memchr::memchr(b'&', s.as_bytes()).is_none() { return s.to_string(); }
    let mut o = String::with_capacity(s.len()); let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]); let r = &rest[i..];
        let (rep, l) = if r.starts_with("&lt;") { ("<", 4) } else if r.starts_with("&gt;") { (">", 4) } else if r.starts_with("&amp;") { ("&", 5) } else if r.starts_with("&quot;") { ("\"", 6) } else if r.starts_with("&apos;") { ("'", 6) } else { ("&", 1) };
        o.push_str(rep); rest = &r[l..];
    }
    o.push_str(rest);
    return o;
    #[allow(unreachable_code)]
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn read_zip(path: &str) -> HashMap<String, String> {
    let f = std::fs::File::open(path).expect("open");
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).expect("zip");
    let mut m = HashMap::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        let name = e.name().to_string();
        if name.ends_with(".xml") || name.ends_with(".rels") { let mut s = String::new(); e.read_to_string(&mut s).unwrap(); m.insert(name, s); }
    }
    m
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!(" {}=\"", name);
    let i = tag.find(&pat)? + pat.len();
    let j = tag[i..].find('"')? + i;
    Some(unescape(&tag[i..j]))
}

fn text_between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let i = s.find(open)?; let st = s[i..].find('>')? + i + 1;
    if s[i..st].ends_with("/>") { return Some(""); }
    let j = s[st..].find(close)? + st; Some(&s[st..j])
}

fn all_t(s: &str) -> String {
    // concatenate all <t ...>..</t>
    let mut o = String::new(); let mut p = 0;
    while let Some(i) = s[p..].find("<t") {
        let i = p + i; let nx = s.as_bytes().get(i + 2).copied().unwrap_or(0);
        if nx != b'>' && nx != b' ' { p = i + 2; continue; }
        let st = s[i..].find('>').unwrap() + i + 1;
        if s[i..st].ends_with("/>") { p = st; continue; }
        let j = s[st..].find("</t>").unwrap() + st; o.push_str(&unescape(&s[st..j])); p = j + 4;
    }
    o
}

fn load(path: &str) -> (Eng, Vec<String>) {
    let tz = std::time::Instant::now();
    let files = read_zip(path);
    if std::env::var("CALC_T").is_ok() { eprintln!("zip {:?}", tz.elapsed()); }
    let wb = &files["xl/workbook.xml"];
    let rels = files.get("xl/_rels/workbook.xml.rels").cloned().unwrap_or_default();
    let mut relmap = HashMap::new();
    for part in rels.split("<Relationship").skip(1) {
        let tag = &part[..part.find('>').unwrap_or(part.len())];
        if let (Some(id), Some(t)) = (attr(tag, "Id"), attr(tag, "Target")) {
            let t = if let Some(x) = t.strip_prefix('/') { x.to_string() } else { format!("xl/{}", t) };
            relmap.insert(id, t);
        }
    }
    let mut sheet_files = Vec::new();
    for part in wb.split("<sheet ").skip(1) {
        let tag = format!(" {}", &part[..part.find('>').unwrap()]);
        let name = attr(&tag, "name").unwrap();
        let rid = attr(&tag, "r:id").unwrap_or_default();
        let f = relmap.get(&rid).cloned().unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", sheet_files.len() + 1));
        sheet_files.push((name, f));
    }
    let mut sst: Vec<Rc<str>> = Vec::new();
    if let Some(s) = files.get("xl/sharedStrings.xml") {
        for part in s.split("<si>").skip(1) { let e = part.find("</si>").unwrap_or(part.len()); sst.push(Rc::from(all_t(&part[..e]).as_str())); }
    }
    let mut names = HashMap::new();
    for (i, (n, _)) in sheet_files.iter().enumerate() { names.insert(n.to_lowercase(), i as u16); }
    let mut eng = Eng { lcache: HashMap::new(), mcache: HashMap::new(), sheets: Vec::new(), fs: Vec::new(), arrctx: 0, cur: (0, 0, 0), lastlog: false };
    let mut shared: HashMap<(usize, String), (u32, u32, String)> = HashMap::new();
    let mut ftexts: Vec<String> = Vec::new(); let mut pre: Vec<Option<Ex>> = Vec::new();
    for (si, (name, f)) in sheet_files.iter().enumerate() {
        let empty = String::new(); let x = files.get(f).unwrap_or(&empty);
        let xb = x.as_bytes();
        // split into chunks at <row boundaries
        let nch = if xb.len() > 200_000 { 8 } else { 1 };
        let mut bounds = vec![0usize];
        for k in 1..nch { let t = xb.len() * k / nch; let t = t.max(*bounds.last().unwrap()); if let Some(j) = memfind(&xb[t..], b"<row ") { bounds.push(t + j); } }
        bounds.push(xb.len());
        let chunks: Vec<&str> = bounds.windows(2).map(|w| &x[w[0]..w[1]]).collect();
        use rayon::prelude::*;
        let ts = std::time::Instant::now();
        let parts: Vec<Vec<(u32, u32, Item)>> = chunks.par_iter().map(|c| scan(c, &sst, &names, si as u16)).collect();
        if std::env::var("CALC_T").is_ok() { eprintln!("scan {} {:?}", si, ts.elapsed()); }
        let mut cells: Vec<(u32, u32, Cell)> = Vec::new();
        let (mut mr, mut mc) = (0u32, 0u32);
        for part in parts { for (row, col, it) in part {
            match it {
                Item::V(v) => cells.push((row, col, Cell::V(v))),
                Item::P(ex) => {
                    let idx = eng.fs.len() as u32;
                    ftexts.push(String::new()); pre.push(Some(ex));
                    eng.fs.push(Formula { s: si as u16, r: row, c: col, ex: Ex::Lit(V::Empty), state: 0, val: V::Empty, er: false });
                    cells.push((row, col, Cell::F(idx)));
                }
                Item::F(ftext, sh) => {
                    let ftext = if let Some((si_, has)) = sh {
                        if has { shared.insert((si, si_), (row, col, ftext.clone())); ftext }
                        else if let Some((r0, c0, t0)) = shared.get(&(si, si_)) { shift_formula(t0, row as i64 - *r0 as i64, col as i64 - *c0 as i64) } else { String::new() }
                    } else { ftext };
                    let idx = eng.fs.len() as u32;
                    ftexts.push(ftext); pre.push(None);
                    eng.fs.push(Formula { s: si as u16, r: row, c: col, ex: Ex::Lit(V::Empty), state: 0, val: V::Empty, er: false });
                    cells.push((row, col, Cell::F(idx)));
                }
            }
            mr = mr.max(row + 1); mc = mc.max(col + 1);
        } }
        let mut sh = Sheet { name: name.clone(), ncols: mc as usize, nrows: mr as usize, cells: vec![Cell::Empty; (mr as usize) * (mc as usize)] };
        for (r, c, cl) in cells { sh.cells[r as usize * sh.ncols + c as usize] = cl; }
        eng.sheets.push(sh);
    }
    let tp = std::time::Instant::now();
    {
        use rayon::prelude::*;
        let sheets_of: Vec<u16> = eng.fs.iter().map(|f| f.s).collect();
        for (k, p) in pre.into_iter().enumerate() {
            match p { Some(e) => eng.fs[k].ex = e, None => { let mut pr = P { b: ftexts[k].as_bytes(), i: 0, names: &names, cs: sheets_of[k] }; eng.fs[k].ex = pr.expr(); } }
        }
    }
    if std::env::var("CALC_T").is_ok() { eprintln!("parse {:?}", tp.elapsed()); }
    let names_v = sheet_files.into_iter().map(|x| x.0).collect();
    (eng, names_v)
}

fn memfind(h: &[u8], n: &[u8]) -> Option<usize> { memchr::memmem::find(h, n) }
fn attr_raw<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let b = tag.as_bytes(); let nb = name.as_bytes(); let mut p = 0;
    while let Some(i) = memchr::memmem::find(&b[p..], nb) {
        let i = p + i;
        if i > 0 && b[i - 1] == b' ' && b.get(i + nb.len()) == Some(&b'=') && b.get(i + nb.len() + 1) == Some(&b'"') {
            let st = i + nb.len() + 2; let e = memchr::memchr(b'"', &b[st..])? + st; return Some(&tag[st..e]);
        }
        p = i + 1;
    }
    None
}

fn shift_formula(f: &str, dr: i64, dc: i64) -> String {
    // shift relative refs outside of strings
    let b = f.as_bytes(); let mut o = String::new(); let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' { let st = i; i += 1; while i < b.len() { if b[i] == b'"' { if i + 1 < b.len() && b[i + 1] == b'"' { i += 2; continue; } i += 1; break; } i += 1; } o.push_str(&f[st..i]); continue; }
        if c.is_ascii_alphabetic() || c == b'$' {
            let st = i; while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'$' || b[i] == b'_' || b[i] == b'.') { i += 1; }
            let tok = &f[st..i];
            if (i >= b.len() || b[i] != b'(') && (i >= b.len() || b[i] != b'!') && parse_a1(tok).is_some() {
                let tb = tok.as_bytes(); let mut j = 0; let ca = tb[0] == b'$'; if ca { j = 1; }
                let cs = j; while tb[j].is_ascii_alphabetic() { j += 1; } let col = col_to_n(&tok[cs..j]) as i64;
                let ra = tb[j] == b'$'; if ra { j += 1; } let row: i64 = tok[j..].parse().unwrap();
                let nc = if ca { col } else { col + dc }; let nr = if ra { row } else { row + dr };
                o.push_str(&format!("{}{}{}{}", if ca { "$" } else { "" }, n_to_col(nc as u32), if ra { "$" } else { "" }, nr));
            } else { o.push_str(tok); }
            continue;
        }
        o.push(c as char); i += 1;
    }
    o
}
fn n_to_col(mut n: u32) -> String { let mut s = Vec::new(); n += 1; while n > 0 { let r = (n - 1) % 26; s.push(b'A' + r as u8); n = (n - 1) / 26; } s.reverse(); String::from_utf8(s).unwrap() }

fn json_str(o: &mut String, s: &str) {
    o.push('"');
    for ch in s.chars() { match ch { '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), '\t' => o.push_str("\\t"), c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)), c => o.push(c) } }
    o.push('"');
}

fn run(inp: &str, out: &str) {
    let t0 = std::time::Instant::now();
    let (mut eng, names) = load(inp);
    let t1 = std::time::Instant::now();
    std::panic::set_hook(Box::new(|_| {}));
    for i in 0..eng.fs.len() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { eng.calc(i); }));
        if r.is_err() {
            for f in eng.fs.iter_mut() { if f.state == 1 { f.state = 2; f.val = V::E(Er::Value); } }
            eng.arrctx = 0; eng.lastlog = false;
        }
    }
    let t2 = std::time::Instant::now();
    if std::env::var("CALC_T").is_ok() { eprintln!("load {:?} calc {:?}", t1 - t0, t2 - t1); }
    let mut o = String::with_capacity(eng.fs.len() * 40);
    o.push('{');
    for (si, nm) in names.iter().enumerate() {
        if si > 0 { o.push(','); }
        json_str(&mut o, nm); o.push_str(":{");
        let mut first = true;
        for f in eng.fs.iter().filter(|f| f.s as usize == si) {
            if !first { o.push(','); } first = false;
            o.push('"'); o.push_str(&n_to_col(f.c)); o.push_str(itoa::Buffer::new().format(f.r + 1)); o.push_str("\":");
            match &f.val {
                V::N(n) => { if n.is_finite() { o.push_str("{\"t\":\"n\",\"v\":"); let mut b = ryu_fmt(*n); if b == "-0" { b = "0".into(); } o.push_str(&b); o.push('}'); } else { o.push_str("{\"t\":\"e\",\"v\":\"#NUM!\"}"); } }
                V::S(s) => { o.push_str("{\"t\":\"s\",\"v\":"); json_str(&mut o, s); o.push('}'); }
                V::B(b) => { o.push_str(if *b { "{\"t\":\"b\",\"v\":true}" } else { "{\"t\":\"b\",\"v\":false}" }); }
                V::E(e) => { o.push_str("{\"t\":\"e\",\"v\":\""); o.push_str(e.s()); o.push_str("\"}"); }
                _ => o.push_str("{\"t\":\"n\",\"v\":0}"),
            }
        }
        o.push('}');
    }
    o.push('}');
    let _ = eng.sheets.iter().map(|s| &s.name).count();
    std::fs::File::create(out).unwrap().write_all(o.as_bytes()).unwrap();
    if std::env::var("CALC_T").is_ok() { eprintln!("total {:?}", t0.elapsed()); }
    std::process::exit(0);
}

fn ryu_fmt(n: f64) -> String { if n == n.trunc() && n.abs() < 1e15 { itoa::Buffer::new().format(n as i64).to_string() } else { ryu::Buffer::new().format(n).to_string() } }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut inp = String::new(); let mut out = String::new();
    let mut i = 1;
    while i < args.len() { match args[i].as_str() { "--in" => { inp = args[i + 1].clone(); i += 1; } "--out" => { out = args[i + 1].clone(); i += 1; } _ => {} } i += 1; }
    let h = std::thread::Builder::new().stack_size(4 << 30).spawn(move || run(&inp, &out)).unwrap();
    h.join().unwrap();
}
pub fn to_num_pub(v: &V) -> R<f64> { to_num(v) }

#[derive(Default)]
pub struct Kahan { s: f64, c: f64 }
impl Kahan {
    #[inline] fn add(&mut self, x: f64) { let t = self.s + x; if self.s.abs() >= x.abs() { self.c += (self.s - t) + x; } else { self.c += (x - t) + self.s; } self.s = t; }
    #[inline] fn get(&self) -> f64 { self.s + self.c }
}

fn crit_key(v: &V) -> String {
    match v { V::N(x) => format!("n{}", x.to_bits()), V::S(s) => format!("s{}", s), V::B(b) => format!("b{}", b), V::Empty => "e".into(), _ => "x".into() }
}

#[derive(Hash, PartialEq, Eq)]
pub enum LK { N(u64), S(String) }
fn lk_key(v: &V) -> Option<LK> {
    match v {
        V::N(x) => Some(LK::N(if *x == 0.0 { 0 } else { x.to_bits() })),
        V::B(b) => Some(LK::N(if *b { 1f64.to_bits() } else { 0 })),
        V::S(s) => Some(LK::S(s.to_lowercase())),
        _ => None,
    }
}

enum Item { V(V), F(String, Option<(String, bool)>), P(Ex) }
fn scan(x: &str, sst: &[Rc<str>], names: &HashMap<String, u16>, si: u16) -> Vec<(u32, u32, Item)> {
    let mut out = Vec::new();
    let mut p = 0; let b = x.as_bytes();
    while let Some(i) = memfind(&b[p..], b"<c ") {
        let i = p + i;
        let te = memchr::memchr(b'>', &b[i..]).unwrap() + i;
        let tag = &x[i + 2..te];
        let selfclose = b[te - 1] == b'/';
        let body; if selfclose { body = ""; p = te + 1; } else { let e = memfind(&b[te..], b"</c>").unwrap() + te; body = &x[te + 1..e]; p = e + 4; }
        let r = attr_raw(tag, "r").unwrap();
        let (row, col) = parse_a1(r).unwrap();
        let t = attr_raw(tag, "t").unwrap_or("n");
        if let Some(fi) = memfind(body.as_bytes(), b"<f") {
            let fend = memchr::memchr(b'>', &body.as_bytes()[fi..]).unwrap() + fi;
            let ftag = &body[fi..fend];
            let ftext = if body.as_bytes()[fend - 1] == b'/' { String::new() } else { let e = memfind(&body.as_bytes()[fend..], b"</f>").unwrap() + fend; unescape(&body[fend + 1..e]) };
            let sh = if attr_raw(ftag, "t") == Some("shared") { let si_ = attr_raw(ftag, "si").unwrap_or_default().to_string(); let has = !ftext.is_empty(); Some((si_, has)) } else { None };
            if sh.is_none() { let mut pr = P { b: ftext.as_bytes(), i: 0, names, cs: si }; out.push((row, col, Item::P(pr.expr()))); }
            else { out.push((row, col, Item::F(ftext, sh))); }
        } else {
            let v = match t {
                "inlineStr" => { let t = all_t(body); if t.is_empty() { None } else { Some(V::S(Rc::from(t.as_str()))) } }
                "s" => text_between(body, "<v", "</v>").and_then(|v| v.trim().parse::<usize>().ok()).map(|k| V::S(sst[k].clone())),
                "str" => text_between(body, "<v", "</v>").map(|v| V::S(Rc::from(unescape(v).as_str()))),
                "b" => text_between(body, "<v", "</v>").map(|v| V::B(v.trim() == "1")),
                "e" => text_between(body, "<v", "</v>").map(|v| V::E(match v { "#DIV/0!" => Er::Div0, "#N/A" => Er::NA, "#NUM!" => Er::Num, "#REF!" => Er::Ref, "#NAME?" => Er::Name, "#NULL!" => Er::Null, _ => Er::Value })),
                _ => text_between(body, "<v", "</v>").and_then(|v| v.trim().parse::<f64>().ok()).map(V::N),
            };
            if let Some(v) = v { out.push((row, col, Item::V(v))); }
        }
    }
    out
}

static FUNCS: &[&str] = &["ABS","AND","AVERAGE","AVERAGEIF","AVERAGEIFS","CHOOSE","CONCAT","CONCATENATE","COUNT","COUNTA","COUNTBLANK","COUNTIF","COUNTIFS","DATE","DATEDIF","DAY","DAYS","EDATE","EOMONTH","EXACT","EXP","FIND","HLOOKUP","IF","IFERROR","IFNA","IFS","INDEX","INT","ISBLANK","ISERROR","ISLOGICAL","ISNA","ISNUMBER","ISTEXT","LARGE","LEFT","LEN","LN","LOG","LOG10","LOWER","MATCH","MAX","MEDIAN","MID","MIN","MOD","MONTH","N","NOT","OR","PI","POWER","PRODUCT","PROPER","REPLACE","REPT","RIGHT","ROUND","ROUNDDOWN","ROUNDUP","SEARCH","SIGN","SMALL","SQRT","STDEV","SUBSTITUTE","SUM","SUMIF","SUMIFS","SUMPRODUCT","SUMSQ","SWITCH","T","TEXT","TEXTJOIN","TRIM","TRUNC","UPPER","VALUE","VAR","VLOOKUP","WEEKDAY","XLOOKUP","XOR","YEAR","TRUE","FALSE","NA"];
