use crate::*;
use crate::fmt::*;
use crate::parse::Expr;

macro_rules! tr { ($e:expr) => { match $e { Ok(v) => v, Err(e) => return V::E(e) } }; }

pub enum CV { N(f64), S(String, bool), B(bool), Empty, NS(f64, String) }
pub struct Crit { op: u8, v: CV }

pub fn wild_match(p: &[char], s: &[char]) -> bool {
    // p lowercase, s lowercase; supports * ? ~
    let (mut pi, mut si) = (0, 0);
    let (mut star_p, mut star_s) = (usize::MAX, 0);
    while si < s.len() {
        if pi < p.len() {
            let pc = p[pi];
            if pc == '*' { star_p = pi; star_s = si; pi += 1; continue; }
            if pc == '~' && pi + 1 < p.len() { if p[pi + 1] == s[si] { pi += 2; si += 1; continue; } }
            else if pc == '?' || pc == s[si] { pi += 1; si += 1; continue; }
        }
        if star_p != usize::MAX { pi = star_p + 1; star_s += 1; si = star_s; continue; }
        return false;
    }
    while pi < p.len() && p[pi] == '*' { pi += 1; }
    pi == p.len()
}

pub fn wild_match_b(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star_p, mut star_s) = (usize::MAX, 0);
    while si < s.len() {
        if pi < p.len() {
            let pc = p[pi];
            if pc == b'*' { star_p = pi; star_s = si; pi += 1; continue; }
            if pc == b'~' && pi + 1 < p.len() { if p[pi + 1] == s[si] { pi += 2; si += 1; continue; } }
            else if pc == b'?' || pc == s[si] { pi += 1; si += 1; continue; }
        }
        if star_p != usize::MAX { pi = star_p + 1; star_s += 1; si = star_s; continue; }
        return false;
    }
    while pi < p.len() && p[pi] == b'*' { pi += 1; }
    pi == p.len()
}

fn has_wild(s: &str) -> bool { s.contains('*') || s.contains('?') || s.contains('~') }

pub fn make_crit(v: &V) -> Crit {
    match v {
        V::N(x) => Crit { op: EQ, v: CV::N(*x) },
        V::B(b) => Crit { op: EQ, v: CV::N(if *b { 1.0 } else { 0.0 }) },
        V::Empty => Crit { op: EQ, v: CV::N(0.0) },
        V::S(s) => {
            let s: &str = s;
            let (op, rest) = if let Some(r) = s.strip_prefix("<=") { (LE, r) } else if let Some(r) = s.strip_prefix(">=") { (GE, r) }
                else if let Some(r) = s.strip_prefix("<>") { (NE, r) } else if let Some(r) = s.strip_prefix('<') { (LT, r) }
                else if let Some(r) = s.strip_prefix('>') { (GT, r) } else if let Some(r) = s.strip_prefix('=') { (EQ, r) } else { (EQ, s) };
            if rest.is_empty() { return Crit { op, v: CV::Empty }; }
            if let Some(x) = parse_num_str(rest) { return Crit { op, v: CV::NS(x, rest.to_lowercase()) }; }
            let l = rest.to_lowercase();
            let w = has_wild(&l) && (op == EQ || op == NE);
            Crit { op, v: CV::S(l, w) }
        }
        _ => Crit { op: EQ, v: CV::Empty },
    }
}

fn ord_ok(op: u8, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op { EQ => o == Equal, NE => o != Equal, LT => o == Less, GT => o == Greater, LE => o != Greater, _ => o != Less }
}

pub fn crit_match(c: &Crit, v: &V) -> bool {
    match &c.v {
        CV::Empty => {
            let empty = match v { V::Empty => true, V::S(s) => s.is_empty(), _ => false };
            if c.op == EQ { empty } else if c.op == NE { !empty } else { false }
        }
        CV::N(x) => match v {
            V::N(y) => ord_ok(c.op, y.partial_cmp(x).unwrap()),
            V::B(b) => ord_ok(c.op, (if *b { 1.0 } else { 0.0 }).partial_cmp(x).unwrap()),
            V::S(s) => { if c.op == NE { parse_num_str(s).map(|y| y != *x).unwrap_or(true) } else if c.op == EQ { parse_num_str(s).map(|y| y == *x).unwrap_or(false) && false } else { false } }
            _ => c.op == NE,
        },
        CV::NS(x, t) => match v {
            V::N(y) => ord_ok(c.op, y.partial_cmp(x).unwrap()),
            V::B(b) => ord_ok(c.op, (if *b { 1.0 } else { 0.0 }).partial_cmp(x).unwrap()),
            V::S(s) => { if c.op == EQ { lower(s).as_ref() == t.as_str() } else if c.op == NE { lower(s).as_ref() != t.as_str() } else { false } }
            _ => c.op == NE,
        },
        CV::B(b) => match v { V::B(y) => ord_ok(c.op, y.cmp(b)), _ => c.op == NE },
        CV::S(p, w) => {
            if !*w && (c.op == EQ || c.op == NE) { if let V::S(s) = v { if s.is_ascii() { let e = s.eq_ignore_ascii_case(p); return if c.op == EQ { e } else { !e }; } } }
            let ls: std::borrow::Cow<str> = match v {
                V::S(s) => lower(s),
                V::N(x) if c.op == EQ || c.op == NE => std::borrow::Cow::Owned(crate::fmt::num_to_str(*x)),
                V::B(b) if c.op == EQ || c.op == NE => std::borrow::Cow::Borrowed(if *b { "true" } else { "false" }),
                _ => return c.op == NE,
            };
            if *w {
                let m = if p.is_ascii() && ls.is_ascii() { wild_match_b(p.as_bytes(), ls.as_bytes()) } else {
                    let pc: Vec<char> = p.chars().collect(); let sc: Vec<char> = ls.chars().collect();
                    wild_match(&pc, &sc) };
                if c.op == EQ { m } else { !m }
            } else { ord_ok(c.op, crate::coll_cmp(ls.as_ref(), p.as_str())) }
        }
    }
}

