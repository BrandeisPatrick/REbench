use crate::{Er, parse_a1};
use std::rc::Rc;

pub enum Node {
    Num(f64), Str(Rc<str>), Bool(bool), Err(Er),
    Ref(usize, usize, usize),
    Range(usize, usize, usize, usize, usize),
    Neg(Box<Node>), Pct(Box<Node>),
    Bin(u8, Box<Node>, Box<Node>),
    Func(String, Vec<Node>),
}
// ops: + - * / ^ & = <> < > <= >=
pub const OP_ADD: u8 = 0; pub const OP_SUB: u8 = 1; pub const OP_MUL: u8 = 2; pub const OP_DIV: u8 = 3; pub const OP_POW: u8 = 4;
pub const OP_CAT: u8 = 5; pub const OP_EQ: u8 = 6; pub const OP_NE: u8 = 7; pub const OP_LT: u8 = 8; pub const OP_GT: u8 = 9; pub const OP_LE: u8 = 10; pub const OP_GE: u8 = 11;

struct P<'a> { b: &'a [u8], i: usize, names: &'a [String], fail: bool }

pub fn parse_formula(f: &str, names: &[String]) -> Node {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse_formula_inner(f, names))).unwrap_or(Node::Err(Er::Name))
}
fn parse_formula_inner(f: &str, names: &[String]) -> Node {
    let f = f.trim_start_matches('=');
    let mut p = P { b: f.as_bytes(), i: 0, names, fail: false };
    let n = p.cmp();
    p.ws();
    if p.fail || p.i < p.b.len() { return Node::Err(Er::Name); }
    n
}

