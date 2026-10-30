use crate::*;
use crate::parse::*;
use std::cell::UnsafeCell;

pub struct Formula { pub sheet: usize, pub row: usize, pub col: usize, pub ast: Node }
#[derive(Clone, Copy)]
pub struct Cx { pub sheet: usize }
pub enum Arg { V(Value), R(usize, usize, usize, usize, usize) }

pub struct Engine {
    pub sheets: UnsafeCell<Vec<Sheet>>,
    pub formulas: Vec<Formula>,
    pub empty: Value,
    pub cyc: Value,
    pub ccache: std::cell::RefCell<std::collections::HashMap<(usize, usize, usize, usize, usize, String), Rc<Vec<u64>>>>,
}

pub const CUR: usize = usize::MAX - 1;

impl Engine {
    pub fn new(sheets: Vec<Sheet>, formulas: Vec<Formula>) -> Engine {
        Engine { sheets: UnsafeCell::new(sheets), formulas, empty: Value::Empty, cyc: Value::Err(Er::Value), ccache: Default::default() }
    }
    #[inline]
    pub fn sh(&self, s: usize) -> &Sheet { unsafe { &(&*self.sheets.get())[s] } }
    pub fn dims(&self, s: usize) -> (usize, usize) { let h = self.sh(s); (h.rows, h.cols) }
    pub fn nsheets(&self) -> usize { unsafe { (&*self.sheets.get()).len() } }

    pub fn formula_value(&self, fi: usize) -> Value {
        let f = &self.formulas[fi];
        self.cell(f.sheet, f.row, f.col).clone()
    }

    #[inline]
    pub fn cell(&self, s: usize, r: usize, c: usize) -> &Value {
        unsafe {
            let shs = &mut *self.sheets.get();
            if s >= shs.len() { return &self.empty; }
            let sh = shs.as_mut_ptr().add(s);
            if r >= (*sh).rows || c >= (*sh).cols { return &self.empty; }
            let idx = r * (*sh).cols + c;
            let st = *(*sh).state.as_ptr().add(idx);
            match st {
                0 | 3 => &*(*sh).cells.as_ptr().add(idx),
                2 => &self.cyc,
                _ => {
                    *(*sh).state.as_mut_ptr().add(idx) = 2;
                    let fi = (*(*sh).fid.as_ptr().add(idx) - 1) as usize;
                    let f = &self.formulas[fi];
                    let v = self.eval_top(&f.ast, Cx { sheet: f.sheet });
                    *(*sh).cells.as_mut_ptr().add(idx) = v;
                    *(*sh).state.as_mut_ptr().add(idx) = 3;
                    &*(*sh).cells.as_ptr().add(idx)
                }
            }
        }
    }

    fn eval_top(&self, n: &Node, cx: Cx) -> Value {
        match self.eval(n, cx) { Value::Empty => Value::EmptyF, v => v }
    }

    pub fn eval(&self, n: &Node, cx: Cx) -> Value {
        match n {
            Node::Num(x) => Value::Num(*x),
            Node::Str(s) => Value::Str(s.clone()),
            Node::Bool(b) => Value::Bool(*b),
            Node::Err(Er::Null) => Value::Empty,
            Node::Err(e) => Value::Err(*e),
            Node::Ref(s, r, c) => { let s = if *s == CUR { cx.sheet } else { *s }; self.cell(s, *r, *c).clone() }
            Node::Range(s, r1, c1, r2, c2) => {
                if r1 == r2 && c1 == c2 { let s = if *s == CUR { cx.sheet } else { *s }; self.cell(s, *r1, *c1).clone() } else { Value::Err(Er::Value) }
            }
            Node::Neg(a) => match { let (v, l) = self.eval_l(a, cx, false); if let (Value::Bool(false), false) = (&v, l) { return Value::Bool(false); } v } {
                Value::Err(e) => Value::Err(e),
                v => match self.to_num(&v) { Ok(x) => Value::Num(-x), Err(e) => Value::Err(e) },
            },
            Node::Pct(a) => match self.eval(a, cx) {
                Value::Err(e) => Value::Err(e),
                v => match self.to_num(&v) { Ok(x) => Value::Num(x / 100.0), Err(e) => Value::Err(e) },
            },
            Node::Bin(op, a, b) => {
                let (x, xl) = self.eval_l(a, cx, false);
                let mut xe = false;
                if let Value::Err(_) = x { if !xl { xe = true; } }
                let y = self.eval(b, cx);
                if let Value::Err(e) = y { if !xe || crate::funcs::is_jump_top(b) { return Value::Err(e); } }
                if xe { return x; }
                self.binop(*op, &x, &y)
            }
            Node::Func(..) => match self.arg(n, cx) {
                Arg::V(v) => v,
                Arg::R(s, r1, c1, r2, c2) => if r1 == r2 && c1 == c2 { self.cell(s, r1, c1).clone() } else { Value::Err(Er::Value) },
            },
        }
    }

