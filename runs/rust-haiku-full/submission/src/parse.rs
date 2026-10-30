use crate::eval::{ErrK, Expr, BinOp, Val};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(ErrK),
    // sheet name, (row,col) 0-based, optional end
    Ref(Option<String>, (u32, u32), Option<(u32, u32)>),
    Ident(String),
    Op(String),
    LP,
    RP,
    Comma,
}

fn col_letters(c: u32) -> String {
    let mut n = c + 1;
    let mut s = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.push((b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s.iter().rev().collect()
}
pub fn cell_name(r: u32, c: u32) -> String {
    format!("{}{}", col_letters(c), r + 1)
}

// parse a cell ref at position i; returns ((row,col), end)
fn parse_cell(c: &[char], i: usize) -> Option<((u32, u32), usize)> {
    let mut j = i;
    if j < c.len() && c[j] == '$' {
        j += 1;
    }
    let cs = j;
    while j < c.len() && c[j].is_ascii_alphabetic() {
        j += 1;
    }
    let letters = &c[cs..j];
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    if j < c.len() && c[j] == '$' {
        j += 1;
    }
    let ds = j;
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    if j == ds {
        return None;
    }
    let mut col: u32 = 0;
    for ch in letters {
        col = col * 26 + (ch.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = c[ds..j].iter().collect::<String>().parse().ok()?;
    if row == 0 || col == 0 || col > 16384 {
        return None;
    }
    Some(((row - 1, col - 1), j))
}

fn parse_ref(c: &[char], i: usize) -> Option<((u32, u32), Option<(u32, u32)>, usize)> {
    let (a, mut j) = parse_cell(c, i)?;
    if j < c.len() && c[j] == ':' {
        if let Some((b, k)) = parse_cell(c, j + 1) {
            return Some((a, Some(b), k));
        }
    }
    j = j.max(j);
    Some((a, None, j))
}

const ERRS: [(&str, ErrK); 7] = [
    ("#DIV/0!", ErrK::Div0),
    ("#VALUE!", ErrK::Value),
    ("#N/A", ErrK::NA),
    ("#NUM!", ErrK::Num),
    ("#REF!", ErrK::Ref),
    ("#NAME?", ErrK::Name),
    ("#NULL!", ErrK::Null),
];

pub fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '"' {
            let mut j = i + 1;
            let mut st = String::new();
            loop {
                if j >= c.len() {
                    return None;
                }
                if c[j] == '"' {
                    if j + 1 < c.len() && c[j + 1] == '"' {
                        st.push('"');
                        j += 2;
                        continue;
                    }
                    j += 1;
                    break;
                }
                st.push(c[j]);
                j += 1;
            }
            out.push(Tok::Str(st));
            i = j;
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && i + 1 < c.len() && c[i + 1].is_ascii_digit()) {
            let mut j = i;
            while j < c.len() && (c[j].is_ascii_digit() || c[j] == '.') {
                j += 1;
            }
            if j < c.len() && (c[j] == 'E' || c[j] == 'e') {
                let mut k = j + 1;
                if k < c.len() && (c[k] == '+' || c[k] == '-') {
                    k += 1;
                }
                if k < c.len() && c[k].is_ascii_digit() {
                    while k < c.len() && c[k].is_ascii_digit() {
                        k += 1;
                    }
                    j = k;
                }
            }
            let t: String = c[i..j].iter().collect();
            let parsed: f64 = t.parse().ok()?;
            let sig = t.bytes().filter(|b| b.is_ascii_digit()).count();
            let parsed = if sig > 15 { crate::eval::fix15(parsed) } else { parsed };
            out.push(Tok::Num(parsed));
            i = j;
            continue;
        }
        if ch == '#' {
            let rest: String = c[i..].iter().take(8).collect::<String>().to_uppercase();
            let mut ok = false;
            for (name, k) in ERRS.iter() {
                if rest.starts_with(name) {
                    out.push(Tok::Err(*k));
                    i += name.len();
                    ok = true;
                    break;
                }
            }
            if !ok {
                return None;
            }
            continue;
        }
        if ch == '\'' {
            // quoted sheet name
            let mut j = i + 1;
            let mut name = String::new();
            loop {
                if j >= c.len() {
                    return None;
                }
                if c[j] == '\'' {
                    if j + 1 < c.len() && c[j + 1] == '\'' {
                        name.push('\'');
                        j += 2;
                        continue;
                    }
                    j += 1;
                    break;
                }
                name.push(c[j]);
                j += 1;
            }
            if j < c.len() && c[j] == '!' {
                let (a, b, k) = parse_ref(&c, j + 1)?;
                out.push(Tok::Ref(Some(name), a, b));
                i = k;
                continue;
            }
            return None;
        }
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut j = i;
            while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_' || c[j] == '.' || c[j] == '$') {
                j += 1;
            }
            let word: String = c[i..j].iter().collect();
            if j < c.len() && c[j] == '!' {
                let (a, b, k) = parse_ref(&c, j + 1)?;
                out.push(Tok::Ref(Some(word), a, b));
                i = k;
                continue;
            }
            let mut k = j;
            while k < c.len() && c[k].is_whitespace() {
                k += 1;
            }
            if k < c.len() && c[k] == '(' {
                let mut name = word.to_uppercase();
                if let Some(stripped) = name.strip_prefix("_XLFN.") {
                    name = stripped.to_string();
                }
                out.push(Tok::Ident(name));
                i = j;
                continue;
            }
            let up = word.to_uppercase();
            if up == "TRUE" {
                out.push(Tok::Bool(true));
                i = j;
                continue;
            }
            if up == "FALSE" {
                out.push(Tok::Bool(false));
                i = j;
                continue;
            }
            if let Some((a, b, e)) = parse_ref(&c, i) {
                if e == j || (e > j && c[j] == ':') {
                    out.push(Tok::Ref(None, a, b));
                    i = e;
                    continue;
                }
            }
            out.push(Tok::Ident(word));
            i = j;
            continue;
        }
        match ch {
            '(' => out.push(Tok::LP),
            ')' => out.push(Tok::RP),
            ',' | ';' => out.push(Tok::Comma),
            '<' => {
                if i + 1 < c.len() && c[i + 1] == '>' {
                    out.push(Tok::Op("<>".into()));
                    i += 2;
                    continue;
                } else if i + 1 < c.len() && c[i + 1] == '=' {
                    out.push(Tok::Op("<=".into()));
                    i += 2;
                    continue;
                }
                out.push(Tok::Op("<".into()));
            }
            '>' => {
                if i + 1 < c.len() && c[i + 1] == '=' {
                    out.push(Tok::Op(">=".into()));
                    i += 2;
                    continue;
                }
                out.push(Tok::Op(">".into()));
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '=' | '%' => out.push(Tok::Op(ch.to_string())),
            _ => return None,
        }
        i += 1;
    }
    Some(out)
}

pub struct Parser<'a> {
    t: Vec<Tok>,
    pos: usize,
    cur: usize,
    names: &'a HashMap<String, usize>,
}

