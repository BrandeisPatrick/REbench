use crate::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Cat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(Arc<str>),
    Bool(bool),
    Err(Er),
    Ref { sheet: usize, r: u32, c: u32 },
    Range { sheet: usize, r1: u32, c1: u32, r2: u32, c2: u32 },
    Bin(Op, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String), // function name / TRUE / FALSE / ref text
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Err(Er),
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut toks = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' {
            i += 1;
            continue;
        }
        if c == b'"' {
            let mut st = String::new();
            i += 1;
            loop {
                if i >= b.len() {
                    return None;
                }
                if b[i] == b'"' {
                    if i + 1 < b.len() && b[i + 1] == b'"' {
                        st.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                // utf8 safe: copy char
                let ch = s[i..].chars().next().unwrap();
                st.push(ch);
                i += ch.len_utf8();
            }
            toks.push(Tok::Str(st));
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            let st = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'E' || b[i] == b'e') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            toks.push(Tok::Num(s[st..i].parse().ok()?));
            continue;
        }
        if c == b'#' {
            // error literal
            let errs: [(&str, Er); 7] = [
                ("#DIV/0!", Er::Div0),
                ("#VALUE!", Er::Value),
                ("#N/A", Er::NA),
                ("#NUM!", Er::Num),
                ("#REF!", Er::Ref),
                ("#NAME?", Er::Name),
                ("#NULL!", Er::Null),
            ];
            let mut found = false;
            for (t, e) in errs.iter() {
                if s[i..].starts_with(t) {
                    toks.push(Tok::Err(*e));
                    i += t.len();
                    found = true;
                    break;
                }
            }
            if !found {
                return None;
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c == b'\'' {
            let st = i;
            if c == b'\'' {
                i += 1;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                i += 1;
            }
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$' || b[i] == b'.' || b[i] == b'!' || b[i] == b':') {
                if b[i] == b'!' && i + 1 < b.len() && b[i + 1] == b'\'' {
                    i += 2;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                }
                i += 1;
            }
            toks.push(Tok::Ident(s[st..i].to_string()));
            continue;
        }
        match c {
            b'(' => toks.push(Tok::LParen),
            b')' => toks.push(Tok::RParen),
            b',' => toks.push(Tok::Comma),
            b';' => toks.push(Tok::Comma),
            b'+' => toks.push(Tok::Op("+")),
            b'-' => toks.push(Tok::Op("-")),
            b'*' => toks.push(Tok::Op("*")),
            b'/' => toks.push(Tok::Op("/")),
            b'^' => toks.push(Tok::Op("^")),
            b'&' => toks.push(Tok::Op("&")),
            b'%' => toks.push(Tok::Op("%")),
            b'=' => toks.push(Tok::Op("=")),
            b'<' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    toks.push(Tok::Op("<="));
                    i += 1;
                } else if i + 1 < b.len() && b[i + 1] == b'>' {
                    toks.push(Tok::Op("<>"));
                    i += 1;
                } else {
                    toks.push(Tok::Op("<"));
                }
            }
            b'>' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    toks.push(Tok::Op(">="));
                    i += 1;
                } else {
                    toks.push(Tok::Op(">"));
                }
            }
            _ => return None,
        }
        i += 1;
    }
    Some(toks)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    names: &'a HashMap<String, usize>,
    cur_sheet: usize,
}