fn kv(v: &V) -> Option<LKey> {
    match v { V::N(x) => Some(LKey::N(x.to_bits())), V::S(s) => Some(LKey::S(lower(s).into_owned())), V::B(b) => Some(LKey::N((if *b { 1.0f64 } else { 0.0 }).to_bits())), _ => None }
}

impl Eng {
    fn range_of(&mut self, e: &Expr) -> Result<(u16, u32, u32, u32, u32), u8> {
        match self.eval(e) { V::R(a, b, c, d, f) => Ok((a, b, c, d, f)), V::E(x) => Err(x), _ => Err(2) }
    }
    fn num(&mut self, e: &Expr) -> Result<f64, u8> { let v = self.sc(e); to_num(&v) }
    fn st(&mut self, e: &Expr) -> Result<Rc<str>, u8> { let v = self.sc(e); to_str(&v) }

    // collect numbers from args, aggregate-style
    fn nums(&mut self, args: &[Expr], out: &mut Vec<f64>) -> Result<(), u8> {
        self.allbool = true; let mut first: Option<bool> = None;
        let mut err: Option<u8> = None;
        let mut reset = false;
        let mut referr: Option<u8> = None;
        for a in args {
            if let Expr::Missing = a { continue; }
            let mut this: Option<u8> = None;
            if let Expr::Ref(sh, r, c) = a {
                match self.cell(*sh, *r, *c) { V::N(x) => { out.push(x); if first.is_none() { first = Some(false); } } V::B(b) => { out.push(if b { 1.0 } else { 0.0 }); if first.is_none() { first = Some(true); } }, V::E(e) => this = Some(e), _ => {} }
                if this.is_some() { referr = this; }
                continue;
            }
            let v = self.eval(a);
            match v {
                V::R(sh, r1, c1, r2, c2) => {
                    'o: for r in r1..=r2 { for c in c1..=c2 {
                        match self.cell(sh, r, c) { V::N(x) => { out.push(x); if first.is_none() { first = Some(false); } } V::B(b) => { out.push(if b { 1.0 } else { 0.0 }); if first.is_none() { first = Some(true); } }, V::E(e) => { referr = Some(e); break 'o; } _ => {} }
                    }}
                }
                V::A(arr) => { if first.is_none() { first = Some(false); } for x in arr.d.iter() { match x { V::N(x) => out.push(*x), V::E(e) => { this = Some(*e); break; } _ => {} } } }
                V::E(e) => this = Some(e),
                V::S(_) => { if !crate::refish_val(a) { this = Some(2) } }
                V::B(b) => { out.push(if b { 1.0 } else { 0.0 }); if first.is_none() { first = Some(true); } },
                V::N(x) => { if first.is_none() { first = Some(false); } out.push(x) }
                V::Empty => {}
            }
            if let Some(x) = this { if err.is_none() || reset || has_reset(a) { err = Some(x); reset = false; } }
            if err.is_some() && this.is_none() && has_reset(a) { reset = true; }
        }
        self.allbool = first == Some(true);
        match err.or(referr) { Some(e) => Err(e), None => Ok(()) }
    }
    fn vals_flat(&mut self, e: &Expr) -> Vec<V> {
        let v = self.eval(e);
        match v {
            V::R(..) | V::A(_) => { let a = self.to_arr(v); a.d.clone() }
            x => vec![x],
        }
    }

    fn crit_bits(&mut self, r: (u16, u32, u32, u32, u32), cv: &V) -> Rc<Vec<u64>> {
        let key = (r, format!("{:?}", cv));
        if let Some(b) = self.crit_cache.get(&key) { return b.clone(); }
        let c = make_crit(cv);
        let arr = if let Some(a) = self.range_cache.get(&r) { a.clone() } else {
            let a = self.to_arr(V::R(r.0, r.1, r.2, r.3, r.4));
            self.range_cache.insert(r, a.clone());
            a
        };
        let n = arr.d.len();
        let mut bits = vec![0u64; (n + 63) / 64];
        if let CV::NS(x, _) = &c.v {
            let x = *x;
            let op = c.op;
            for (k, v) in arr.d.iter().enumerate() {
                let m = match v { V::N(y) => match op { EQ => *y == x, NE => *y != x, LT => *y < x, GT => *y > x, LE => *y <= x, _ => *y >= x }, _ => crit_match(&c, v) };
                if m { bits[k >> 6] |= 1u64 << (k & 63); }
            }
        } else {
            for (k, v) in arr.d.iter().enumerate() {
                if crit_match(&c, v) { bits[k >> 6] |= 1u64 << (k & 63); }
            }
        }
        let b = Rc::new(bits);
        self.crit_cache.insert(key, b.clone());
        b
    }
    fn ifs_generic(&mut self, args: &[Expr], first: usize) -> Result<Vec<(u32, u32)>, u8> {
        let mut sets: Vec<((u16, u32, u32, u32, u32), V)> = Vec::new();
        let mut i = first;
        while i + 1 < args.len() {
            let r = self.range_of(&args[i])?;
            let cv = self.sc(&args[i + 1]);
            if let V::E(e) = cv { return Err(e); }
            sets.push((r, cv));
            i += 2;
        }
        if sets.is_empty() { return Err(2); }
        let (h, w) = (sets[0].0 .3 - sets[0].0 .1, sets[0].0 .4 - sets[0].0 .2);
        for s in &sets { if s.0 .3 - s.0 .1 != h || s.0 .4 - s.0 .2 != w { return Err(2); } }
        let mut acc: Vec<u64> = (*self.crit_bits(sets[0].0, &sets[0].1)).clone();
        for s in &sets[1..] {
            let b = self.crit_bits(s.0, &s.1);
            for (x, y) in acc.iter_mut().zip(b.iter()) { *x &= *y; }
        }
        let w1 = w + 1;
        let mut out = Vec::new();
        for (wi, word) in acc.iter().enumerate() {
            let mut m = *word;
            while m != 0 {
                let t = m.trailing_zeros() as usize;
                let k = (wi * 64 + t) as u32;
                out.push((k / w1, k % w1));
                m &= m - 1;
            }
        }
        Ok(out)
    }