pub fn parse_formula(src: &str, cur: usize, names: &HashMap<String, usize>) -> Expr {
    let toks = match tokenize(src) {
        Some(t) => t,
        None => return Expr::Lit(Val::Err(ErrK::Name)),
    };
    let mut p = Parser { t: toks, pos: 0, cur, names };
    match p.expr() {
        Some(e) if p.pos == p.t.len() => e,
        _ => Expr::Lit(Val::Err(ErrK::Name)),
    }
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.pos)
    }
    fn is_op(&self, ops: &[&str]) -> Option<String> {
        if let Some(Tok::Op(o)) = self.peek() {
            if ops.contains(&o.as_str()) {
                return Some(o.clone());
            }
        }
        None
    }
    fn expr(&mut self) -> Option<Expr> {
        let mut l = self.concat()?;
        loop {
            let op = match self.is_op(&["=", "<>", "<", "<=", ">", ">="]) {
                Some(o) => o,
                None => break,
            };
            self.pos += 1;
            let r = self.concat()?;
            let bop = match op.as_str() {
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                _ => BinOp::Ge,
            };
            l = Expr::Bin(bop, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn concat(&mut self) -> Option<Expr> {
        let mut l = self.add()?;
        while self.is_op(&["&"]).is_some() {
            self.pos += 1;
            let r = self.add()?;
            l = Expr::Bin(BinOp::Cat, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn add(&mut self) -> Option<Expr> {
        let mut l = self.mul()?;
        while let Some(op) = self.is_op(&["+", "-"]) {
            self.pos += 1;
            let r = self.mul()?;
            let b = if op == "+" { BinOp::Add } else { BinOp::Sub };
            l = Expr::Bin(b, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn mul(&mut self) -> Option<Expr> {
        let mut l = self.pow()?;
        while let Some(op) = self.is_op(&["*", "/"]) {
            self.pos += 1;
            let r = self.pow()?;
            let b = if op == "*" { BinOp::Mul } else { BinOp::Div };
            l = Expr::Bin(b, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn pow(&mut self) -> Option<Expr> {
        let mut l = self.unary()?;
        while self.is_op(&["^"]).is_some() {
            self.pos += 1;
            let r = self.unary()?;
            l = Expr::Bin(BinOp::Pow, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn unary(&mut self) -> Option<Expr> {
        if let Some(op) = self.is_op(&["-", "+"]) {
            self.pos += 1;
            let e = self.unary()?;
            return Some(if op == "-" { Expr::Neg(Box::new(e)) } else { e });
        }
        self.postfix()
    }
    fn postfix(&mut self) -> Option<Expr> {
        let mut e = self.primary()?;
        while self.is_op(&["%"]).is_some() {
            self.pos += 1;
            e = Expr::Pct(Box::new(e));
        }
        Some(e)
    }
    fn sheet_idx(&self, name: &Option<String>) -> Option<usize> {
        match name {
            None => Some(self.cur),
            Some(n) => self.names.get(&n.to_uppercase()).copied(),
        }
    }
    fn primary(&mut self) -> Option<Expr> {
        let tok = self.t.get(self.pos)?.clone();
        self.pos += 1;
        match tok {
            Tok::Num(n) => Some(Expr::Lit(Val::Num(n))),
            Tok::Str(s) => Some(Expr::Lit(Val::Str(Rc::from(s.as_str())))),
            Tok::Bool(b) => Some(Expr::Lit(Val::Bool(b))),
            Tok::Err(e) => Some(Expr::Lit(Val::Err(e))),
            Tok::Ref(sheet, a, b) => {
                let s = match self.sheet_idx(&sheet) {
                    Some(s) => s,
                    None => return Some(Expr::Lit(Val::Err(ErrK::Ref))),
                };
                match b {
                    None => Some(Expr::Ref(s, a.0, a.1)),
                    Some(b) => Some(Expr::Range(
                        s,
                        a.0.min(b.0),
                        a.1.min(b.1),
                        a.0.max(b.0),
                        a.1.max(b.1),
                    )),
                }
            }
            Tok::LP => {
                let e = self.expr()?;
                if self.peek() != Some(&Tok::RP) {
                    return None;
                }
                self.pos += 1;
                Some(e)
            }
            Tok::Ident(name) => {
                if self.peek() != Some(&Tok::LP) {
                    return Some(Expr::Lit(Val::Err(ErrK::Name)));
                }
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() == Some(&Tok::RP) {
                    self.pos += 1;
                    return Some(Expr::Call(name, args));
                }
                loop {
                    // empty argument
                    if self.peek() == Some(&Tok::Comma) {
                        args.push(Expr::Lit(Val::Empty));
                        self.pos += 1;
                        continue;
                    }
                    if self.peek() == Some(&Tok::RP) {
                        args.push(Expr::Lit(Val::Empty));
                        self.pos += 1;
                        break;
                    }
                    let e = self.expr()?;
                    args.push(e);
                    match self.peek() {
                        Some(Tok::Comma) => {
                            self.pos += 1;
                            if self.peek() == Some(&Tok::RP) {
                                args.push(Expr::Lit(Val::Empty));
                                self.pos += 1;
                                break;
                            }
                        }
                        Some(Tok::RP) => {
                            self.pos += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
                Some(Expr::Call(name, args))
            }
            _ => None,
        }
    }
}