fn parse_cell(s: &str) -> Option<(u32, u32)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let mut c: u64 = 0;
    let cs = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        c = c * 26 + (b[i].to_ascii_uppercase() - b'A' + 1) as u64;
        i += 1;
    }
    if i == cs || i - cs > 3 {
        return None;
    }
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let rs = i;
    let mut r: u64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        r = r * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    if i == rs || i != b.len() || r == 0 || r > 1048576 || c > 16384 {
        return None;
    }
    Some(((r - 1) as u32, (c - 1) as u32))
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn parse_cmp(&mut self) -> Option<Expr> {
        let mut l = self.parse_cat()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("=")) => Op::Eq,
                Some(Tok::Op("<>")) => Op::Ne,
                Some(Tok::Op("<")) => Op::Lt,
                Some(Tok::Op(">")) => Op::Gt,
                Some(Tok::Op("<=")) => Op::Le,
                Some(Tok::Op(">=")) => Op::Ge,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_cat()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn parse_cat(&mut self) -> Option<Expr> {
        let mut l = self.parse_add()?;
        while let Some(Tok::Op("&")) = self.peek() {
            self.pos += 1;
            let r = self.parse_add()?;
            l = Expr::Bin(Op::Cat, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn parse_add(&mut self) -> Option<Expr> {
        let mut l = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("+")) => Op::Add,
                Some(Tok::Op("-")) => Op::Sub,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_mul()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn parse_mul(&mut self) -> Option<Expr> {
        let mut l = self.parse_pow()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("*")) => Op::Mul,
                Some(Tok::Op("/")) => Op::Div,
                _ => break,
            };
            self.pos += 1;
            let r = self.parse_pow()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn parse_pow(&mut self) -> Option<Expr> {
        let mut l = self.parse_unary()?;
        while let Some(Tok::Op("^")) = self.peek() {
            self.pos += 1;
            let r = self.parse_unary()?;
            l = Expr::Bin(Op::Pow, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn parse_unary(&mut self) -> Option<Expr> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.pos += 1;
                let e = self.parse_unary()?;
                Some(Expr::Neg(Box::new(e)))
            }
            Some(Tok::Op("+")) => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_postfix(),
        }
    }
    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut e = self.parse_primary()?;
        while let Some(Tok::Op("%")) = self.peek() {
            self.pos += 1;
            e = Expr::Pct(Box::new(e));
        }
        Some(e)
    }
    fn parse_ref(&self, s: &str) -> Option<Expr> {
        // sheet!a1:b2 or a1 or a1:b2
        let (sheet, rest) = if let Some(p) = s.rfind('!') {
            let sn = &s[..p];
            let sn = sn.trim_matches('\'');
            let si = *self.names.get(&sn.to_lowercase())?;
            (si, &s[p + 1..])
        } else {
            (self.cur_sheet, s)
        };
        if let Some(p) = rest.find(':') {
            let (r1, c1) = parse_cell(&rest[..p])?;
            let (r2, c2) = parse_cell(&rest[p + 1..])?;
            Some(Expr::Range { sheet, r1: r1.min(r2), c1: c1.min(c2), r2: r1.max(r2), c2: c1.max(c2) })
        } else {
            let (r, c) = parse_cell(rest)?;
            Some(Expr::Ref { sheet, r, c })
        }
    }
    fn parse_primary(&mut self) -> Option<Expr> {
        match self.next()? {
            Tok::Num(n) => Some(Expr::Num(n)),
            Tok::Str(s) => Some(Expr::Str(Arc::from(s.as_str()))),
            Tok::Err(e) => Some(Expr::Err(e)),
            Tok::LParen => {
                let e = self.parse_cmp()?;
                match self.next()? {
                    Tok::RParen => Some(e),
                    _ => None,
                }
            }
            Tok::Ident(id) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    let mut name = id.to_uppercase();
                    if let Some(p) = name.rfind('.') {
                        name = name[p + 1..].to_string();
                    }
                    let mut args = Vec::new();
                    if let Some(Tok::RParen) = self.peek() {
                        self.pos += 1;
                        return Some(Expr::Call(name, args));
                    }
                    loop {
                        // empty argument
                        match self.peek() {
                            Some(Tok::Comma) => {
                                args.push(Expr::Err(Er::Null)); // placeholder for missing
                            }
                            Some(Tok::RParen) => {
                                args.push(Expr::Err(Er::Null));
                            }
                            _ => {
                                let e = self.parse_cmp()?;
                                args.push(e);
                            }
                        }
                        match self.next()? {
                            Tok::Comma => continue,
                            Tok::RParen => break,
                            _ => return None,
                        }
                    }
                    Some(Expr::Call(name, args))
                } else {
                    let up = id.to_uppercase();
                    if up == "TRUE" {
                        return Some(Expr::Bool(true));
                    }
                    if up == "FALSE" {
                        return Some(Expr::Bool(false));
                    }
                    self.parse_ref(&id)
                }
            }
            _ => None,
        }
    }
}

pub fn parse_formula(s: &str, names: &HashMap<String, usize>, cur_sheet: usize) -> Option<Expr> {
    let toks = tokenize(s)?;
    let mut p = Parser { toks, pos: 0, names, cur_sheet };
    let e = p.parse_cmp()?;
    if p.pos != p.toks.len() {
        return None;
    }
    Some(e)
}