    fn lookup_exact(&mut self, key: &V, sh: u16, r1: u32, c1: u32, r2: u32, c2: u32) -> Option<u32> {
        // returns offset in a 1-D range (vertical if c1==c2 else horizontal)
        if let V::S(s) = key { if has_wild(s) {
            let p: Vec<char> = s.to_lowercase().chars().collect();
            let n = if c1 == c2 { r2 - r1 + 1 } else { c2 - c1 + 1 };
            for i in 0..n {
                let v = if c1 == c2 { self.cell(sh, r1 + i, c1) } else { self.cell(sh, r1, c1 + i) };
                if let V::S(t) = &v { let tc: Vec<char> = lower(t).chars().collect(); if wild_match(&p, &tc) { return Some(i); } }
            }
            return None;
        }}
        let k = kv(key)?;
        let ck = (sh, r1, c1, r2, c2);
        let map = if let Some(m) = self.lookup_cache.get(&ck) { m.clone() } else {
            let n = if c1 == c2 { r2 - r1 + 1 } else { c2 - c1 + 1 };
            let mut m = HashMap::with_capacity(n as usize);
            for i in 0..n {
                let v = if c1 == c2 { self.cell(sh, r1 + i, c1) } else { self.cell(sh, r1, c1 + i) };
                if let Some(k) = kv(&v) { m.entry(k).or_insert(i); }
            }
            let m = Rc::new(m);
            self.lookup_cache.insert(ck, m.clone());
            m
        };
        map.get(&k).copied()
    }

    fn lookup_approx(&mut self, key: &V, sh: u16, r1: u32, c1: u32, r2: u32, c2: u32, desc: bool) -> Option<u32> {
        use std::cmp::Ordering::*;
        let n = if c1 == c2 { r2 - r1 + 1 } else { c2 - c1 + 1 };
        let mut best: Option<u32> = None;
        let kn = matches!(key, V::N(_) | V::B(_));
        if !desc && false {
            let mut vals: Vec<V> = Vec::with_capacity(n as usize);
            let mut has_err = false;
            for i in 0..n {
                let v = if c1 == c2 { self.cell(sh, r1 + i, c1) } else { self.cell(sh, r1, c1 + i) };
                if matches!(v, V::E(_)) { has_err = true; break; }
                vals.push(v);
            }
            if !has_err {
                let (mut lo, mut hi) = (0i64, vals.len() as i64 - 1);
                while lo <= hi {
                    let mid = (lo + hi) / 2;
                    let v = &vals[mid as usize];
                    let o = if matches!(v, V::Empty) { Less } else { cmp_vals(v, key) };
                    if o == Equal { return Some(mid as u32); }
                    if o == Less { best = Some(mid as u32); lo = mid + 1; } else { hi = mid - 1; }
                }
                return best;
            }
        }
        for i in 0..n {
            let v = if c1 == c2 { self.cell(sh, r1 + i, c1) } else { self.cell(sh, r1, c1 + i) };
            if matches!(v, V::E(_)) { break; }
            let vn = matches!(v, V::N(_) | V::B(_));
            let vs = matches!(v, V::S(_));
            if !(vn && kn) && !(vs && matches!(key, V::S(_))) { continue; }
            let o = cmp_vals(&v, key);
            if o == Equal { return Some(i); }
            if desc { if o == Less { break; } } else if o == Greater { break; }
            best = Some(i);
        }
        best
    }

