use crate::*;
use crate::eval::*;
use crate::parse::*;
use crate::fmt::*;

pub struct Crit { pub key: String, raw: String, op: u8, num: Option<f64>, pat: Vec<char>, lower: String, wild: bool, ascii: bool }

fn has_wild(s: &str) -> bool { s.contains('*') || s.contains('?') || s.contains('~') }

pub fn wild_match(p: &[char], t: &[char]) -> bool {
    // full match, case-insensitive pre-lowered
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '~' && pi + 1 < p.len() {
            if p[pi + 1] == t[ti] { pi += 2; ti += 1; continue; }
        } else if pi < p.len() && p[pi] == '*' { star = pi; mark = ti; pi += 1; continue; }
        else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) { pi += 1; ti += 1; continue; }
        if star != usize::MAX { pi = star + 1; mark += 1; ti = mark; continue; }
        return false;
    }
    while pi < p.len() && p[pi] == '*' { pi += 1; }
    pi == p.len()
}

impl Crit {
    pub fn new(v: &Value) -> Result<Crit, Er> {
        match v {
            Value::Err(e) => Err(*e),
            Value::Num(x) => Ok(Crit { key: format!("n{}", x.to_bits()), raw: String::new(), op: OP_EQ, num: Some(*x), pat: vec![], lower: String::new(), wild: false, ascii: true }),
            Value::Bool(b) => Ok(Crit { key: format!("n{}", (if *b { 1.0f64 } else { 0.0 }).to_bits()), raw: String::new(), op: OP_EQ, num: Some(if *b { 1.0 } else { 0.0 }), pat: vec![], lower: String::new(), wild: false, ascii: true }),
            Value::Empty | Value::EmptyF => Ok(Crit { key: format!("n{}", 0f64.to_bits()), raw: String::new(), op: OP_EQ, num: Some(0.0), pat: vec![], lower: String::new(), wild: false, ascii: true }),
            Value::Str(s) => {
                let (op, rest) = if let Some(r) = s.strip_prefix("<>") { (OP_NE, r) } else if let Some(r) = s.strip_prefix("<=") { (OP_LE, r) }
                    else if let Some(r) = s.strip_prefix(">=") { (OP_GE, r) } else if let Some(r) = s.strip_prefix('<') { (OP_LT, r) }
                    else if let Some(r) = s.strip_prefix('>') { (OP_GT, r) } else if let Some(r) = s.strip_prefix('=') { (OP_EQ, r) } else { (OP_EQ, &s[..]) };
                let num = if rest.is_empty() { None } else { parse_num_str(rest) };
                let lower = rest.to_lowercase();
                let wild = has_wild(&lower);
                Ok(Crit { key: format!("s{}", s), raw: rest.to_lowercase(), op, num, pat: lower.chars().collect(), ascii: rest.is_ascii(), lower, wild })
            }
        }
    }
    pub fn matches(&self, v: &Value) -> bool {
        if let Some(x) = self.num {
            let y = match v { Value::Num(y) => *y, Value::Bool(b) => if *b { 1.0 } else { 0.0 }, Value::Err(_) => return false,
                Value::Str(s) if !self.raw.is_empty() && (self.op == OP_EQ || self.op == OP_NE) => { let m = s.to_lowercase() == self.raw; return if self.op == OP_EQ { m } else { !m }; }
                _ => return self.op == OP_NE };
            let o = cmp_ord(y, x);
            return match self.op { OP_EQ => o == Ordering::Equal, OP_NE => o != Ordering::Equal, OP_LT => o == Ordering::Less, OP_GT => o == Ordering::Greater, OP_LE => o != Ordering::Greater, _ => o != Ordering::Less };
        }
        match v {
            Value::Num(_) | Value::Bool(_) if self.wild && (self.op == OP_EQ || self.op == OP_NE) => {
                let st = match v { Value::Num(x) => num_to_str(*x), Value::Bool(b) => (if *b { "TRUE" } else { "FALSE" }).to_string(), _ => String::new() };
                let t: Vec<char> = st.to_lowercase().chars().collect();
                let m = wild_match(&self.pat, &t);
                if self.op == OP_EQ { m } else { !m }
            }
            Value::Str(s) => {
                match self.op {
                    OP_EQ | OP_NE => {
                        let m = if self.wild { let t: Vec<char> = s.to_lowercase().chars().collect(); wild_match(&self.pat, &t) }
                            else if self.ascii && s.is_ascii() { s.len() == self.lower.len() && s.eq_ignore_ascii_case(&self.lower) }
                            else { s.to_lowercase() == self.lower };
                        if self.op == OP_EQ { m } else { !m }
                    }
                    _ => { let o = str_cmp(s, &self.lower); match self.op { OP_LT => o == Ordering::Less, OP_GT => o == Ordering::Greater, OP_LE => o != Ordering::Greater, _ => o != Ordering::Less } }
                }
            }
            Value::Empty => {
                if self.lower.is_empty() { self.op == OP_EQ } else { self.op == OP_NE }
            }
            Value::Err(_) => false,
            _ => self.op == OP_NE,
        }
    }
}

type Rg = (usize, usize, usize, usize, usize);

impl Engine {
    fn num(&self, n: &Node, cx: Cx) -> Result<f64, Er> { let v = self.eval(n, cx); self.to_num(&v) }
    fn st(&self, n: &Node, cx: Cx) -> Result<Rc<str>, Er> { let v = self.eval(n, cx); if let Value::Err(e) = v { Err(e) } else { Ok(self.to_str(&v)) } }
    fn clamp(&self, g: Rg) -> Rg {
        let (rows, cols) = if g.0 < self.nsheets() { self.dims(g.0) } else { (0, 0) };
        let r2 = g.3.min(rows.wrapping_sub(1)); let c2 = g.4.min(cols.wrapping_sub(1));
        if rows == 0 || cols == 0 { return (g.0, 1, 1, 0, 0); }
        (g.0, g.1, g.2, r2, c2)
    }
    fn is_empty_rg(g: Rg) -> bool { g.1 > g.3 || g.2 > g.4 }

