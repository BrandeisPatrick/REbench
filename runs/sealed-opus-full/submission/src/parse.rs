use crate::io::col_to_num;

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Str(Box<str>),
    Bool(bool),
    Err(u8),
    Ref(u16, u32, u32),
    Range(u16, u32, u32, u32, u32),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(u8, Box<Expr>, Box<Expr>),
    Func(&'static str, Vec<Expr>),
    Missing,
}

// binary ops: + - * / ^ & = <> < > <= >=
pub const ADD: u8 = 0; pub const SUB: u8 = 1; pub const MUL: u8 = 2; pub const DIV: u8 = 3; pub const POW: u8 = 4;
pub const CAT: u8 = 5; pub const EQ: u8 = 6; pub const NE: u8 = 7; pub const LT: u8 = 8; pub const GT: u8 = 9; pub const LE: u8 = 10; pub const GE: u8 = 11;

pub struct P<'a> { s: &'a [u8], i: usize, sheet: u16, names: &'a [String] }

pub fn err_code(s: &str) -> u8 {
    match s { "#DIV/0!" => 1, "#VALUE!" => 2, "#N/A" => 3, "#NUM!" => 4, "#REF!" => 5, "#NAME?" => 6, "#NULL!" => 7, _ => 2 }
}

pub fn parse(f: &str, sheet: u16, names: &[String]) -> Expr {
    let f = f.strip_prefix('=').unwrap_or(f);
    let mut p = P { s: f.as_bytes(), i: 0, sheet, names };
    let e = p.cmp();
    p.ws();
    if p.i < p.s.len() { return Expr::Err(6); }
    e
}

