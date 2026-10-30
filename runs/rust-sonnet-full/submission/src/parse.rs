use crate::eval::Er;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op { Add, Sub, Mul, Div, Pow, Cat, Eq, Ne, Lt, Gt, Le, Ge }

#[derive(Debug)]
pub enum Node {
    Num(f64),
    Str(Box<str>),
    Bool(bool),
    Err(Er),
    Ref(usize, u32, u32),
    Range(usize, u32, u32, u32, u32),
    Neg(Box<Node>),
    Pct(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Func(String, Vec<Node>),
}

pub struct P<'a> {
    s: &'a [u8],
    i: usize,
    sheet: usize,
    names: &'a [String],
}

fn col_from(b: &[u8]) -> u32 {
    let mut c = 0u32;
    for &x in b { c = c * 26 + (x.to_ascii_uppercase() - b'A' + 1) as u32; }
    c - 1
}

impl<'a> P<'a> {
    fn ws(&mut self) { while self.i < self.s.len() && self.s[self.i] == b' ' { self.i += 1; } }
    fn peek(&self) -> u8 { if self.i < self.s.len() { self.s[self.i] } else { 0 } }

    pub fn expr(&mut self) -> Result<Node, ()> {
        let mut l = self.concat()?;
        loop {
            self.ws();
            let op = match (self.peek(), self.s.get(self.i + 1).copied().unwrap_or(0)) {
                (b'<', b'>') => { self.i += 2; Op::Ne }
                (b'<', b'=') => { self.i += 2; Op::Le }
                (b'>', b'=') => { self.i += 2; Op::Ge }
                (b'<', _) => { self.i += 1; Op::Lt }
                (b'>', _) => { self.i += 1; Op::Gt }
                (b'=', _) => { self.i += 1; Op::Eq }
                _ => return Ok(l),
            };
            let r = self.concat()?;
            l = Node::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn concat(&mut self) -> Result<Node, ()> {
        let mut l = self.add()?;
        loop {
            self.ws();
            if self.peek() == b'&' {
                self.i += 1;
                let r = self.add()?;
                l = Node::Bin(Op::Cat, Box::new(l), Box::new(r));
            } else { return Ok(l); }
        }
    }
    fn add(&mut self) -> Result<Node, ()> {
        let mut l = self.mul()?;
        loop {
            self.ws();
            let op = match self.peek() { b'+' => Op::Add, b'-' => Op::Sub, _ => return Ok(l) };
            self.i += 1;
            let r = self.mul()?;
            l = Node::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn mul(&mut self) -> Result<Node, ()> {
        let mut l = self.pow()?;
        loop {
            self.ws();
            let op = match self.peek() { b'*' => Op::Mul, b'/' => Op::Div, _ => return Ok(l) };
            self.i += 1;
            let r = self.pow()?;
            l = Node::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn pow(&mut self) -> Result<Node, ()> {
        let mut l = self.unary()?;
        loop {
            self.ws();
            if self.peek() == b'^' {
                self.i += 1;
                let r = self.unary()?;
                l = Node::Bin(Op::Pow, Box::new(l), Box::new(r));
            } else { return Ok(l); }
        }
    }
    fn unary(&mut self) -> Result<Node, ()> {
        self.ws();
        match self.peek() {
            b'-' => { self.i += 1; let e = self.unary()?; Ok(Node::Neg(Box::new(e))) }
            b'+' => { self.i += 1; self.unary() }
            _ => self.postfix(),
        }
    }
    fn postfix(&mut self) -> Result<Node, ()> {
        let mut e = self.primary()?;
        loop {
            self.ws();
            if self.peek() == b'%' { self.i += 1; e = Node::Pct(Box::new(e)); } else { return Ok(e); }
        }
    }
    fn sheet_idx(&self, n: &str) -> Option<usize> {
        self.names.iter().position(|x| x.eq_ignore_ascii_case(n))
    }
    // try parse a cell ref at current pos: [$]LETTERS[$]DIGITS ; returns (r,c) and advances
    fn cellref(&mut self) -> Option<(u32, u32)> {
        let st = self.i;
        let mut j = self.i;
        if j < self.s.len() && self.s[j] == b'$' { j += 1; }
        let cs = j;
        while j < self.s.len() && self.s[j].is_ascii_alphabetic() { j += 1; }
        let ce = j;
        if ce == cs || ce - cs > 3 { self.i = st; return None; }
        if j < self.s.len() && self.s[j] == b'$' { j += 1; }
        let rs = j;
        while j < self.s.len() && self.s[j].is_ascii_digit() { j += 1; }
        if j == rs { self.i = st; return None; }
        // must not be followed by ident char or '('
        if j < self.s.len() && (self.s[j].is_ascii_alphanumeric() || self.s[j] == b'_' || self.s[j] == b'(' || self.s[j] == b'.') { self.i = st; return None; }
        let c = col_from(&self.s[cs..ce]);
        let r: u32 = std::str::from_utf8(&self.s[rs..j]).unwrap().parse().ok()?;
        if r == 0 { self.i = st; return None; }
        self.i = j;
        Some((r - 1, c))
    }
    fn refnode(&mut self, sh: usize) -> Result<Node, ()> {
        let a = self.cellref().ok_or(())?;
        if self.peek() == b':' {
            let save = self.i;
            self.i += 1;
            if let Some(b) = self.cellref() {
                return Ok(Node::Range(sh, a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)));
            }
            self.i = save;
        }
        Ok(Node::Ref(sh, a.0, a.1))
    }
    fn primary(&mut self) -> Result<Node, ()> {
        self.ws();
        let c = self.peek();
        if c == b'(' {
            self.i += 1;
            let e = self.expr()?;
            self.ws();
            if self.peek() != b')' { return Err(()); }
            self.i += 1;
            return Ok(e);
        }
        if c == b'"' {
            self.i += 1;
            let mut out = String::new();
            let st = self.i;
            let mut seg = st;
            loop {
                if self.i >= self.s.len() { return Err(()); }
                if self.s[self.i] == b'"' {
                    out.push_str(std::str::from_utf8(&self.s[seg..self.i]).map_err(|_| ())?);
                    if self.s.get(self.i + 1) == Some(&b'"') { out.push('"'); self.i += 2; seg = self.i; }
                    else { self.i += 1; break; }
                } else { self.i += 1; }
            }
            return Ok(Node::Str(out.into_boxed_str()));
        }
        if c.is_ascii_digit() || c == b'.' {
            let st = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') { self.i += 1; }
            if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
                let mut j = self.i + 1;
                if j < self.s.len() && (self.s[j] == b'+' || self.s[j] == b'-') { j += 1; }
                if j < self.s.len() && self.s[j].is_ascii_digit() {
                    while j < self.s.len() && self.s[j].is_ascii_digit() { j += 1; }
                    self.i = j;
                }
            }
            let t = std::str::from_utf8(&self.s[st..self.i]).unwrap();
            return t.parse::<f64>().map(Node::Num).map_err(|_| ());
        }
        if c == b'#' {
            let rest = std::str::from_utf8(&self.s[self.i..]).unwrap_or("");
            for n in ["#DIV/0!", "#VALUE!", "#N/A", "#NUM!", "#REF!", "#NAME?", "#NULL!"] {
                if rest.starts_with(n) { self.i += n.len(); return Ok(Node::Err(Er::from_str(n))); }
            }
            return Err(());
        }
        if c == b'\'' {
            self.i += 1;
            let st = self.i;
            while self.i < self.s.len() && self.s[self.i] != b'\'' { self.i += 1; }
            let name = std::str::from_utf8(&self.s[st..self.i]).unwrap().to_string();
            self.i += 1;
            if self.peek() != b'!' { return Err(()); }
            self.i += 1;
            return match self.sheet_idx(&name) {
                Some(sh) => self.refnode(sh),
                None => { let _ = self.refnode(0); Ok(Node::Err(Er::Name)) }
            };
        }
        if c == b'$' || c.is_ascii_alphabetic() || c == b'_' {
            // identifier?
            let st = self.i;
            let mut j = self.i;
            while j < self.s.len() && (self.s[j].is_ascii_alphanumeric() || self.s[j] == b'_' || self.s[j] == b'.') { j += 1; }
            let id = std::str::from_utf8(&self.s[st..j]).unwrap().to_string();
            if j < self.s.len() && self.s[j] == b'!' {
                self.i = j + 1;
                return match self.sheet_idx(&id) {
                    Some(sh) => self.refnode(sh),
                    None => { let _ = self.refnode(0); Ok(Node::Err(Er::Name)) }
                };
            }
            if j < self.s.len() && self.s[j] == b'(' {
                self.i = j + 1;
                let mut name = id.to_ascii_uppercase();
                if let Some(n) = name.strip_prefix("_XLFN.") { name = n.to_string(); }
                let mut args = Vec::new();
                self.ws();
                if self.peek() == b')' { self.i += 1; return Ok(Node::Func(name, args)); }
                loop {
                    self.ws();
                    if self.peek() == b',' || self.peek() == b')' {
                        args.push(Node::Func("\0MISSING".into(), vec![]));
                    } else {
                        args.push(self.expr()?);
                    }
                    self.ws();
                    match self.peek() {
                        b',' => { self.i += 1; }
                        b')' => { self.i += 1; break; }
                        _ => return Err(()),
                    }
                }
                return Ok(Node::Func(name, args));
            }
            let up = id.to_ascii_uppercase();
            if let Some(r) = self.cellref() { 
                if self.peek() == b':' {
                    self.i = st;
                    return self.refnode(self.sheet);
                }
                let _ = r;
                self.i = st;
                return self.refnode(self.sheet);
            }
            if up == "TRUE" { self.i = j; return Ok(Node::Bool(true)); }
            if up == "FALSE" { self.i = j; return Ok(Node::Bool(false)); }
            self.i = j;
            return Ok(Node::Err(Er::Name));
        }
        Err(())
    }
}

pub fn parse(f: &str, sheet: usize, names: &[String]) -> Node {
    let mut p = P { s: f.as_bytes(), i: 0, sheet, names };
    match p.expr() {
        Ok(n) => { p.ws(); if p.i < p.s.len() { Node::Err(Er::Name) } else { n } }
        Err(_) => Node::Err(Er::Name),
    }
}