impl<'a> P<'a> {
    fn ws(&mut self) { while self.i < self.b.len() && (self.b[self.i] == b' ' || self.b[self.i] == b'\t') { self.i += 1; } }
    fn peek(&mut self) -> u8 { self.ws(); if self.i < self.b.len() { self.b[self.i] } else { 0 } }
    fn cmp(&mut self) -> Node {
        let mut l = self.cat();
        loop {
            let c = self.peek();
            let c2 = if self.i + 1 < self.b.len() { self.b[self.i + 1] } else { 0 };
            let (op, len) = match (c, c2) {
                (b'=', _) => (OP_EQ, 1), (b'<', b'>') => (OP_NE, 2), (b'<', b'=') => (OP_LE, 2), (b'>', b'=') => (OP_GE, 2),
                (b'<', _) => (OP_LT, 1), (b'>', _) => (OP_GT, 1), _ => break,
            };
            self.i += len;
            let r = self.cat();
            l = Node::Bin(op, Box::new(l), Box::new(r));
        }
        l
    }
    fn cat(&mut self) -> Node {
        let mut l = self.add();
        while self.peek() == b'&' { self.i += 1; let r = self.add(); l = Node::Bin(OP_CAT, Box::new(l), Box::new(r)); }
        l
    }
    fn add(&mut self) -> Node {
        let mut l = self.mul();
        loop {
            let c = self.peek();
            if c == b'+' || c == b'-' { self.i += 1; let r = self.mul(); l = Node::Bin(if c == b'+' { OP_ADD } else { OP_SUB }, Box::new(l), Box::new(r)); } else { break; }
        }
        l
    }
    fn mul(&mut self) -> Node {
        let mut l = self.pow();
        loop {
            let c = self.peek();
            if c == b'*' || c == b'/' { self.i += 1; let r = self.pow(); l = Node::Bin(if c == b'*' { OP_MUL } else { OP_DIV }, Box::new(l), Box::new(r)); } else { break; }
        }
        l
    }
    fn pow(&mut self) -> Node {
        let mut l = self.unary();
        while self.peek() == b'^' { self.i += 1; let r = self.unary(); l = Node::Bin(OP_POW, Box::new(l), Box::new(r)); }
        l
    }
    fn unary(&mut self) -> Node {
        let c = self.peek();
        if c == b'-' { self.i += 1; let n = self.unary(); return Node::Neg(Box::new(n)); }
        if c == b'+' { self.i += 1; return self.unary(); }
        let mut n = self.primary();
        while self.peek() == b'%' { self.i += 1; n = Node::Pct(Box::new(n)); }
        n
    }
    fn primary(&mut self) -> Node {
        let c = self.peek();
        if c == b'(' {
            self.i += 1;
            let n = self.cmp();
            if self.peek() != b')' { self.fail = true; } else { self.i += 1; }
            return n;
        }
        if c == b'"' {
            self.i += 1;
            let mut s = Vec::new();
            loop {
                if self.i >= self.b.len() { self.fail = true; break; }
                if self.b[self.i] == b'"' {
                    if self.i + 1 < self.b.len() && self.b[self.i + 1] == b'"' { s.push(b'"'); self.i += 2; continue; }
                    self.i += 1; break;
                }
                s.push(self.b[self.i]); self.i += 1;
            }
            return Node::Str(Rc::from(String::from_utf8_lossy(&s).as_ref()));
        }
        if c == b'#' {
            let rest = &self.b[self.i..];
            for (s, e) in [("#DIV/0!", Er::Div0), ("#VALUE!", Er::Value), ("#N/A", Er::Na), ("#NUM!", Er::Num), ("#REF!", Er::Ref), ("#NAME?", Er::Name), ("#NULL!", Er::Null)] {
                if rest.starts_with(s.as_bytes()) { self.i += s.len(); return Node::Err(e); }
            }
            self.fail = true; return Node::Err(Er::Name);
        }
        if c.is_ascii_digit() || c == b'.' {
            let st = self.i;
            while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'.') { self.i += 1; }
            if self.i < self.b.len() && (self.b[self.i] == b'e' || self.b[self.i] == b'E') {
                let save = self.i;
                self.i += 1;
                if self.i < self.b.len() && (self.b[self.i] == b'+' || self.b[self.i] == b'-') { self.i += 1; }
                if self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                    while self.i < self.b.len() && self.b[self.i].is_ascii_digit() { self.i += 1; }
                } else { self.i = save; }
            }
            let s = std::str::from_utf8(&self.b[st..self.i]).unwrap();
            return match s.parse::<f64>() { Ok(v) => Node::Num(v), Err(_) => { self.fail = true; Node::Num(0.0) } };
        }
        // identifier / ref
        let mut sheet: Option<usize> = None;
        let mut name: String;
        if c == b'\'' {
            self.i += 1;
            let mut s = Vec::new();
            while self.i < self.b.len() {
                if self.b[self.i] == b'\'' { if self.i + 1 < self.b.len() && self.b[self.i + 1] == b'\'' { s.push(b'\''); self.i += 2; continue; } self.i += 1; break; }
                s.push(self.b[self.i]); self.i += 1;
            }
            let sn = String::from_utf8_lossy(&s).into_owned();
            if self.i < self.b.len() && self.b[self.i] == b'!' { self.i += 1; } else { self.fail = true; }
            sheet = self.names.iter().position(|n| n.eq_ignore_ascii_case(&sn));
            if sheet.is_none() { self.fail = false; self.skip_ref(); return Node::Err(Er::Name); }
            name = self.ident();
        } else {
            name = self.ident();
            if name.is_empty() { self.fail = true; self.i += 1; return Node::Err(Er::Name); }
            if self.i < self.b.len() && self.b[self.i] == b'!' {
                self.i += 1;
                sheet = self.names.iter().position(|n| n.eq_ignore_ascii_case(&name));
                if sheet.is_none() { self.skip_ref(); return Node::Err(Er::Name); }
                name = self.ident();
            }
        }
        let s = sheet.unwrap_or(usize::MAX);
        if sheet.is_none() && self.i < self.b.len() && self.b[self.i] == b'(' {
            self.i += 1;
            let mut args = Vec::new();
            if self.peek() == b')' { self.i += 1; } else {
                loop {
                    // empty arg
                    let c = self.peek();
                    if c == b',' { args.push(Node::Err(Er::Null)); self.i += 1; continue; }
                    if c == b')' { self.i += 1; break; }
                    args.push(self.cmp());
                    let c = self.peek();
                    if c == b',' { self.i += 1; if self.peek() == b')' { args.push(Node::Err(Er::Null)); self.i += 1; break; } }
                    else if c == b')' { self.i += 1; break; }
                    else { self.fail = true; break; }
                }
            }
            let mut up = name.to_ascii_uppercase();
            if let Some(r) = up.strip_prefix("_XLFN.") { up = r.to_string(); }
            if let Some(r) = up.strip_prefix("_XLWS.") { up = r.to_string(); }
            return Node::Func(up, args);
        }
        let up = name.to_ascii_uppercase();
        if sheet.is_none() {
            if up == "TRUE" { return Node::Bool(true); }
            if up == "FALSE" { return Node::Bool(false); }
        }
        if let Some((r, cc)) = parse_a1(&name) {
            let sidx = if sheet.is_none() { usize::MAX - 1 } else { s };
            if self.peek_raw() == b':' {
                let save = self.i;
                self.i += 1;
                let n2 = self.ident();
                if let Some((r2, c2)) = parse_a1(&n2) {
                    return Node::Range(sidx, r.min(r2), cc.min(c2), r.max(r2), cc.max(c2));
                }
                self.i = save;
            }
            return Node::Ref(sidx, r, cc);
        }
        // whole column / row references
        if self.peek_raw() == b':' && !name.is_empty() {
            let save = self.i;
            self.i += 1;
            let n2 = self.ident();
            let sidx = if sheet.is_none() { usize::MAX - 1 } else { s };
            if name.bytes().all(|c| c.is_ascii_alphabetic()) && n2.bytes().all(|c| c.is_ascii_alphabetic()) && !n2.is_empty() {
                let c1 = crate::col_to_idx(name.as_bytes()); let c2 = crate::col_to_idx(n2.as_bytes());
                return Node::Range(sidx, 0, c1.min(c2), 1048575, c1.max(c2));
            }
            if name.bytes().all(|c| c.is_ascii_digit()) && n2.bytes().all(|c| c.is_ascii_digit()) && !n2.is_empty() {
                let r1: usize = name.parse().unwrap_or(1); let r2: usize = n2.parse().unwrap_or(1);
                return Node::Range(sidx, r1.min(r2).saturating_sub(1), 0, r1.max(r2).saturating_sub(1), 16383);
            }
            self.i = save;
        }
        Node::Err(Er::Name)
    }
    fn peek_raw(&self) -> u8 { if self.i < self.b.len() { self.b[self.i] } else { 0 } }
    fn skip_ref(&mut self) {
        let _ = self.ident();
        if self.peek_raw() == b':' { self.i += 1; let _ = self.ident(); }
    }
    fn ident(&mut self) -> String {
        let st = self.i;
        while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_' || self.b[self.i] == b'.' || self.b[self.i] == b'$' || self.b[self.i] >= 128) { self.i += 1; }
        String::from_utf8_lossy(&self.b[st..self.i]).replace('$', "")
    }
}