    // iterate numeric items. returns Err on error. f gets numbers.
    fn pre_args(&self, args: &[Node], cx: Cx) -> Result<Vec<Arg>, Er> {
        let mut v = Vec::with_capacity(args.len());
        let mut pending: Option<Er> = None;
        for a in args {
            let x = self.arg(a, cx);
            if let (Arg::V(Value::Err(e)), Node::Func(..) | Node::Bin(..) | Node::Neg(..) | Node::Pct(..) | Node::Err(..)) = (&x, a) { if !matches!(a, Node::Err(Er::Null)) && (pending.is_none() || is_jump_top(a)) { pending = Some(*e); } }
            v.push(x);
        }
        if let Some(e) = pending { return Err(e); }
        Ok(v)
    }
    fn fold_nums(&self, args: &[Node], cx: Cx, f: &mut dyn FnMut(f64), count_all: bool) -> Result<(), Er> {
        let pre = if count_all { args.iter().map(|a| self.arg(a, cx)).collect() } else { self.pre_args(args, cx)? };
        for ar in pre {
            match ar {
                Arg::R(s, r1, c1, r2, c2) => {
                    let g = self.clamp((s, r1, c1, r2, c2));
                    if Self::is_empty_rg(g) { continue; }
                    for r in g.1..=g.3 { for c in g.2..=g.4 {
                        match self.cell(s, r, c) { Value::Num(x) => f(*x), Value::Bool(b) => f(if *b { 1.0 } else { 0.0 }), Value::Err(e) if !count_all => return Err(*e), _ => {} }
                    } }
                }
                Arg::V(v) => match v {
                    Value::Num(x) => f(x), Value::Bool(b) => f(if b { 1.0 } else { 0.0 }),
                    Value::Err(e) => if !count_all { return Err(e) },
                    Value::Str(s) => if count_all { if let Some(x) = parse_num_str(&s) { f(x) } } else { return Err(Er::Value) },
                    Value::Empty | Value::EmptyF => f(0.0),
                },
            }
        }
        Ok(())
    }
    fn nums(&self, args: &[Node], cx: Cx) -> Result<Vec<f64>, Er> {
        let mut v = Vec::new();
        self.fold_nums(args, cx, &mut |x| v.push(x), false)?;
        Ok(v)
    }

    fn rg(&self, n: &Node, cx: Cx) -> Result<Rg, Er> {
        match self.arg(n, cx) { Arg::R(s, a, b, c, d) => Ok((s, a, b, c, d)), Arg::V(Value::Err(e)) => Err(e), Arg::V(_) => Err(Er::Value) }
    }
    // all values from args flattened (for CONCAT, TEXTJOIN etc.)
    fn flat(&self, n: &Node, cx: Cx, out: &mut Vec<Value>) {
        match self.arg(n, cx) {
            Arg::V(v) => out.push(v),
            Arg::R(s, r1, c1, r2, c2) => {
                let g = self.clamp((s, r1, c1, r2, c2));
                if Self::is_empty_rg(g) { return; }
                for r in g.1..=g.3 { for c in g.2..=g.4 { out.push(match self.cell(s, r, c) { Value::Bool(b) => Value::Num(if *b {1.0} else {0.0}), Value::EmptyF => Value::Str(Rc::from("")), v => v.clone() }); } }
            }
        }
    }