    pub fn eval_l(&self, n: &Node, cx: Cx, ll: bool) -> (Value, bool) {
        match n {
            Node::Ref(..) | Node::Range(..) => (self.eval(n, cx), true),
            Node::Func(name, args) => match self.call(name, args, cx) {
                Arg::V(v) => {
                    let mut lazy = ll && matches!(v, Value::Err(Er::Na)) && matches!(name.as_str(), "VLOOKUP" | "HLOOKUP" | "XLOOKUP" | "MATCH");
                    (v, lazy)
                }
                Arg::R(s, r1, c1, r2, c2) => (if r1 == r2 && c1 == c2 { self.cell(s, r1, c1).clone() } else { Value::Err(Er::Value) }, true),
            },
            _ => (self.eval(n, cx), false),
        }
    }
    pub fn arg(&self, n: &Node, cx: Cx) -> Arg {
        match n {
            Node::Ref(s, r, c) => { let s = if *s == CUR { cx.sheet } else { *s }; Arg::R(s, *r, *c, *r, *c) }
            Node::Range(s, r1, c1, r2, c2) => { let s = if *s == CUR { cx.sheet } else { *s }; Arg::R(s, *r1, *c1, *r2, *c2) }
            Node::Func(name, args) => self.call(name, args, cx),
            _ => Arg::V(self.eval(n, cx)),
        }
    }

    pub fn binop(&self, op: u8, x: &Value, y: &Value) -> Value {
        match op {
            OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_POW => {
                let b = match self.to_num(y) { Ok(v) => v, Err(e) => return Value::Err(e) };
                let a = match self.to_num(x) { Ok(v) => v, Err(e) => return Value::Err(e) };
                let r = match op {
                    OP_ADD => approx_add(a, b),
                    OP_SUB => approx_add(a, -b),
                    OP_MUL => a * b,
                    OP_DIV => if b == 0.0 { return Value::Err(Er::Div0) } else { a / b },
                    _ => match pow(a, b) { Some(v) => v, None => return Value::Err(Er::Num) },
                };
                if r.is_finite() { Value::Num(r) } else { Value::Err(Er::Num) }
            }
            OP_CAT => {
                if let Value::Err(e) = x { return Value::Err(*e); }
                let a = self.to_str(x); let b = self.to_str(y);
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(&a); s.push_str(&b);
                Value::Str(Rc::from(s))
            }
            _ => {
                if let Value::Err(e) = x { return Value::Err(*e); }
                let o = self.compare(x, y);
                Value::Bool(match op {
                    OP_EQ => o == Ordering::Equal, OP_NE => o != Ordering::Equal, OP_LT => o == Ordering::Less,
                    OP_GT => o == Ordering::Greater, OP_LE => o != Ordering::Greater, _ => o != Ordering::Less,
                })
            }
        }
    }