impl<'a> P<'a> {
    fn ws(&mut self) { while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b'\n' || self.s[self.i] == b'\r') { self.i += 1; } }
    fn peek(&mut self) -> u8 { self.ws(); if self.i < self.s.len() { self.s[self.i] } else { 0 } }
    fn cmp(&mut self) -> Expr {
        let mut l = self.cat();
        loop {
            let c = self.peek();
            let n = if self.i + 1 < self.s.len() { self.s[self.i + 1] } else { 0 };
            let op = match (c, n) {
                (b'<', b'>') => { self.i += 2; NE }
                (b'<', b'=') => { self.i += 2; LE }
                (b'>', b'=') => { self.i += 2; GE }
                (b'<', _) => { self.i += 1; LT }
                (b'>', _) => { self.i += 1; GT }
                (b'=', _) => { self.i += 1; EQ }
                _ => return l,
            };
            let r = self.cat();
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn cat(&mut self) -> Expr {
        let mut l = self.add();
        while self.peek() == b'&' { self.i += 1; let r = self.add(); l = Expr::Bin(CAT, Box::new(l), Box::new(r)); }
        l
    }
    fn add(&mut self) -> Expr {
        let mut l = self.mul();
        loop {
            let op = match self.peek() { b'+' => ADD, b'-' => SUB, _ => return l };
            self.i += 1;
            let r = self.mul();
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn mul(&mut self) -> Expr {
        let mut l = self.pow();
        loop {
            let op = match self.peek() { b'*' => MUL, b'/' => DIV, _ => return l };
            self.i += 1;
            let r = self.pow();
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
    }
    fn pow(&mut self) -> Expr {
        let mut l = self.unary();
        while self.peek() == b'^' { self.i += 1; let r = self.unary(); l = Expr::Bin(POW, Box::new(l), Box::new(r)); }
        l
    }
    fn unary(&mut self) -> Expr {
        match self.peek() {
            b'-' => { self.i += 1; let e = self.unary(); Expr::Neg(Box::new(e)) }
            b'+' => { self.i += 1; self.unary() }
            _ => self.postfix(),
        }
    }
    fn postfix(&mut self) -> Expr {
        let mut e = self.primary();
        while self.peek() == b'%' { self.i += 1; e = Expr::Pct(Box::new(e)); }
        e
    }
    fn args(&mut self) -> Vec<Expr> {
        let mut v = Vec::new();
        if self.peek() == b')' { self.i += 1; return v; }
        loop {
            let c = self.peek();
            if c == b',' || c == b')' { v.push(Expr::Missing); } else { v.push(self.cmp()); }
            match self.peek() {
                b',' => { self.i += 1; }
                b')' => { self.i += 1; return v; }
                _ => { return v; }
            }
        }
    }
    fn try_cellref(&mut self) -> Option<(u32, u32)> {
        let s = self.s; let mut j = self.i;
        if j < s.len() && s[j] == b'$' { j += 1; }
        let (c, n) = col_to_num(&s[j..]);
        if n == 0 || n > 3 { return None; }
        j += n;
        if j < s.len() && s[j] == b'$' { j += 1; }
        let st = j;
        while j < s.len() && s[j].is_ascii_digit() { j += 1; }
        if j == st { return None; }
        if j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_' || s[j] == b'(' || s[j] == b'.') { return None; }
        let r: u32 = std::str::from_utf8(&s[st..j]).unwrap().parse().ok()?;
        self.i = j;
        Some((r - 1, c - 1))
    }
    fn refexpr(&mut self, sheet: u16) -> Option<Expr> {
        let (r1, c1) = self.try_cellref()?;
        if self.i < self.s.len() && self.s[self.i] == b':' {
            let save = self.i;
            self.i += 1;
            if let Some((r2, c2)) = self.try_cellref() {
                return Some(Expr::Range(sheet, r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)));
            }
            self.i = save;
        }
        Some(Expr::Ref(sheet, r1, c1))
    }
    fn sheet_idx(&self, n: &str) -> u16 {
        self.names.iter().position(|x| x.eq_ignore_ascii_case(n)).map(|x| x as u16).unwrap_or(u16::MAX)
    }
    fn primary(&mut self) -> Expr {
        let c = self.peek();
        let s = self.s;
        if c == b'(' {
            self.i += 1;
            let e = self.cmp();
            if self.peek() == b')' { self.i += 1; }
            return e;
        }
        if c == b'"' {
            self.i += 1;
            let mut o = Vec::new();
            while self.i < s.len() {
                if s[self.i] == b'"' {
                    if self.i + 1 < s.len() && s[self.i + 1] == b'"' { o.push(b'"'); self.i += 2; continue; }
                    self.i += 1; break;
                }
                o.push(s[self.i]); self.i += 1;
            }
            return Expr::Str(String::from_utf8(o).unwrap().into_boxed_str());
        }
        if c.is_ascii_digit() || c == b'.' {
            let st = self.i;
            while self.i < s.len() && (s[self.i].is_ascii_digit() || s[self.i] == b'.') { self.i += 1; }
            if self.i < s.len() && (s[self.i] == b'e' || s[self.i] == b'E') {
                let save = self.i;
                self.i += 1;
                if self.i < s.len() && (s[self.i] == b'+' || s[self.i] == b'-') { self.i += 1; }
                let ds = self.i;
                while self.i < s.len() && s[self.i].is_ascii_digit() { self.i += 1; }
                if ds == self.i { self.i = save; }
            }
            return Expr::Num(std::str::from_utf8(&s[st..self.i]).unwrap().parse().unwrap_or(0.0));
        }
        if c == b'#' {
            for e in ["#DIV/0!", "#VALUE!", "#N/A", "#NUM!", "#REF!", "#NAME?", "#NULL!"] {
                if s[self.i..].starts_with(e.as_bytes()) { self.i += e.len(); return Expr::Err(err_code(e)); }
            }
            self.i += 1; return Expr::Err(6);
        }
        if c == b'\'' {
            self.i += 1;
            let mut o = Vec::new();
            while self.i < s.len() {
                if s[self.i] == b'\'' { if self.i + 1 < s.len() && s[self.i + 1] == b'\'' { o.push(b'\''); self.i += 2; continue; } self.i += 1; break; }
                o.push(s[self.i]); self.i += 1;
            }
            if self.i < s.len() && s[self.i] == b'!' { self.i += 1; }
            let sh = self.sheet_idx(&String::from_utf8_lossy(&o));
            return self.refexpr(sh).unwrap_or(Expr::Err(5));
        }
        // identifier / ref
        let st = self.i;
        if c == b'$' { if let Some(e) = self.refexpr(self.sheet) { return e; } }
        while self.i < s.len() && (s[self.i].is_ascii_alphanumeric() || s[self.i] == b'_' || s[self.i] == b'.') { self.i += 1; }
        let id = std::str::from_utf8(&s[st..self.i]).unwrap().to_string();
        if self.i < s.len() && s[self.i] == b'!' {
            self.i += 1;
            let sh = self.sheet_idx(&id);
            return self.refexpr(sh).unwrap_or(Expr::Err(5));
        }
        if self.peek() == b'(' {
            self.i += 1;
            let a = self.args();
            let mut name = id.to_ascii_uppercase();
            for p in ["_XLFN._XLWS.", "_XLFN.", "_XLWS."] { if let Some(r) = name.strip_prefix(p) { name = r.to_string(); break; } }
            let nm: &'static str = FNAMES.iter().find(|x| **x == name).copied().unwrap_or("#NAME");
            return Expr::Func(nm, a);
        }
        let up = id.to_ascii_uppercase();
        if up == "TRUE" { return Expr::Bool(true); }
        if up == "FALSE" { return Expr::Bool(false); }
        self.i = st;
        if let Some(e) = self.refexpr(self.sheet) { return e; }
        self.i = st + id.len().max(1);
        Expr::Err(6)
    }
}

pub const FNAMES: &[&str] = &["ABS","AND","AVERAGE","AVERAGEIF","AVERAGEIFS","CHOOSE","CONCAT","CONCATENATE","COUNT","COUNTA",
"COUNTBLANK","COUNTIF","COUNTIFS","DATE","DATEDIF","DAY","DAYS","EDATE","EOMONTH","EXACT","EXP","FIND","HLOOKUP","IF","IFERROR",
"IFNA","IFS","INDEX","INT","ISBLANK","ISERROR","ISLOGICAL","ISNA","ISNUMBER","ISTEXT","LARGE","LEFT","LEN","LN","LOG","LOG10","LOWER",
"MATCH","MAX","MEDIAN","MID","MIN","MOD","MONTH","N","NOT","OR","PI","POWER","PRODUCT","PROPER","REPLACE","REPT","RIGHT","ROUND",
"ROUNDDOWN","ROUNDUP","SEARCH","SIGN","SMALL","SQRT","STDEV","SUBSTITUTE","SUM","SUMIF","SUMIFS","SUMPRODUCT","SUMSQ","SWITCH",
"T","TEXT","TEXTJOIN","TRIM","TRUNC","UPPER","VALUE","VAR","VLOOKUP","WEEKDAY","XLOOKUP","XOR","YEAR","TRUE","FALSE","NA"];