    fn lookup_eq(&self, key: &Value, cell: &Value) -> bool {
        match (key, cell) {
            (Value::Num(a), Value::Num(b)) => a == b || approx_eq(*a, *b),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Bool(b)) => *a == if *b { 1.0 } else { 0.0 },
            (Value::Bool(a), Value::Num(b)) => *b == if *a { 1.0 } else { 0.0 },
            (Value::Str(a), Value::Num(_) | Value::Bool(_)) if !has_wild(a) => { match parse_num_str(a) { Some(x) => self.lookup_eq(&Value::Num(x), cell), None => false } }
            (Value::Str(a), Value::Str(b)) => {
                if has_wild(a) { let p: Vec<char> = a.to_lowercase().chars().collect(); let t: Vec<char> = b.to_lowercase().chars().collect(); wild_match(&p, &t) }
                else { a.len() == b.len() && a.eq_ignore_ascii_case(b) || (!a.is_ascii() && a.to_lowercase() == b.to_lowercase()) }
            }
            (Value::Empty, Value::Empty) => true,
            _ => false,
        }
    }
    fn typ_rank(v: &Value) -> u8 { match v { Value::Num(_) | Value::Bool(_) => 0, Value::Str(_) => 1, _ => 2 } }

    // sorted search over a list accessor; returns index of last element <= key
    fn bin_search(&self, key: &Value, n: usize, get: &dyn Fn(usize) -> Value, desc: bool) -> Option<usize> {
        let kr = Self::typ_rank(key);
        if n > 0 {
            let f = get(0);
            if Self::typ_rank(&f) == kr {
                let o = self.compare(&f, key);
                if (!desc && o == Ordering::Greater) || (desc && o == Ordering::Less) { return None; }
                if o == Ordering::Equal && (n == 1 || { let g = get(1); Self::typ_rank(&g) == kr && { let o2 = self.compare(&g, key); (!desc && o2 == Ordering::Greater) || (desc && o2 == Ordering::Less) } }) { return Some(0); }
            }
        }
        let (mut lo, mut hi) = (0i64, n as i64 - 1);
        let mut res: Option<usize> = None;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let v = get(mid as usize);
            let le = if Self::typ_rank(&v) != kr {
                // different type: strings greater than numbers
                if desc { Self::typ_rank(&v) > kr } else { Self::typ_rank(&v) < kr }
            } else { let o = self.compare(&v, key); if desc { o != Ordering::Less } else { o != Ordering::Greater } };
            if le { res = Some(mid as usize); lo = mid + 1; } else { hi = mid - 1; }
        }
        res
    }

    fn crit_bits(&self, g: &Rg, cr: &Crit) -> Rc<Vec<u64>> {
        let key = (g.0, g.1, g.2, g.3, g.4, cr.key.clone());
        if let Some(b) = self.ccache.borrow().get(&key) { return b.clone(); }
        let (nr, nc) = (g.3 - g.1 + 1, g.4 - g.2 + 1);
        let mut bits = vec![0u64; (nr * nc + 63) / 64];
        let mut i = 0;
        for r in 0..nr { for c in 0..nc {
            if cr.matches(self.cell(g.0, g.1 + r, g.2 + c)) { bits[i >> 6] |= 1u64 << (i & 63); }
            i += 1;
        } }
        let rc = Rc::new(bits);
        self.ccache.borrow_mut().insert(key, rc.clone());
        rc
    }
    fn crit_iter(&self, rngs: &[Rg], crits: &[Crit], mut f: impl FnMut(usize, usize)) {
        let (nr, nc) = (rngs[0].3 - rngs[0].1 + 1, rngs[0].4 - rngs[0].2 + 1);
        if nr * nc >= 512 {
            let mut acc: Vec<u64> = Vec::new();
            for (k, (g, cr)) in rngs.iter().zip(crits.iter()).enumerate() {
                let b = self.crit_bits(g, cr);
                if k == 0 { acc = (*b).clone(); } else { for (a, x) in acc.iter_mut().zip(b.iter()) { *a &= *x; } }
            }
            for (w, &word) in acc.iter().enumerate() {
                let mut x = word;
                while x != 0 {
                    let t = x.trailing_zeros() as usize; x &= x - 1;
                    let i = w * 64 + t;
                    f(i / nc, i % nc);
                }
            }
            return;
        }
        for r in 0..nr { for c in 0..nc {
            let mut ok = true;
            for (g, cr) in rngs.iter().zip(crits.iter()) {
                if !cr.matches(self.cell(g.0, g.1 + r, g.2 + c)) { ok = false; break; }
            }
            if ok { f(r, c); }
        } }
    }
    // parse (range, crit) pairs from args
    fn parse_pairs(&self, args: &[Node], cx: Cx) -> Result<(Vec<Rg>, Vec<Crit>), Er> {
        let mut rs = Vec::new(); let mut cs = Vec::new();
        let mut i = 0;
        while i + 1 < args.len() {
            let g = self.rg(&args[i], cx)?;
            let cv = self.eval(&args[i + 1], cx);
            cs.push(Crit::new(&cv)?);
            rs.push(g);
            i += 2;
        }
        let (h, w) = (rs[0].3 - rs[0].1, rs[0].4 - rs[0].2);
        for g in &rs { if g.3 - g.1 != h || g.4 - g.2 != w { return Err(Er::Value); } }
        Ok((rs, cs))
    }

    pub fn call(&self, name: &str, args: &[Node], cx: Cx) -> Arg {
        match self.call2(name, args, cx) { Ok(a) => a, Err(e) => Arg::V(Value::Err(e)) }
    }

    fn call2(&self, name: &str, args: &[Node], cx: Cx) -> Result<Arg, Er> {
        let n = args.len();
        let ok = |v: Value| -> Result<Arg, Er> { Ok(Arg::V(v)) };
        let okn = |x: f64| -> Result<Arg, Er> { if x.is_finite() { Ok(Arg::V(Value::Num(x))) } else { Err(Er::Num) } };
        let oks = |s: String| -> Result<Arg, Er> { Ok(Arg::V(Value::Str(Rc::from(s)))) };
        let okb = |b: bool| -> Result<Arg, Er> { Ok(Arg::V(Value::Bool(b))) };
        macro_rules! need { ($lo:expr, $hi:expr) => { if n < $lo || n > $hi { return Err(Er::Value); } } }
        // arg present (not placeholder)
        let has = |i: usize| -> bool { i < n && !matches!(args[i], Node::Err(Er::Null)) };
        match name {
            "SUM" => { let mut k = Kahan::new(); self.fold_nums(args, cx, &mut |x| k.add(x), false)?; okn(k.get()) }
            "SUMSQ" => { let mut s = 0.0; self.fold_nums(args, cx, &mut |x| s += x * x, false)?; okn(s) }
            "PRODUCT" => { let mut s = 1.0; let mut k = 0; self.fold_nums(args, cx, &mut |x| { s *= x; k += 1 }, false)?; okn(if k == 0 { 0.0 } else { s }) }
            "COUNT" => { let mut k = 0.0; self.fold_nums(args, cx, &mut |_| k += 1.0, true)?; okn(k) }
            "AVERAGE" => { let mut s = Kahan::new(); let mut k = 0.0; self.fold_nums(args, cx, &mut |x| { s.add(x); k += 1.0 }, false)?; if k == 0.0 { Err(Er::Div0) } else { okn(s.get() / k) } }
            "MAX" | "MIN" => {
                let mut m: Option<(f64, bool)> = None;
                let ismax = name == "MAX";
                let mut upd = |x: f64, b: bool| { match m { None => m = Some((x, b)), Some((y, fb)) => if (ismax && x >= y) || (!ismax && x <= y) { m = Some((x, fb)) } } };
                for ar in self.pre_args(args, cx)? { match ar {
                    Arg::R(s, r1, c1, r2, c2) => {
                        let g = self.clamp((s, r1, c1, r2, c2)); if Self::is_empty_rg(g) { continue; }
                        for r in g.1..=g.3 { for c in g.2..=g.4 { match self.cell(s, r, c) { Value::Bool(b) => upd(if *b { 1.0 } else { 0.0 }, true), Value::Num(x) => upd(*x, false), Value::Err(e) => return Err(*e), _ => {} } } }
                    }
                    Arg::V(v) => match v { Value::Num(x) => upd(x, false), Value::Bool(b) => upd(if b { 1.0 } else { 0.0 }, false), Value::Err(e) => return Err(e), Value::Str(_) => return Err(Er::Value), Value::Empty | Value::EmptyF => upd(0.0, false) },
                } }
                match m { Some((x, true)) if x == 0.0 || x == 1.0 => ok(Value::Bool(x != 0.0)), Some((x, _)) => okn(x), None => okn(0.0) }
            }
            "MEDIAN" => {
                let mut v = self.nums(args, cx)?; if v.is_empty() { return Err(Er::Value); }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let k = v.len(); okn(if k % 2 == 1 { v[k / 2] } else { (v[k / 2 - 1] + v[k / 2]) / 2.0 })
            }
            "VAR" | "STDEV" => {
                let v = self.nums(args, cx)?; if v.len() < 2 { return Err(Er::Div0); }
                let k = v.len() as f64;
                let mean = v.iter().sum::<f64>() / k;
                let ss: f64 = v.iter().map(|x| (x - mean) * (x - mean)).sum();
                let var = ss / (k - 1.0); okn(if name == "VAR" { var } else { var.sqrt() })
            }
            "LARGE" | "SMALL" => {
                need!(2, 2);
                let mut v = self.nums(&args[..1], cx)?;
                let k = self.num(&args[1], cx)?;
                if k < 1.0 { return Err(Er::Value); }
                let k = if name == "LARGE" { k.ceil() } else { k.floor() };
                if k as usize > v.len() { return Err(Er::Value); }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let k = k as usize;
                okn(if name == "LARGE" { v[v.len() - k] } else { v[k - 1] })
            }
            "COUNTA" => {
                let mut k = 0.0;
                for a in args { match self.arg(a, cx) {
                    Arg::V(_) => k += 1.0,
                    Arg::R(s, r1, c1, r2, c2) => { let g = self.clamp((s, r1, c1, r2, c2)); if Self::is_empty_rg(g) { continue; } for r in g.1..=g.3 { for c in g.2..=g.4 { if !matches!(self.cell(s, r, c), Value::Empty) { k += 1.0; } } } }
                } }
                okn(k)
            }
            "COUNTBLANK" => {
                need!(1, 1);
                let g = self.rg(&args[0], cx)?;
                let total = ((g.3 - g.1 + 1) * (g.4 - g.2 + 1)) as f64;
                let cg = self.clamp(g);
                let mut nonblank = 0.0;
                if !Self::is_empty_rg(cg) { for r in cg.1..=cg.3 { for c in cg.2..=cg.4 { match self.cell(g.0, r, c) { Value::Empty => {}, Value::Str(s) if s.is_empty() => {}, _ => nonblank += 1.0 } } } }
                okn(total - nonblank)
            }
            "SUMPRODUCT" => {
                if n == 0 { return Err(Er::Value); }
                let mut rs = Vec::new();
                for a in args { match self.arg(a, cx) { Arg::R(s, a1, b, c, d) => rs.push((s, a1, b, c, d)), Arg::V(Value::Err(e)) => return Err(e), Arg::V(_) => return Err(Er::Value) } }
                let (h, w) = (rs[0].3 - rs[0].1, rs[0].4 - rs[0].2);
                for g in &rs { if g.3 - g.1 != h || g.4 - g.2 != w { return Err(Er::Value); } }
                let mut tot = Kahan::new();
                for r in 0..=h { for c in 0..=w {
                    let mut p = 1.0;
                    for g in &rs { match self.cell(g.0, g.1 + r, g.2 + c) { Value::Num(x) => p *= x, Value::Bool(b) => p *= if *b { 1.0 } else { 0.0 }, Value::Err(e) => return Err(*e), _ => p = 0.0 } }
                    tot.add(p);
                } }
                okn(tot.get())
            }
            "COUNTIF" | "COUNTIFS" | "SUMIF" | "SUMIFS" | "AVERAGEIF" | "AVERAGEIFS" => {
                let single = !name.ends_with('S');
                let kind = &name[..3];
                let (rs, cs, sumr);
                if single {
                    if n < 2 || n > 3 { return Err(Er::Value); }
                    let g = self.rg(&args[0], cx)?;
                    let cv = self.eval(&args[1], cx);
                    let cr = Crit::new(&cv)?;
                    let sg = if kind != "COU" && n == 3 { let s = self.rg(&args[2], cx)?; (s.0, s.1, s.2, s.1 + (g.3 - g.1), s.2 + (g.4 - g.2)) } else { g };
                    rs = vec![g]; cs = vec![cr]; sumr = sg;
                } else if kind == "COU" {
                    if n < 2 || n % 2 != 0 { return Err(Er::Value); }
                    let (a, b) = self.parse_pairs(args, cx)?; rs = a; cs = b; sumr = rs[0];
                } else {
                    if n < 3 || n % 2 != 1 { return Err(Er::Value); }
                    let s = self.rg(&args[0], cx)?;
                    let (a, b) = self.parse_pairs(&args[1..], cx)?; rs = a; cs = b; sumr = s;
                    if s.3 - s.1 != rs[0].3 - rs[0].1 || s.4 - s.2 != rs[0].4 - rs[0].2 { return Err(Er::Value); }
                }
                // clamp ranges to sheet dims: treat out-of-range as empty (cell() handles)
                let mut cnt = 0.0; let mut sum = Kahan::new(); let mut err: Option<Er> = None;
                self.crit_iter(&rs, &cs, |r, c| {
                    cnt += 1.0;
                    if kind != "COU" {
                        match self.cell(sumr.0, sumr.1 + r, sumr.2 + c) { Value::Num(x) => sum.add(*x), Value::Bool(b) => sum.add(if *b { 1.0 } else { 0.0 }), Value::Err(e) => { if err.is_none() { err = Some(*e); } } , _ => { /* non-numeric not counted for avg */ } }
                    }
                });
                if let Some(e) = err { return Err(e); }
                match kind {
                    "COU" => okn(cnt),
                    "SUM" => okn(sum.get()),
                    _ => {
                        // average: count only numeric
                        let mut k = 0.0; let mut s2 = Kahan::new();
                        self.crit_iter(&rs, &cs, |r, c| { match self.cell(sumr.0, sumr.1 + r, sumr.2 + c) { Value::Num(x) => { s2.add(*x); k += 1.0 }, Value::Bool(b) => { s2.add(if *b { 1.0 } else { 0.0 }); k += 1.0 }, _ => {} } });
                        if k == 0.0 { Err(Er::Div0) } else { okn(s2.get() / k) }
                    }
                }
            }
            "IF" => {
                need!(2, 3);
                let c = self.eval(&args[0], cx);
                let b = self.to_bool(&c)?;
                if b { if has(1) { Ok(self.arg(&args[1], cx)) } else { ok(Value::Num(0.0)) } }
                else if n == 3 { if has(2) { Ok(self.arg(&args[2], cx)) } else { ok(Value::Num(0.0)) } } else { okb(false) }
            }
            "IFERROR" => { need!(2, 2); match self.arg(&args[0], cx) { Arg::V(Value::Err(_)) => Ok(self.arg(&args[1], cx)), Arg::R(s, r, c, r2, c2) if r == r2 && c == c2 => { if let Value::Err(_) = self.cell(s, r, c) { Ok(self.arg(&args[1], cx)) } else { Ok(Arg::R(s, r, c, r2, c2)) } } a => Ok(a) } }
            "IFNA" => { need!(2, 2); match self.arg(&args[0], cx) { Arg::V(Value::Err(Er::Na)) => Ok(self.arg(&args[1], cx)), Arg::R(s, r, c, r2, c2) if r == r2 && c == c2 => { if let Value::Err(Er::Na) = self.cell(s, r, c) { Ok(self.arg(&args[1], cx)) } else { Ok(Arg::R(s, r, c, r2, c2)) } } a => Ok(a) } }
            "IFS" => {
                if n < 2 || n % 2 != 0 { return Err(Er::Value); }
                let mut i = 0;
                while i < n { let c = self.eval(&args[i], cx); if self.to_bool(&c)? { return Ok(self.arg(&args[i + 1], cx)); } i += 2; }
                Err(Er::Na)
            }
            "SWITCH" => {
                if n < 3 { return Err(Er::Value); }
                let e = self.eval(&args[0], cx);
                if let Value::Err(x) = e { return Err(x); }
                let mut i = 1;
                while i + 1 < n {
                    let v = self.eval(&args[i], cx);
                    if let Value::Err(_) = v { i += 2; continue; }
                    let eq = match (&e, &v) {
                        (Value::Str(a), Value::Num(b)) | (Value::Num(b), Value::Str(a)) => parse_num_str(a).map_or(false, |x| x == *b),
                        (Value::Empty, Value::Num(_)) | (Value::Num(_), Value::Empty) | (Value::Empty, Value::Bool(_)) | (Value::Bool(_), Value::Empty) => false,
                        _ => self.compare(&e, &v) == Ordering::Equal };
                    if eq { return Ok(self.arg(&args[i + 1], cx)); }
                    i += 2;
                }
                if i < n { Ok(self.arg(&args[i], cx)) } else { Err(Er::Na) }
            }
            "CHOOSE" => {
                if n < 2 { return Err(Er::Value); }
                let k = self.num(&args[0], cx)?.trunc();
                if k < 1.0 || k as usize >= n { return Err(Er::Value); }
                Ok(self.arg(&args[k as usize], cx))
            }
            "AND" | "OR" | "XOR" => {
                let mut acc = name == "AND"; let mut cnt = 0; let mut xr = false;
                let mut upd = |b: bool| { cnt += 1; match name { "AND" => acc &= b, "OR" => acc |= b, _ => xr ^= b } };
                let pre = self.pre_args(args, cx)?;
                for (a, ar) in args.iter().zip(pre) {
                    if let Node::Func(fnm, fa) = a { if fnm == "ISLOGICAL" && fa.len() == 1 && matches!(fa[0], Node::Str(_) | Node::Num(_)) { upd(true); continue; } }
                    match ar {
                        Arg::V(v) => { if let Value::Str(s) = &v { let u = s.to_ascii_uppercase(); if u == "TRUE" { upd(true); continue; } else if u == "FALSE" { upd(false); continue; } else { return Err(Er::Value); } } let b = self.to_bool(&v)?; upd(b); }
                        Arg::R(s, r1, c1, r2, c2) => { let g = self.clamp((s, r1, c1, r2, c2)); if Self::is_empty_rg(g) { continue; } for r in g.1..=g.3 { for c in g.2..=g.4 { match self.cell(s, r, c) { Value::Num(x) => upd(*x != 0.0), Value::Bool(b) => upd(*b), Value::Err(e) => return Err(*e), _ => {} } } } }
                    }
                }
                if cnt == 0 { return Err(Er::Value); }
                okb(if name == "XOR" { xr } else { acc })
            }
            "NOT" => { need!(1, 1); let v = self.eval(&args[0], cx); okb(!self.to_bool(&v)?) }
            "TRUE" => okb(true), "FALSE" => okb(false),
            "PI" => okn(std::f64::consts::PI),
            "NA" => Err(Er::Na),
            "ABS" | "EXP" | "INT" | "SIGN" | "SQRT" | "LN" | "LOG10" => {
                need!(1, 1); let x = self.num(&args[0], cx)?;
                match name { "ABS" => okn(x.abs()), "EXP" => okn(x.exp()), "INT" => okn(x.floor()), "SIGN" => okn(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }),
                    "SQRT" => if x < 0.0 { Err(Er::Value) } else { okn(x.sqrt()) },
                    "LN" => if x <= 0.0 { Err(Er::Value) } else { okn(x.ln()) },
                    _ => if x <= 0.0 { Err(Er::Value) } else { okn(x.log10()) } }
            }
            "LOG" => {
                need!(1, 2);
                let b = if has(1) { self.num(&args[1], cx)? } else { 10.0 };
                let x = self.num(&args[0], cx)?;
                if x <= 0.0 || b <= 0.0 { return Err(Er::Value); }
                if b == 1.0 { return Err(Er::Value); }
                okn(x.ln() / b.ln())
            }
            "POWER" => { need!(2, 2); let b = self.num(&args[1], cx)?; let a = self.num(&args[0], cx)?; match pow(a, b) { Some(v) => okn(v), None => Err(Er::Num) } }
            "MOD" => {
                need!(2, 2);
                let (av, al) = self.eval_l(&args[0], cx, false);
                if let Value::Err(e) = av { if !al { return Err(e); } }
                let b = self.num(&args[1], cx)?;
                if b == 0.0 { return Err(Er::Div0); }
                let a = self.to_num(&av)?;
                let r = a - b * (a / b).floor();
                okn(if approx_eq(r.abs() + 1.0, 1.0) { 0.0 } else { r })
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" => {
                need!(1, 2);
                let d = if has(1) { self.num(&args[1], cx)?.trunc() } else { 0.0 };
                let x = self.num(&args[0], cx)?;
                let d = d.max(-300.0).min(300.0) as i32;
                let r = match name {
                    "ROUND" => round_dec(x, d),
                    _ => {
                        let p = 10f64.powi(d.abs());
                        let y = if d >= 0 { x * p } else { x / p };
                        let y = if y.abs() < 1e15 { format!("{:.14e}", y).parse::<f64>().unwrap_or(y) } else { y };
                        let t = if name == "ROUNDUP" { if y >= 0.0 { y.ceil() } else { y.floor() } } else { y.trunc() };
                        if d >= 0 { t / p } else { t * p }
                    }
                };
                okn(r)
            }
            "LEN" => { need!(1, 1); let s = self.st(&args[0], cx)?; okn(s.chars().count() as f64) }
            "LOWER" => { need!(1, 1); let s = self.st(&args[0], cx)?; oks(s.to_lowercase()) }
            "UPPER" => { need!(1, 1); let s = self.st(&args[0], cx)?; oks(s.to_uppercase()) }
            "PROPER" => {
                need!(1, 1); let s = self.st(&args[0], cx)?;
                let mut o = String::new(); let mut prev = false;
                for c in s.chars() { if c.is_alphabetic() { if prev { o.extend(c.to_lowercase()); } else { o.extend(c.to_uppercase()); } prev = true; } else { o.push(c); prev = false; } }
                oks(o)
            }
            "TRIM" => { need!(1, 1); let s = self.st(&args[0], cx)?; oks(s.split(' ').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")) }
            "EXACT" => { need!(2, 2); let a = self.st(&args[0], cx)?; let b = self.st(&args[1], cx)?; okb(a == b) }
            "LEFT" | "RIGHT" => {
                need!(1, 2); let s = self.st(&args[0], cx)?;
                let k = if has(1) { self.num(&args[1], cx)?.trunc() } else { 1.0 };
                if k < 0.0 { return Err(Er::Value); }
                let cs: Vec<char> = s.chars().collect(); let k = (k as usize).min(cs.len());
                oks(if name == "LEFT" { cs[..k].iter().collect() } else { cs[cs.len() - k..].iter().collect() })
            }
            "MID" => {
                need!(3, 3);
                let k = self.num(&args[2], cx)?.trunc(); let st = self.num(&args[1], cx)?.trunc(); let s = self.st(&args[0], cx)?;
                if st < 1.0 || k < 0.0 { return Err(Er::Value); }
                let cs: Vec<char> = s.chars().collect();
                let a = ((st as usize) - 1).min(cs.len()); let b = (a + k as usize).min(cs.len());
                oks(cs[a..b].iter().collect())
            }
            "REPLACE" => {
                need!(4, 4); let nw = self.st(&args[3], cx)?; let k = self.num(&args[2], cx)?.trunc(); let st = self.num(&args[1], cx)?.trunc(); let s = self.st(&args[0], cx)?;
                if st < 1.0 || k < 0.0 { return Err(Er::Value); }
                let cs: Vec<char> = s.chars().collect();
                let a = ((st as usize) - 1).min(cs.len()); let b = (a + k as usize).min(cs.len());
                let mut o: String = cs[..a].iter().collect(); o.push_str(&nw); o.extend(cs[b..].iter()); oks(o)
            }
            "REPT" => {
                need!(2, 2); let k = self.num(&args[1], cx)?.trunc(); let s = self.st(&args[0], cx)?;
                if k < 0.0 { return Err(Er::Value); }
                if s.len() as f64 * k > 33_000_000.0 { return Err(Er::Value); }
                oks(s.repeat(k as usize))
            }
            "SUBSTITUTE" => {
                need!(3, 4); let s = self.st(&args[0], cx)?; let old = self.st(&args[1], cx)?; let nw = self.st(&args[2], cx)?;
                let inst = if has(3) { let k = self.num(&args[3], cx)?.trunc(); if k < 1.0 { return Err(Er::Value); } k as usize } else { 0 };
                if old.is_empty() { return oks(s.to_string()); }
                if inst == 0 { return oks(s.replace(&*old, &nw)); }
                let mut o = String::new(); let mut cnt = 0; let mut i = 0;
                while let Some(p) = s[i..].find(&*old) {
                    cnt += 1;
                    if cnt == inst { o.push_str(&s[i..i + p]); o.push_str(&nw); i += p + old.len(); o.push_str(&s[i..]); return oks(o); }
                    o.push_str(&s[i..i + p + old.len()]); i += p + old.len();
                }
                oks(s.to_string())
            }
            "FIND" | "SEARCH" => {
                need!(2, 3); let f = self.st(&args[0], cx)?; let w = self.st(&args[1], cx)?;
                let st = if has(2) { self.num(&args[2], cx)?.trunc() } else { 1.0 };
                let wc: Vec<char> = w.chars().collect();
                if st < 1.0 || st as usize > wc.len().max(1) && !(st as usize == 1) { return Err(Er::Value); }
                let st = st as usize - 1;
                if st > wc.len() { return Err(Er::Value); }
                let fc: Vec<char> = f.chars().collect();
                if name == "FIND" {
                    if fc.is_empty() { return Err(Er::Value); }
                    for i in st..wc.len() { if i + fc.len() <= wc.len() && wc[i..i + fc.len()] == fc[..] { return okn((i + 1) as f64); } }
                    Err(Er::Value)
                } else {
                    let fl: Vec<char> = f.to_lowercase().chars().collect(); let wl: Vec<char> = w.to_lowercase().chars().collect();
                    if fl.is_empty() { return Err(Er::Value); }
                    let wild = has_wild(&f.to_lowercase());
                    for i in st..wl.len() {
                        if !wild { if i + fl.len() <= wl.len() && wl[i..i + fl.len()] == fl[..] { return okn((i + 1) as f64); } }
                        else { for j in i..=wl.len() { if wild_match(&fl, &wl[i..j]) { return okn((i + 1) as f64); } } }
                    }
                    Err(Er::Value)
                }
            }
            "CONCATENATE" => {
                let mut vals = Vec::with_capacity(n);
                let mut pending: Option<Er> = None;
                for a in args { let (v, l) = self.eval_l(a, cx, true); if let Value::Err(e) = v { if !l && (pending.is_none() || is_jump_top(a)) { pending = Some(e); } } vals.push(v); }
                if let Some(e) = pending { return Err(e); }
                for v in vals.iter().rev() { if let Value::Err(e) = v { return Err(*e); } }
                let mut o = String::new(); for v in &vals { o.push_str(&self.to_str(v)); } oks(o)
            }
            "CONCAT" => {
                let mut v = Vec::new(); for a in args { self.flat(a, cx, &mut v); }
                let mut o = String::new(); for x in &v { if let Value::Err(e) = x { return Err(*e); } o.push_str(&self.to_str(x)); } oks(o)
            }
            "TEXTJOIN" => {
                if n < 3 { return Err(Er::Value); }
                let d = self.st(&args[0], cx)?; let ie = { let v = self.eval(&args[1], cx); self.to_bool(&v)? };
                let mut v = Vec::new(); for a in &args[2..] { self.flat(a, cx, &mut v); }
                let mut parts = Vec::new();
                for x in &v { if let Value::Err(e) = x { return Err(*e); } let s = self.to_str(x); if ie && s.is_empty() { continue; } parts.push(s); }
                oks(parts.iter().map(|s| &**s).collect::<Vec<_>>().join(&d))
            }
            "VALUE" => { need!(1, 1); let (v, isref) = self.eval_l(&args[0], cx, false); match &v { Value::Bool(_) if !isref => ok(v.clone()), Value::Str(s) => match parse_num_str(s) { Some(x) => okn(x), None => Err(Er::Value) }, _ => okn(self.to_num(&v)?) } }
            "N" => { need!(1, 1); let v = self.eval(&args[0], cx); match v { Value::Num(x) => okn(x), Value::Bool(b) => okn(if b { 1.0 } else { 0.0 }), Value::Err(e) => Err(e), _ => okn(0.0) } }
            "T" => { need!(1, 1); let v = self.eval(&args[0], cx); match v { Value::Str(_) => ok(v), Value::Err(e) => Err(e), _ => oks(String::new()) } }
            "TEXT" => {
                need!(2, 2); let v = self.eval(&args[0], cx); if let Value::Err(e) = v { return Err(e); }
                let f = self.st(&args[1], cx)?;
                match &v { Value::Str(s) => { match parse_num_str(s) { Some(x) => oks(text_format(x, &f)), None => ok(v.clone()) } } _ => { let x = self.to_num(&v)?; oks(text_format(x, &f)) } }
            }
            "ISBLANK" | "ISNUMBER" | "ISTEXT" | "ISLOGICAL" | "ISERROR" | "ISNA" => {
                need!(1, 1);
                let v = match self.arg(&args[0], cx) { Arg::V(v) => v, Arg::R(s, r, c, r2, c2) => if r == r2 && c == c2 { self.cell(s, r, c).clone() } else { Value::Err(Er::Value) } };
                okb(match name { "ISBLANK" => matches!(v, Value::Empty), "ISNUMBER" => matches!(v, Value::Num(_) | Value::Bool(_)), "ISTEXT" => matches!(v, Value::Str(_)),
                    "ISLOGICAL" => matches!(v, Value::Bool(_)), "ISERROR" => matches!(v, Value::Err(_)), _ => matches!(v, Value::Err(Er::Na)) })
            }
            "DATE" => {
                need!(3, 3); let d = self.num(&args[2], cx)?.trunc(); let m = self.num(&args[1], cx)?.trunc(); let y = self.num(&args[0], cx)?.trunc();
                let y = if y >= 0.0 && y < 30.0 { y + 2000.0 } else if y >= 30.0 && y < 100.0 { y + 1900.0 } else { y };
                if y < 0.0 || y > 32767.0 { return Err(Er::Value); }
                let mut yy = y as i64; let mm = m as i64 - 1;
                yy += mm.div_euclid(12); let mo = mm.rem_euclid(12) + 1;
                let s = days_from_civil(yy, mo, 1) + 25569 + d as i64 - 1;
                if s < -115858 { return Err(Er::Value); }
                okn(s as f64)
            }
            "DAY" | "MONTH" | "YEAR" => {
                need!(1, 1); let x = self.num(&args[0], cx)?;
                if x.abs() > 1e12 { return Err(Er::Value); }
                let (y, m, d) = ymd_from_serial(x);
                okn(match name { "DAY" => d, "MONTH" => m, _ => y } as f64)
            }
            "WEEKDAY" => {
                need!(1, 2); let x = self.num(&args[0], cx)?.floor();
                let t = if has(1) { self.num(&args[1], cx)?.trunc() as i64 } else { 1 };
                let w0 = ((x as i64 + 6) % 7 + 7) % 7; // 0=Sunday
                okn(match t { 1 => w0 + 1, 2 => (w0 + 6) % 7 + 1, 3 => (w0 + 6) % 7, 11..=17 => { let start = (t - 11 + 1) % 7; (w0 - start).rem_euclid(7) + 1 } _ => return Err(Er::Value) } as f64)
            }
            "DAYS" => { need!(2, 2); let a = self.num(&args[0], cx)?; let b = self.num(&args[1], cx)?; okn(a - b) }
            "EDATE" | "EOMONTH" => {
                need!(2, 2); let k = self.num(&args[1], cx)?.trunc() as i64; let x = self.num(&args[0], cx)?.trunc();
                let (y, m, d) = ymd_from_serial(x);
                let tot = y * 12 + (m - 1) + k; let ny = tot.div_euclid(12); let nm = tot.rem_euclid(12) + 1;
                let dim = |y: i64, m: i64| -> i64 { days_from_civil(if m == 12 { y + 1 } else { y }, if m == 12 { 1 } else { m + 1 }, 1) - days_from_civil(y, m, 1) };
                let nd = if name == "EDATE" { d.min(dim(ny, nm)) } else { dim(ny, nm) };
                let s = serial_from_ymd(ny, nm, nd);
                okn(s)
            }
            "DATEDIF" => {
                need!(3, 3); let a = self.num(&args[0], cx)?.floor(); let b = self.num(&args[1], cx)?.floor(); let u = self.st(&args[2], cx)?.to_uppercase();
                if a > b { return Err(Er::Value); }
                let (y1, m1, d1) = ymd_from_serial(a); let (y2, m2, d2) = ymd_from_serial(b);
                let mut months = (y2 - y1) * 12 + (m2 - m1); if d2 < d1 { months -= 1; }
                okn(match u.as_str() {
                    "D" => b - a, "M" => months as f64, "Y" => (months / 12) as f64,
                    "MD" => { if d2 >= d1 { (d2 - d1) as f64 } else { let pm = if m2 == 1 { 12 } else { m2 - 1 }; let py = if m2 == 1 { y2 - 1 } else { y2 }; let dim = days_from_civil(if pm == 12 { py + 1 } else { py }, if pm == 12 { 1 } else { pm + 1 }, 1) - days_from_civil(py, pm, 1); (dim - d1 + d2) as f64 } }
                    "YM" => (months % 12) as f64,
                    "YD" => { let mut y = y2; if (m2, d2) < (m1, d1) { y -= 1; } let _ = y; let anniv_y = if (m2, d2) < (m1, d1) { y2 - 1 } else { y2 }; b - serial_from_ymd(anniv_y, m1, d1) }
                    _ => return Err(Er::Value),
                })
            }
            "INDEX" => {
                need!(2, 3); let g = self.rg(&args[0], cx)?;
                let r = self.num(&args[1], cx)?.trunc(); let c = if has(2) { self.num(&args[2], cx)?.trunc() } else { -1.0 };
                let (h, w) = ((g.3 - g.1 + 1) as f64, (g.4 - g.2 + 1) as f64);
                let (r, c) = if n == 2 || !has(2) { if h == 1.0 { (1.0, r) } else { (r, 1.0) } } else { (r, c) };
                if n == 2 && h > 1.0 && w > 1.0 { return Err(Er::Value); }
                if r < 0.0 || c < 0.0 || r == 0.0 || c == 0.0 { return Err(Er::Value); }
                if r > h || c > w { return Err(Er::Ref); }
                let (rr, cc) = (g.1 + r as usize - 1, g.2 + c as usize - 1);
                Ok(Arg::R(g.0, rr, cc, rr, cc))
            }
            "MATCH" => {
                need!(2, 3); let key = self.eval(&args[0], cx); if let Value::Err(e) = key { return Err(e); }
                let g = self.rg(&args[1], cx)?;
                let t = if has(2) { self.num(&args[2], cx)? } else { 1.0 };
                if t.fract() == 0.0 && (t > 1.0 || t < -1.0) { return Err(Er::Value); }
                let (h, w) = (g.3 - g.1 + 1, g.4 - g.2 + 1);
                if h != 1 && w != 1 { return Err(Er::Value); }
                let len = h * w;
                let get = |i: usize| -> Value { if h == 1 { self.cell(g.0, g.1, g.2 + i).clone() } else { self.cell(g.0, g.1 + i, g.2).clone() } };
                if t == 0.0 {
                    for i in 0..len { let v = if h == 1 { self.cell(g.0, g.1, g.2 + i) } else { self.cell(g.0, g.1 + i, g.2) }; if self.lookup_eq(&key, v) { return okn((i + 1) as f64); } }
                    Err(Er::Na)
                } else {
                    match self.bin_search(&key, len, &get, t < 0.0) { Some(i) => okn((i + 1) as f64), None => Err(Er::Na) }
                }
            }
            "VLOOKUP" | "HLOOKUP" => {
                need!(3, 4); let key = self.eval(&args[0], cx); if let Value::Err(e) = key { return Err(e); }
                let g = self.rg(&args[1], cx)?;
                let idx = self.num(&args[2], cx)?.trunc();
                let approx = if has(3) { let v = self.eval(&args[3], cx); self.to_bool(&v)? } else { true };
                let v = name == "VLOOKUP";
                let (h, w) = (g.3 - g.1 + 1, g.4 - g.2 + 1);
                let (len, other) = if v { (h, w) } else { (w, h) };
                if idx < 1.0 { return Err(Er::Value); }
                if idx as usize > other { return Err(Er::Value); }
                let at = |i: usize, k: usize| -> &Value { if v { self.cell(g.0, g.1 + i, g.2 + k) } else { self.cell(g.0, g.1 + k, g.2 + i) } };
                let found = if !approx {
                    let mut f = None;
                    for i in 0..len { if self.lookup_eq(&key, at(i, 0)) { f = Some(i); break; } }
                    f
                } else { self.bin_search(&key, len, &|i| at(i, 0).clone(), false) };
                match found { Some(i) => { let k = idx as usize - 1;
                    let (rr, cc) = if v { (g.1 + i, g.2 + k) } else { (g.1 + k, g.2 + i) };
                    return match self.cell(g.0, rr, cc) { Value::Bool(b) => okn(if *b { 1.0 } else { 0.0 }), Value::EmptyF => ok(Value::Num(0.0)), v => ok(v.clone()) }; } None => Err(Er::Na) }
            }
            "XLOOKUP" => {
                if n < 3 || n > 6 { return Err(Er::Value); }
                let key = self.eval(&args[0], cx); if let Value::Err(e) = key { return Err(e); }
                let g = self.rg(&args[1], cx)?; let rg = self.rg(&args[2], cx)?;
                let mm = if has(4) { self.num(&args[4], cx)?.trunc() as i64 } else { 0 };
                let sm = if has(5) { self.num(&args[5], cx)?.trunc() as i64 } else { 1 };
                let (h, w) = (g.3 - g.1 + 1, g.4 - g.2 + 1);
                if h != 1 && w != 1 { return Err(Er::Value); }
                let len = h * w;
                let vert = w == 1;
                if vert && (rg.3 - rg.1 + 1 != h) || !vert && (rg.4 - rg.2 + 1 != w) { return Err(Er::Value); }
                let at = |i: usize| -> &Value { if vert { self.cell(g.0, g.1 + i, g.2) } else { self.cell(g.0, g.1, g.2 + i) } };
                let order: Vec<usize> = if sm == -1 { (0..len).rev().collect() } else { (0..len).collect() };
                let mut best: Option<usize> = None;
                match mm {
                    0 | 2 => {
                        let kk = if mm == 2 { key.clone() } else { match &key { Value::Str(s) => Value::Str(Rc::from(s.replace('~', "~~").replace('*', "~*").replace('?', "~?"))), k => k.clone() } };
                        for &i in &order { if self.lookup_eq(&kk, at(i)) { best = Some(i); break; } }
                    }
                    -1 | 1 => {
                        // exact or next smaller/larger
                        let mut bv: Option<&Value> = None;
                        for &i in &order {
                            let v = at(i);
                            if Self::typ_rank(v) != Self::typ_rank(&key) || matches!(v, Value::Empty) { continue; }
                            let o = self.compare(v, &key);
                            if o == Ordering::Equal { best = Some(i); break; }
                            if (mm == -1 && o == Ordering::Less) || (mm == 1 && o == Ordering::Greater) {
                                let better = match bv { None => true, Some(b) => { let ob = self.compare(v, b); if mm == -1 { ob == Ordering::Greater } else { ob == Ordering::Less } } };
                                if better { bv = Some(v); best = Some(i); }
                            }
                        }
                    }
                    _ => return Err(Er::Value),
                }
                match best {
                    Some(i) => if vert { Ok(Arg::R(rg.0, rg.1 + i, rg.2, rg.1 + i, rg.4)) } else { Ok(Arg::R(rg.0, rg.1, rg.2 + i, rg.3, rg.2 + i)) },
                    None => if has(3) { Ok(self.arg(&args[3], cx)) } else { Err(Er::Na) },
                }
            }
            _ => Err(Er::Name),
        }
    }
}

pub struct Kahan { s: f64, c: f64 }
impl Kahan {
    pub fn new() -> Kahan { Kahan { s: 0.0, c: 0.0 } }
    #[inline]
    pub fn add(&mut self, x: f64) {
        let t = self.s + x;
        if self.s.abs() >= x.abs() { self.c += (self.s - t) + x; } else { self.c += (x - t) + self.s; }
        self.s = t;
    }
    pub fn get(&self) -> f64 { self.s + self.c }
}

pub fn has_jump(n: &Node) -> bool {
    match n {
        Node::Func(name, args) => matches!(name.as_str(), "IF" | "IFS" | "CHOOSE" | "SWITCH") || args.iter().any(has_jump),
        Node::Bin(_, a, b) => has_jump(a) || has_jump(b),
        Node::Neg(a) | Node::Pct(a) => has_jump(a),
        _ => false,
    }
}

pub fn is_jump_top(n: &Node) -> bool { if let Node::Neg(a) | Node::Pct(a) = n { return is_jump_top(a); } matches!(n, Node::Func(name, _) if matches!(name.as_str(), "IF" | "IFS" | "CHOOSE" | "SWITCH" | "XLOOKUP")) }