    pub fn compare(&self, x: &Value, y: &Value) -> Ordering {
        let x = if let Value::EmptyF = x { &Value::Empty } else { x };
        let y = if let Value::EmptyF = y { &Value::Empty } else { y };
        fn rank(v: &Value) -> u8 { match v { Value::Num(_) | Value::Bool(_) => 0, Value::Str(_) => 1, _ => 2 } }
        match (x, y) {
            (Value::Empty, Value::Empty) => Ordering::Equal,
            (Value::Empty, Value::Str(s)) => if s.is_empty() { Ordering::Equal } else { Ordering::Less },
            (Value::Str(s), Value::Empty) => if s.is_empty() { Ordering::Equal } else { Ordering::Greater },
            (Value::Empty, _) => self.compare(&Value::Num(0.0), y),
            (_, Value::Empty) => self.compare(x, &Value::Num(0.0)),
            _ => {
                let (rx, ry) = (rank(x), rank(y));
                if rx != ry { return rx.cmp(&ry); }
                if rx == 0 { cmp_ord(num_of(x), num_of(y)) } else { str_cmp(self.str_of(x), self.str_of(y)) }
            }
        }
    }
    fn str_of<'a>(&self, v: &'a Value) -> &'a str { if let Value::Str(s) = v { s } else { "" } }

    pub fn to_num(&self, v: &Value) -> Result<f64, Er> {
        match v {
            Value::Num(x) => Ok(*x),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Empty | Value::EmptyF => Ok(0.0),
            Value::Err(e) => Err(*e),
            Value::Str(s) => crate::fmt::parse_num_str(s).ok_or(Er::Value),
        }
    }
    pub fn to_str(&self, v: &Value) -> Rc<str> {
        match v {
            Value::Str(s) => s.clone(),
            Value::Num(x) => Rc::from(crate::fmt::num_to_str(*x)),
            Value::Bool(b) => Rc::from(if *b { "1" } else { "0" }),
            Value::EmptyF => Rc::from("0"),
            _ => Rc::from(""),
        }
    }
    pub fn to_bool(&self, v: &Value) -> Result<bool, Er> {
        match v {
            Value::Bool(b) => Ok(*b),
            Value::Num(x) => Ok(*x != 0.0),
            Value::Empty | Value::EmptyF => Ok(false),
            Value::Err(e) => Err(*e),
            Value::Str(s) => {
                match crate::fmt::parse_num_str(s) { Some(x) => Ok(x != 0.0), None => {
                    let u = s.to_ascii_uppercase();
                    if u == "TRUE" { Ok(true) } else if u == "FALSE" { Ok(false) } else { Err(Er::Value) }
                } }
            }
        }
    }
}

pub fn num_of(v: &Value) -> f64 { match v { Value::Num(x) => *x, Value::Bool(b) => if *b { 1.0 } else { 0.0 }, _ => 0.0 } }

pub fn approx_add(a: f64, b: f64) -> f64 {
    let r = a + b;
    if (a < 0.0) != (b < 0.0) && a != 0.0 && b != 0.0 {
        if r.abs() < a.abs().max(b.abs()) * 1.0 / 281474976710656.0 * 1.0 { return 0.0; }
        if approx_eq(a, -b) { return 0.0; }
    }
    r
}
pub fn pow(a: f64, b: f64) -> Option<f64> {
    let r = if a < 0.0 && b.fract() != 0.0 && (1.0 / b).fract() == 0.0 && (1.0 / b) as i64 % 2 != 0 { -((-a).powf(b)) } else { a.powf(b) };
    if r.is_finite() && (r == 0.0 && a != 0.0 && b < 0.0 || (r != 0.0 && r.abs() < f64::MIN_POSITIVE)) { return None; }
    if r.is_finite() { Some(r) } else { None }
}
fn coll_key(c: char) -> (u8, u32) {
    if c.is_ascii() {
        let b = c as u8;
        if b.is_ascii_alphabetic() { return (3, b.to_ascii_lowercase() as u32); }
        if b.is_ascii_digit() { return (2, b as u32); }
        let order = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
        if let Some(p) = order.find(b as char) { return (1, p as u32); }
        return (1, 100 + b as u32);
    }
    if c.is_alphabetic() { let l = c.to_lowercase().next().unwrap_or(c); return (3, fold_accent(l) as u32 + 1000 * if l.is_ascii() { 0 } else if fold_accent(l).is_ascii() { 0 } else { 1 }); }
    (1, 200 + c as u32)
}
fn fold_accent(c: char) -> char {
    match c {
        'à'..='å' => 'a', 'ç' => 'c', 'è'..='ë' => 'e', 'ì'..='ï' => 'i', 'ñ' => 'n', 'ò'..='ö' | 'ø' => 'o', 'ù'..='ü' => 'u', 'ý' | 'ÿ' => 'y', c => c,
    }
}
pub fn str_cmp(a: &str, b: &str) -> Ordering {
    let mut sec = Ordering::Equal;
    let mut ai = a.chars(); let mut bi = b.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return sec,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => { if x != y {
                let (kx, ky) = (coll_key(x), coll_key(y)); if kx != ky { return kx.cmp(&ky); }
                let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
                if lx != ly && sec == Ordering::Equal { sec = (lx.is_ascii() as u8 ^ 1).cmp(&(ly.is_ascii() as u8 ^ 1)); }
            } }
        }
    }
}

pub fn lazy_node(n: &Node) -> bool {
    match n {
        Node::Ref(..) | Node::Range(..) => true,
        Node::Func(name, _) => matches!(name.as_str(), "IF" | "IFS" | "CHOOSE" | "SWITCH" | "INDEX" | "VLOOKUP" | "HLOOKUP" | "XLOOKUP"),
        _ => false,
    }
}