    pub fn call(&mut self, name: &str, a: &[Expr]) -> V {
        let n = a.len();
        macro_rules! arg { ($i:expr) => { if $i < n { self.sc(&a[$i]) } else { V::Empty } } }
        if n >= 2 && matches!(name, "LEFT" | "RIGHT" | "MID" | "REPLACE" | "REPT" | "SUBSTITUTE" | "FIND" | "SEARCH" | "EXACT" | "TEXT" | "LOG" | "POWER" | "MOD" | "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" | "DATE" | "WEEKDAY" | "DAYS" | "EDATE" | "EOMONTH" | "DATEDIF") {
            let mut any_ref_err = false;
            for e in a { if let Expr::Ref(sh, r, c) = e { if let V::E(_) = self.cell(*sh, *r, *c) { any_ref_err = true; } } }
            if any_ref_err {
                let (mut comp, mut referr) = (None, None);
                for e in a {
                    if let V::E(x) = self.sc(e) { if matches!(e, Expr::Ref(..)) { referr = Some(x); } else if comp.is_none() { comp = Some(x); } }
                }
                if let Some(x) = comp.or(referr) { return V::E(x); }
            }
        }
        match name {
            "SUM" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); V::N(ksum(&v)) }
            "PRODUCT" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); if v.is_empty() { V::N(0.0) } else { V::N(v.iter().product()) } }
            "SUMSQ" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); V::N(ksum(&v.iter().map(|x| x * x).collect::<Vec<_>>())) }
            "AVERAGE" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); if v.is_empty() { V::E(1) } else { V::N(ksum(&v) / v.len() as f64) } }
            "MAX" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); if self.allbool && !v.is_empty() { let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max); if m == 0.0 || m == 1.0 { return V::B(m == 1.0); } } V::N(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(if v.is_empty() { 0.0 } else { f64::NEG_INFINITY })) }
            "MIN" => { let mut v = Vec::new(); tr!(self.nums(a, &mut v)); if self.allbool && !v.is_empty() { let m = v.iter().cloned().fold(f64::INFINITY, f64::min); if m == 0.0 || m == 1.0 { return V::B(m == 1.0); } } if v.is_empty() { V::N(0.0) } else { V::N(v.iter().cloned().fold(f64::INFINITY, f64::min)) } }
            "MEDIAN" => {
                let mut v = Vec::new(); tr!(self.nums(a, &mut v));
                if v.is_empty() { return V::E(2); }
                v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let m = v.len();
                V::N(if m % 2 == 1 { v[m / 2] } else { (v[m / 2 - 1] + v[m / 2]) / 2.0 })
            }
            "LARGE" | "SMALL" => {
                let mut v = Vec::new(); tr!(self.nums(&a[..1], &mut v));
                let k = tr!(self.num(&a[1]));
                let k = if name == "LARGE" { k.ceil() as i64 } else { k.floor() as i64 };
                if k < 1 || k as usize > v.len() { return V::E(2); }
                v.sort_by(|x, y| x.partial_cmp(y).unwrap());
                if name == "LARGE" { V::N(v[v.len() - k as usize]) } else { V::N(v[k as usize - 1]) }
            }
            "STDEV" | "VAR" => {
                let mut v = Vec::new(); tr!(self.nums(a, &mut v));
                if v.len() < 2 { return V::E(1); }
                let m = ksum(&v) / v.len() as f64;
                let s = ksum(&v.iter().map(|x| (x - m) * (x - m)).collect::<Vec<_>>()) / (v.len() - 1) as f64;
                V::N(if name == "VAR" { s } else { s.sqrt() })
            }
            "COUNT" => {
                let mut c = 0;
                for e in a {
                    let v = self.eval(e);
                    match v {
                        V::R(..) | V::A(_) => { let arr = self.to_arr(v); c += arr.d.iter().filter(|x| matches!(x, V::N(_) | V::B(_))).count(); }
                        V::N(_) | V::B(_) => c += 1,
                        V::S(s) => if !crate::refish_val(e) && parse_num_str(&s).is_some() { c += 1 },
                        _ => {}
                    }
                }
                V::N(c as f64)
            }
            "COUNTA" => {
                let mut c = 0;
                for e in a {
                    let v = self.eval(e);
                    match v {
                        V::R(..) | V::A(_) => { let arr = self.to_arr(v); c += arr.d.iter().filter(|x| !matches!(x, V::Empty)).count(); }
                        V::Empty => {}
                        _ => c += 1,
                    }
                }
                V::N(c as f64)
            }
            "COUNTBLANK" => {
                let v = self.eval(&a[0]);
                let arr = self.to_arr(v);
                V::N(arr.d.iter().filter(|x| match x { V::Empty => true, V::S(s) => s.is_empty(), _ => false }).count() as f64)
            }
            "COUNTIF" | "SUMIF" | "AVERAGEIF" => {
                let r = tr!(self.range_of(&a[0]));
                let cv = arg!(1);
                if let V::E(e) = cv { return V::E(e); }
                let sr = if n > 2 { let s = tr!(self.range_of(&a[2])); (s.0, s.1, s.2) } else { (r.0, r.1, r.2) };
                let bits = self.crit_bits(r, &cv);
                let w1 = r.4 - r.2 + 1;
                let (mut cnt, mut sum) = (0usize, Kahan::new());
                for (wi, word) in bits.iter().enumerate() {
                    let mut m = *word;
                    while m != 0 {
                        let t = m.trailing_zeros() as u32;
                        m &= m - 1;
                        if name == "COUNTIF" { cnt += 1; continue; }
                        let k = wi as u32 * 64 + t;
                        let (dr, dc) = (k / w1, k % w1);
                        match self.cell(sr.0, sr.1 + dr, sr.2 + dc) { V::N(x) => { sum.add(x); cnt += 1; } V::B(b) => { sum.add(if b { 1.0 } else { 0.0 }); cnt += 1; } V::E(e) => return V::E(e), _ => {} }
                    }
                }
                let sum = sum.get(); match name { "COUNTIF" => V::N(cnt as f64), "SUMIF" => V::N(sum), _ => if cnt == 0 { V::E(1) } else { V::N(sum / cnt as f64) } }
            }
            "COUNTIFS" => { let m = tr!(self.ifs_generic(a, 0)); V::N(m.len() as f64) }
            "SUMIFS" | "AVERAGEIFS" => {
                let sr = tr!(self.range_of(&a[0]));
                let m = tr!(self.ifs_generic(a, 1));
                let (mut cnt, mut sum) = (0usize, Kahan::new());
                for (dr, dc) in m {
                    match self.cell(sr.0, sr.1 + dr, sr.2 + dc) { V::N(x) => { sum.add(x); cnt += 1; } V::B(b) => { sum.add(if b { 1.0 } else { 0.0 }); cnt += 1; } V::E(e) => return V::E(e), _ => {} }
                }
                let sum = sum.get(); if name == "SUMIFS" { V::N(sum) } else if cnt == 0 { V::E(1) } else { V::N(sum / cnt as f64) }
            }
            "SUMPRODUCT" => {
                let mut arrs = Vec::new();
                for e in a { let v = self.eval(e); arrs.push(self.to_arr(v)); }
                if arrs.is_empty() { return V::E(2); }
                let (r0, c0) = (arrs[0].rows, arrs[0].cols);
                for x in &arrs { if x.rows != r0 || x.cols != c0 { return V::E(2); } }
                let mut s = Kahan::new();
                for i in 0..r0 * c0 {
                    let mut p = 1.0;
                    for x in &arrs { match &x.d[i] { V::N(v) => p *= v, V::B(b) => { if !*b { p = 0.0; } } V::E(e) => return V::E(*e), _ => p = 0.0 } }
                    s.add(p);
                }
                V::N(s.get())
            }
            "IF" => {
                let c = arg!(0);
                self.logical_seen = false;
                self.last_ref = false;
                let b = tr!(to_bool(&c));
                if b { if n > 1 { if let Expr::Missing = a[1] { return V::N(0.0); } self.eval(&a[1]) } else { V::B(true) } }
                else if n > 2 { if let Expr::Missing = a[2] { return V::N(0.0); } self.eval(&a[2]) } else { V::B(false) }
            }
            "IFS" => {
                let mut i = 0;
                while i + 1 < n { let c = arg!(i); if tr!(to_bool(&c)) { return self.eval(&a[i + 1]); } i += 2; }
                V::E(3)
            }
            "SWITCH" => {
                let x = arg!(0);
                if let V::E(e) = x { return V::E(e); }
                let mut i = 1;
                while i + 1 < n {
                    let c = arg!(i);
                    if let V::E(e) = c { return V::E(e); }
                    let hit = match &x {
                        V::N(_) | V::B(_) => match (to_num(&x), to_num(&c)) { (Ok(p), Ok(q)) => p == q, _ => false },
                        _ => match (to_str(&x), to_str(&c)) { (Ok(p), Ok(q)) => lower(&p) == lower(&q), _ => false },
                    };
                    if hit { return self.eval(&a[i + 1]); }
                    i += 2;
                }
                if i < n { self.eval(&a[i]) } else { V::E(3) }
            }
            "CHOOSE" => {
                let iv = self.num(&a[0]);
                self.last_ref = false;
                let i = tr!(iv).trunc() as i64;
                if i < 1 || i as usize >= n { return V::E(2); }
                self.eval(&a[i as usize])
            }
            "IFERROR" => { let v = arg!(0); if let V::E(_) = v { arg!(1) } else { v } }
            "IFNA" => { let v = arg!(0); if let V::E(3) = v { arg!(1) } else { v } }
            "AND" | "OR" | "XOR" => {
                let mut bs = Vec::new();
                for e in a {
                    let v = self.eval(e);
                    if matches!(v, V::S(_)) && crate::refish_val(e) { continue; }
                    match v {
                        V::R(..) | V::A(_) => { let arr = self.to_arr(v); for x in arr.d.iter() { match x { V::N(y) => bs.push(*y != 0.0), V::B(b) => bs.push(*b), V::E(e) => return V::E(*e), _ => {} } } }
                        V::Empty => {}
                        V::S(_) => return V::E(2),
                        x => bs.push(tr!(to_bool(&x))),
                    }
                }
                if bs.is_empty() { return V::E(2); }
                V::B(match name { "AND" => bs.iter().all(|x| *x), "OR" => bs.iter().any(|x| *x), _ => bs.iter().filter(|x| **x).count() % 2 == 1 })
            }
            "NOT" => { let v = arg!(0); V::B(!tr!(to_bool(&v))) }
            "TRUE" => V::B(true),
            "NA" => V::E(3),
            "FALSE" => V::B(false),
            "ISBLANK" => V::B(matches!(arg!(0), V::Empty)),
            "ISERROR" => V::B(matches!(arg!(0), V::E(_))),
            "ISNA" => V::B(matches!(arg!(0), V::E(3))),
            "ISNUMBER" => { let v = arg!(0); if self.is_fe(&a[0]) { return V::B(false); } V::B(matches!(v, V::N(_) | V::B(_))) }
            "ISTEXT" => V::B(matches!(arg!(0), V::S(_))),
            "ISLOGICAL" => { let v = arg!(0); let r = matches!(v, V::B(_)) || (!matches!(a[0], Expr::Ref(..)) && !matches!(v, V::E(_)) && self.logical_seen); self.logical_seen = true; V::B(r) }
            "N" => match arg!(0) { V::N(x) => V::N(x), V::B(b) => V::N(if b { 1.0 } else { 0.0 }), V::E(e) => V::E(e), _ => V::N(0.0) },
            "T" => match arg!(0) { V::S(s) => V::S(s), V::E(e) => V::E(e), _ => V::S("".into()) },
            "ABS" => V::N(tr!(self.num(&a[0])).abs()),
            "SIGN" => { let x = tr!(self.num(&a[0])); V::N(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }) }
            "INT" => V::N(tr!(self.num(&a[0])).floor()),
            "EXP" => { let r = tr!(self.num(&a[0])).exp(); if r.is_finite() { V::N(r) } else { V::E(4) } }
            "LN" => { let x = tr!(self.num(&a[0])); if x <= 0.0 { V::E(2) } else { V::N(x.ln()) } }
            "LOG10" => { let x = tr!(self.num(&a[0])); if x <= 0.0 { V::E(2) } else { V::N(x.log10()) } }
            "LOG" => {
                let x = tr!(self.num(&a[0]));
                let b = if n > 1 { tr!(self.num(&a[1])) } else { 10.0 };
                if x <= 0.0 || b <= 0.0 { return V::E(2); }
                if b == 1.0 { return V::E(2); }
                V::N(if b == 10.0 { x.log10() } else { x.ln() / b.ln() })
            }
            "SQRT" => { let x = tr!(self.num(&a[0])); if x < 0.0 { V::E(2) } else { V::N(x.sqrt()) } }
            "PI" => V::N(std::f64::consts::PI),
            "POWER" => { let x = arg!(0); let y = arg!(1); binop(POW, &x, &y) }
            "MOD" => {
                let xv = self.sc(&a[0]);
                if let V::E(e) = xv { return V::E(e); }
                let y = tr!(self.num(&a[1]));
                if y == 0.0 { return V::E(1); }
                let x = tr!(to_num(&xv));
                let r = x - y * (x / y).floor();
                V::N(if r != 0.0 && (r.abs() < 1e-15 * x.abs().max(1.0)) { 0.0 } else { r })
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" => {
                let x = tr!(self.num(&a[0]));
                let df = if n > 1 { tr!(self.num(&a[1])).trunc() } else { 0.0 };
                if df.abs() > 32767.0 { return V::E(2); }
                let d = df as i32;
                if name == "ROUND" { return V::N(round_half_away(x, d)); }
                let p = 10f64.powi(d.abs());
                let y = round15(if d >= 0 { x * p } else { x / p });
                let r = if name == "ROUNDUP" { y.abs().ceil() * y.signum() } else { y.trunc() };
                V::N(if d >= 0 { r / p } else { r * p })
            }
            "CONCATENATE" | "CONCAT" => {
                let mut s = String::new();
                let mut err: Option<u8> = None;
                let mut referr: Option<u8> = None;
                let mut reset = false;
                for e in a {
                    let mut this = None;
                    for v in self.vals_flat(e) { match to_str(&v) { Ok(t) => s.push_str(&t), Err(x) => { this = Some(x); break; } } }
                    if matches!(e, Expr::Ref(..) | Expr::Range(..)) { if this.is_some() { referr = this; } continue; }
                    if let Some(x) = this { if err.is_none() || reset || has_reset(e) { err = Some(x); reset = false; } }
                    if err.is_some() && has_reset(e) && this.is_none() { reset = true; }
                }
                if let Some(e) = err.or(referr) { return V::E(e); }
                V::S(s.into())
            }
            "TEXTJOIN" => {
                let d = tr!(self.st(&a[0]));
                let ign = tr!(to_bool(&arg!(1)));
                let mut parts: Vec<Rc<str>> = Vec::new();
                for e in &a[2..] {
                    let ev = self.eval(e);
                    let vals: Vec<V> = match ev {
                        V::R(sh, r1, c1, r2, c2) => { let mut o = Vec::new(); for r in r1..=r2 { for c in c1..=c2 { let v = self.cell(sh, r, c); o.push(if self.cell_fe(sh, r, c) { V::Empty } else { v }); } } o }
                        V::A(arr) => arr.d.clone(),
                        x => vec![if self.is_fe(e) { V::Empty } else { x }],
                    };
                    for v in vals {
                        let s = tr!(to_str(&v));
                        if ign && s.is_empty() { continue; }
                        parts.push(s);
                    }
                }
                let v: Vec<&str> = parts.iter().map(|x| x.as_ref()).collect();
                V::S(v.join(&d).into())
            }
            "LEN" => V::N(tr!(self.st(&a[0])).chars().count() as f64),
            "LOWER" => V::S(tr!(self.st(&a[0])).to_lowercase().into()),
            "UPPER" => V::S(tr!(self.st(&a[0])).to_uppercase().into()),
            "PROPER" => {
                let s = tr!(self.st(&a[0]));
                let mut o = String::new(); let mut prev = false;
                for ch in s.chars() {
                    if ch.is_alphabetic() { if prev { o.extend(ch.to_lowercase()); } else { o.extend(ch.to_uppercase()); } prev = true; }
                    else { o.push(ch); prev = false; }
                }
                V::S(o.into())
            }
            "TRIM" => {
                let s = tr!(self.st(&a[0]));
                V::S(s.split(' ').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ").into())
            }
            "LEFT" | "RIGHT" => {
                let s = tr!(self.st(&a[0]));
                let k = if n > 1 { tr!(self.num(&a[1])).trunc() } else { 1.0 };
                if k < 0.0 { return V::E(2); }
                let c: Vec<char> = s.chars().collect();
                let k = (k as usize).min(c.len());
                V::S(if name == "LEFT" { c[..k].iter().collect::<String>() } else { c[c.len() - k..].iter().collect::<String>() }.into())
            }
            "MID" => {
                let s = tr!(self.st(&a[0]));
                let st = tr!(self.num(&a[1])).trunc(); let k = tr!(self.num(&a[2])).trunc();
                if st < 1.0 || k < 0.0 { return V::E(2); }
                let c: Vec<char> = s.chars().collect();
                let st = (st as usize - 1).min(c.len());
                let e = (st + k as usize).min(c.len());
                V::S(c[st..e].iter().collect::<String>().into())
            }
            "REPLACE" => {
                let s = tr!(self.st(&a[0]));
                let st = tr!(self.num(&a[1])).trunc(); let k = tr!(self.num(&a[2])).trunc();
                let r = tr!(self.st(&a[3]));
                if st < 1.0 || k < 0.0 { return V::E(2); }
                let c: Vec<char> = s.chars().collect();
                let st = (st as usize - 1).min(c.len());
                let e = (st + k as usize).min(c.len());
                let mut o: String = c[..st].iter().collect(); o.push_str(&r); o.extend(c[e..].iter());
                V::S(o.into())
            }
            "REPT" => {
                let s = tr!(self.st(&a[0])); let k = tr!(self.num(&a[1])).trunc();
                if k < 0.0 { return V::E(2); }
                V::S(s.repeat(k as usize).into())
            }
            "SUBSTITUTE" => {
                let s = tr!(self.st(&a[0])); let old = tr!(self.st(&a[1])); let new = tr!(self.st(&a[2]));
                if n > 3 {
                    let k = tr!(self.num(&a[3])).trunc() as i64;
                    if k < 1 { return V::E(2); }
                    if old.is_empty() { return V::S(s); }
                    let mut cnt = 0; let mut pos = 0;
                    while let Some(i) = s[pos..].find(&*old) {
                        cnt += 1;
                        if cnt == k { let mut o = s[..pos + i].to_string(); o.push_str(&new); o.push_str(&s[pos + i + old.len()..]); return V::S(o.into()); }
                        pos += i + old.len();
                    }
                    return V::S(s);
                }
                if old.is_empty() { return V::S(s); }
                V::S(s.replace(&*old, &new).into())
            }
            "FIND" | "SEARCH" => {
                let needle = tr!(self.st(&a[0])); let hay = tr!(self.st(&a[1]));
                let st = if n > 2 { tr!(self.num(&a[2])).trunc() } else { 1.0 };
                let hc: Vec<char> = hay.chars().collect();
                if needle.is_empty() { return V::E(2); }
                if st < 1.0 || st as usize > hc.len() + 1 { return V::E(2); }
                let st = st as usize - 1;
                if name == "FIND" {
                    let nc: Vec<char> = needle.chars().collect();
                    for i in st..=hc.len() { if i + nc.len() <= hc.len() && hc[i..i + nc.len()] == nc[..] { return V::N((i + 1) as f64); } }
                    V::E(2)
                } else {
                    let nl: Vec<char> = needle.to_lowercase().chars().collect();
                    let hl: Vec<char> = hay.to_lowercase().chars().collect();
                    let mut p = nl.clone(); p.push('*');
                    for i in st..=hl.len() {
                        if has_wild(&needle) { if wild_match(&p, &hl[i..]) { return V::N((i + 1) as f64); } }
                        else if i + nl.len() <= hl.len() && hl[i..i + nl.len()] == nl[..] { return V::N((i + 1) as f64); }
                    }
                    V::E(2)
                }
            }
            "EXACT" => { let x = tr!(self.st(&a[0])); let y = tr!(self.st(&a[1])); V::B(x == y) }
            "VALUE" => {
                let v = arg!(0);
                match v { V::N(x) => V::N(x), V::E(e) => V::E(e), V::Empty => V::N(0.0), V::B(b) => if crate::refish_val(&a[0]) { V::N(if b { 1.0 } else { 0.0 }) } else { V::B(b) }, V::S(s) => match parse_num_str(&s) { Some(x) => V::N(x), None => V::E(2) }, _ => V::E(2) }
            }
            "TEXT" => {
                let v = arg!(0);
                let f = tr!(self.st(&a[1]));
                match v {
                    V::E(e) => V::E(e),
                    V::S(s) => match parse_num_str(&s) { Some(x) => V::S(text_format(x, &f).into()), None => V::S(s) },
                    x => { let x = tr!(to_num(&x)); V::S(text_format(x, &f).into()) }
                }
            }
            "DATE" => {
                let yf = tr!(self.num(&a[0])).trunc(); let mf = tr!(self.num(&a[1])).trunc(); let df = tr!(self.num(&a[2])).trunc();
                let ok16 = |x: f64| x >= -32768.0 && x <= 32767.0;
                if !ok16(yf) || !ok16(mf) || !ok16(df) { return V::E(2); }
                let (mut y, m, d) = (yf as i64, mf as i64, df as i64);
                if y < 0 { return V::E(2); }
                if y < 30 { y += 2000; } else if y < 100 { y += 1900; }
                let tm = y * 12 + (m - 1);
                let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) + 1);
                if yy < 1583 { return V::E(2); }
                V::N(ymd_to_serial(yy, mm, 1) + (d - 1) as f64)
            }
            "YEAR" | "MONTH" | "DAY" => {
                let x = tr!(self.num(&a[0]));
                let (y, m, d) = serial_to_ymd(x);
                V::N(match name { "YEAR" => y, "MONTH" => m, _ => d } as f64)
            }
            "WEEKDAY" => {
                let x = tr!(self.num(&a[0])).floor() as i64;
                let t = if n > 1 { tr!(self.num(&a[1])) as i64 } else { 1 };
                let w = (x + 6).rem_euclid(7); // 0=Sunday
                V::N(match t { 1 => w + 1, 2 => (w + 6) % 7 + 1, 3 => (w + 6) % 7, 11..=17 => (w - (t - 10) % 7 + 7) % 7 + 1, _ => return V::E(2) } as f64)
            }
            "DAYS" => { let e = tr!(self.num(&a[0])); let s = tr!(self.num(&a[1])); V::N(e - s) }
            "EDATE" | "EOMONTH" => {
                let s = tr!(self.num(&a[0])); let m = tr!(self.num(&a[1])).trunc() as i64;
                let (y, mo, d) = serial_to_ymd(s.trunc());
                let tm = y * 12 + mo - 1 + m;
                let (yy, mm) = (tm.div_euclid(12), tm.rem_euclid(12) + 1);
                let dim = days_in_month(yy, mm);
                if name == "EDATE" { V::N(ymd_to_serial(yy, mm, d.min(dim))) } else { V::N(ymd_to_serial(yy, mm, dim)) }
            }
            "DATEDIF" => {
                let s = tr!(self.num(&a[0])).floor(); let e = tr!(self.num(&a[1])).floor(); let u = tr!(self.st(&a[2])).to_uppercase();
                if s > e { return V::E(2); }
                if s == e { return V::N(0.0); }
                let (y1, m1, d1) = serial_to_ymd(s); let (y2, m2, d2) = serial_to_ymd(e);
                let mut months = (y2 - y1) * 12 + (m2 - m1);
                if d2 < d1 { months -= 1; }
                match u.as_str() {
                    "D" => V::N(e - s),
                    "M" => V::N(months as f64),
                    "Y" => V::N((months / 12) as f64),
                    "YM" => V::N((months % 12) as f64),
                    "MD" => {
                        if d2 >= d1 { V::N((d2 - d1) as f64) } else {
                            let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                            V::N((days_in_month(py, pm) - d1 + d2) as f64)
                        }
                    }
                    "YD" => {
                        let mut ys = y2; if (m2, d2) < (m1, d1) { ys -= 1; }
                        let st = ymd_to_serial(ys, m1, d1.min(days_in_month(ys, m1)));
                        V::N(e - st)
                    }
                    _ => V::E(2),
                }
            }
            "VLOOKUP" | "HLOOKUP" => {
                let key = arg!(0);
                if let V::E(e) = key { return V::E(e); }
                let (sh, r1, c1, r2, c2) = tr!(self.range_of(&a[1]));
                let idx = tr!(self.num(&a[2])).trunc() as i64;
                let approx = if n > 3 { tr!(to_bool(&arg!(3))) } else { true };
                let v = name == "VLOOKUP";
                let width = if v { c2 - c1 + 1 } else { r2 - r1 + 1 } as i64;
                if idx < 1 { return V::E(2); }
                if idx > width { return V::E(2); }
                let pos = if v {
                    if approx { self.lookup_approx(&key, sh, r1, c1, r2, c1, false) } else { self.lookup_exact(&key, sh, r1, c1, r2, c1) }
                } else if approx { self.lookup_approx(&key, sh, r1, c1, r1, c2, false) } else { self.lookup_exact(&key, sh, r1, c1, r1, c2) };
                match pos {
                    None => V::E(3),
                    Some(p) => { let r = if v { self.cell(sh, r1 + p, c1 + idx as u32 - 1) } else { self.cell(sh, r1 + idx as u32 - 1, c1 + p) }; if let V::B(b) = r { V::N(if b { 1.0 } else { 0.0 }) } else { r } }
                }
            }
            "MATCH" => {
                let key = arg!(0);
                if let V::E(e) = key { return V::E(e); }
                let (sh, r1, c1, r2, c2) = tr!(self.range_of(&a[1]));
                let t = if n > 2 { tr!(self.num(&a[2])).trunc() } else { 1.0 };
                if t != 0.0 && t != 1.0 && t != -1.0 { return V::E(2); }
                if r1 != r2 && c1 != c2 { return V::E(3); }
                let pos = if t == 0.0 { self.lookup_exact(&key, sh, r1, c1, r2, c2) } else { self.lookup_approx(&key, sh, r1, c1, r2, c2, t < 0.0) };
                match pos { None => V::E(3), Some(p) => V::N((p + 1) as f64) }
            }
            "INDEX" => {
                let v = self.eval(&a[0]);
                let r = if n > 1 { tr!(to_num(&arg!(1))).trunc() as i64 } else { 0 };
                let c = if n > 2 { tr!(to_num(&arg!(2))).trunc() as i64 } else { 0 };
                match v {
                    V::R(sh, r1, c1, r2, c2) => {
                        let h = (r2 - r1 + 1) as i64; let w = (c2 - c1 + 1) as i64;
                        let (mut r, mut c) = (r, c);
                        if n == 2 && h == 1 { c = r; r = 1; }
                        if r < 0 || c < 0 || r > 1048576 || c > 16384 { return V::E(2); }
                        if r > h || c > w { return V::E(5); }
                        if r == 0 && c == 0 { return V::R(sh, r1, c1, r2, c2); }
                        if r == 0 { return V::R(sh, r1, c1 + c as u32 - 1, r2, c1 + c as u32 - 1); }
                        if c == 0 { if w == 1 { return self.cell(sh, r1 + r as u32 - 1, c1); } return V::R(sh, r1 + r as u32 - 1, c1, r1 + r as u32 - 1, c2); }
                        self.cell(sh, r1 + r as u32 - 1, c1 + c as u32 - 1)
                    }
                    V::E(e) => V::E(e),
                    _ => V::E(2),
                }
            }
            "XLOOKUP" => {
                let key = arg!(0);
                if let V::E(e) = key { return V::E(e); }
                let (sh, r1, c1, r2, c2) = tr!(self.range_of(&a[1]));
                let ret = tr!(self.range_of(&a[2]));
                let mm = if n > 4 && !matches!(a[4], Expr::Missing) { tr!(self.num(&a[4])) as i64 } else { 0 };
                let sm = if n > 5 { tr!(self.num(&a[5])) as i64 } else { 1 };
                let vert = c1 == c2;
                if (vert && ret.3 - ret.1 != r2 - r1) || (!vert && ret.4 - ret.2 != c2 - c1) { return V::E(2); }
                let len = if vert { r2 - r1 + 1 } else { c2 - c1 + 1 };
                let get = |s: &mut Self, i: u32| if vert { s.cell(sh, r1 + i, c1) } else { s.cell(sh, r1, c1 + i) };
                let mut pos: Option<u32> = None;
                if mm == 0 && sm == 1 && !matches!(&key, V::S(s) if has_wild(s)) {
                    pos = self.lookup_exact(&key, sh, r1, c1, r2, c2);
                } else {
                    let order: Vec<u32> = if sm < 0 { (0..len).rev().collect() } else { (0..len).collect() };
                    let wp: Vec<char> = if let V::S(s) = &key { s.to_lowercase().chars().collect() } else { vec![] };
                    let mut best: Option<(u32, V)> = None;
                    for i in order {
                        let v = get(self, i);
                        let isn = |q: &V| matches!(q, V::N(_) | V::B(_));
                        let same = (isn(&v) && isn(&key)) || std::mem::discriminant(&v) == std::mem::discriminant(&key);
                        if mm == 2 {
                            if let (V::S(t), V::S(_)) = (&v, &key) { let tc: Vec<char> = lower(t).chars().collect(); if wild_match(&wp, &tc) { pos = Some(i); break; } }
                            else if same && cmp_vals(&v, &key) == std::cmp::Ordering::Equal { pos = Some(i); break; }
                            continue;
                        }
                        if !same { continue; }
                        let o = cmp_vals(&v, &key);
                        if o == std::cmp::Ordering::Equal { pos = Some(i); break; }
                        if mm == -1 && o == std::cmp::Ordering::Less {
                            if best.as_ref().map(|b| cmp_vals(&v, &b.1) == std::cmp::Ordering::Greater).unwrap_or(true) { best = Some((i, v)); }
                        } else if mm == 1 && o == std::cmp::Ordering::Greater {
                            if best.as_ref().map(|b| cmp_vals(&v, &b.1) == std::cmp::Ordering::Less).unwrap_or(true) { best = Some((i, v)); }
                        }
                    }
                    if pos.is_none() { pos = best.map(|b| b.0); }
                }
                match pos {
                    None => if n > 3 && !matches!(a[3], Expr::Missing) { self.eval(&a[3]) } else { V::E(3) },
                    Some(p) => {
                        if vert { if ret.2 == ret.4 { self.cell(ret.0, ret.1 + p, ret.2) } else { V::R(ret.0, ret.1 + p, ret.2, ret.1 + p, ret.4) } }
                        else if ret.1 == ret.3 { self.cell(ret.0, ret.1, ret.2 + p) } else { V::R(ret.0, ret.1, ret.2 + p, ret.3, ret.2 + p) }
                    }
                }
            }
            _ => V::E(6),
        }
    }
}

pub fn has_reset(e: &Expr) -> bool {
    match e {
        Expr::Func(n, a) => matches!(*n, "IFS" | "SWITCH" | "IFERROR" | "IFNA") || a.iter().any(has_reset),
        Expr::Bin(_, x, y) => has_reset(x) || has_reset(y),
        Expr::Neg(x) | Expr::Pct(x) => has_reset(x),
        _ => false,
    }
}

pub struct Kahan { s: f64, c: f64 }
impl Kahan {
    pub fn new() -> Self { Kahan { s: 0.0, c: 0.0 } }
    #[inline] pub fn add(&mut self, x: f64) {
        let t = self.s + x;
        if self.s.abs() >= x.abs() { self.c += (self.s - t) + x; } else { self.c += (x - t) + self.s; }
        self.s = t;
    }
    pub fn get(&self) -> f64 { self.s + self.c }
}
pub fn ksum(v: &[f64]) -> f64 { let mut k = Kahan::new(); for x in v { k.add(*x); } k.get() }
