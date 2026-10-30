use crate::fmt::*;
use crate::*;

pub struct Ctx<'a> {
    pub book: &'a Book,
    pub idx: RefCell<HashMap<(usize, u32, u32, u32, u32), Rc<LookupIndex>>>,
    pub crit_cache: RefCell<HashMap<(usize, u32, u32, u32, u32, String), Rc<Vec<u64>>>>,
    pub range_cache: RefCell<HashMap<(usize, u32, u32, u32, u32), Rc<Vec<Val>>>>,
}

pub fn crit_key(v: &Val) -> String {
    match v {
        Val::N(n) => format!("N{}", n.to_bits()),
        Val::S(s) => format!("S{}", s),
        Val::B(b) => format!("B{}", b),
        Val::Empty => "E".to_string(),
        Val::E(e) => format!("X{}", e.code()),
    }
}

pub struct LookupIndex {
    pub nums: HashMap<u64, u32>,
    pub strs: HashMap<String, u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Rng {
    pub sheet: usize,
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
}

impl Rng {
    pub fn rows(&self) -> usize {
        (self.r2 - self.r1 + 1) as usize
    }
    pub fn cols(&self) -> usize {
        (self.c2 - self.c1 + 1) as usize
    }
}

pub enum R {
    V(Val),
    Rg(Rng),
}

pub type Res = Result<Val, Er>;

impl From<Er> for Val {
    fn from(e: Er) -> Val {
        Val::E(e)
    }
}

pub fn str_to_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.eq_ignore_ascii_case("true") {
        return Some(1.0);
    }
    if t.eq_ignore_ascii_case("false") {
        return Some(0.0);
    }
    let mut t = t;
    let mut pct = false;
    if let Some(x) = t.strip_suffix('%') {
        pct = true;
        t = x.trim();
    }
    let mut neg = false;
    if let Some(x) = t.strip_prefix('-') {
        neg = true;
        t = x.trim();
    } else if let Some(x) = t.strip_prefix('+') {
        t = x.trim();
    }
    if let Some(x) = t.strip_prefix('$') {
        t = x.trim();
    }
    let mut cleaned = String::with_capacity(t.len());
    let mut valid = true;
    let mut seen_digit = false;
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            seen_digit = true;
            cleaned.push(c as char);
        } else if c == b',' {
            // thousands separator: must be followed by 3 digits
            if !(seen_digit && i + 3 < b.len() + 0 && b[i + 1..].len() >= 3 && b[i + 1..i + 4].iter().all(|x| x.is_ascii_digit())) {
                valid = false;
                break;
            }
        } else if c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
            cleaned.push(c as char);
        } else {
            valid = false;
            break;
        }
        i += 1;
    }
    let mut v: Option<f64> = None;
    if valid && seen_digit {
        if let Ok(x) = cleaned.parse::<f64>() {
            v = Some(x);
        }
    }
    if v.is_none() && t.contains('-') {
        // d-mmm-yy / d-mmm-yyyy
        let parts: Vec<&str> = t.split('-').collect();
        if parts.len() == 3 {
            const MON: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
            let ml = parts[1].to_ascii_lowercase();
            if let Some(mi) = MON.iter().position(|m| ml.starts_with(m) && (ml.len() == 3 || ml.len() > 3)) {
                if let (Ok(d), Ok(y)) = (parts[0].parse::<u32>(), parts[2].parse::<i64>()) {
                    let y = if parts[2].len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
                    let m = mi as u32 + 1;
                    if d >= 1 && d <= days_in_month(y, m) {
                        v = Some(ymd_to_serial(y, m, d) as f64);
                    }
                }
            }
        }
    }
    if v.is_none() {
        // US date m/d/yyyy
        let parts: Vec<&str> = t.split('/').collect();
        if parts.len() == 3 {
            if let (Ok(m), Ok(d), Ok(y)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>(), parts[2].parse::<i64>()) {
                if m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m) && y >= 1 {
                    v = Some(ymd_to_serial(y, m, d) as f64);
                }
            }
        }
    }
    if v.is_none() {
        // ISO date yyyy-mm-dd
        let parts: Vec<&str> = t.split('-').collect();
        if parts.len() == 3 {
            if let (Ok(y), Ok(m), Ok(d)) = (parts[0].parse::<i64>(), parts[1].parse::<u32>(), parts[2].parse::<u32>()) {
                if parts[0].len() == 4 && m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m) {
                    v = Some(ymd_to_serial(y, m, d) as f64);
                }
            }
        }
    }
    let mut v = v?;
    if neg {
        v = -v;
    }
    if pct {
        v /= 100.0;
    }
    Some(v)
}

pub fn to_num(v: &Val) -> Result<f64, Er> {
    match v {
        Val::N(n) => Ok(*n),
        Val::B(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Val::Empty => Ok(0.0),
        Val::E(e) => Err(*e),
        Val::S(s) => str_to_num(s).ok_or(Er::Value),
    }
}

pub fn to_str(v: &Val) -> Result<Arc<str>, Er> {
    match v {
        Val::S(s) => Ok(s.clone()),
        Val::N(n) => Ok(Arc::from(num_to_string(*n).as_str())),
        Val::B(b) => Ok(Arc::from(if *b { "1" } else { "0" })),
        Val::Empty => Ok(Arc::from("")),
        Val::E(e) => Err(*e),
    }
}

pub fn to_bool(v: &Val) -> Result<bool, Er> {
    match v {
        Val::B(b) => Ok(*b),
        Val::N(n) => Ok(*n != 0.0),
        Val::Empty => Ok(false),
        Val::E(e) => Err(*e),
        Val::S(s) => str_to_num(s).map(|x| x != 0.0).ok_or(Er::Value),
    }
}

fn check(n: f64) -> Res {
    if n.is_nan() {
        Err(Er::Num)
    } else if n.is_infinite() {
        Err(Er::Num)
    } else {
        Ok(Val::N(n))
    }
}

pub fn str_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.chars().flat_map(|c| c.to_lowercase());
    let mut bi = b.chars().flat_map(|c| c.to_lowercase());
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
            }
        }
    }
}

// compare two values: numbers(bool,empty) < strings
pub fn cmp_vals(a: &Val, b: &Val) -> Result<std::cmp::Ordering, Er> {
    use std::cmp::Ordering::*;
    match (a, b) {
        (_, Val::E(e)) => Err(*e),
        (Val::E(e), _) => Err(*e),
        (Val::S(x), Val::S(y)) => Ok(str_cmp(x, y)),
        (Val::S(x), Val::Empty) => Ok(if x.is_empty() { Equal } else { Greater }),
        (Val::Empty, Val::S(y)) => Ok(if y.is_empty() { Equal } else { Less }),
        (Val::S(_), _) => Ok(Greater),
        (_, Val::S(_)) => Ok(Less),
        _ => {
            let x = to_num(a)?;
            let y = to_num(b)?;
            Ok(x.partial_cmp(&y).unwrap_or(Equal))
        }
    }
}

// ---------- criteria ----------
#[derive(Clone, Copy, PartialEq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

pub enum Crit {
    Num(CmpOp, f64, Arc<str>), // raw text for text-cell equality
    Text(CmpOp, String, bool), // pattern, has wildcard
    Empty,                     // "" or "=" matches empty
    NonEmpty,                  // "<>"
    Bool(bool),
}

pub fn parse_crit(v: &Val) -> Crit {
    match v {
        Val::N(n) => Crit::Num(CmpOp::Eq, *n, Arc::from(num_to_string(*n).as_str())),
        Val::B(b) => Crit::Num(CmpOp::Eq, if *b { 1.0 } else { 0.0 }, Arc::from(if *b { "TRUE" } else { "FALSE" })),
        Val::Empty => Crit::Num(CmpOp::Eq, 0.0, Arc::from("")),
        Val::E(_) => Crit::Text(CmpOp::Eq, "\u{0}err".to_string(), false),
        Val::S(s) => {
            let (op, rest) = if let Some(r) = s.strip_prefix("<>") {
                (CmpOp::Ne, r)
            } else if let Some(r) = s.strip_prefix("<=") {
                (CmpOp::Le, r)
            } else if let Some(r) = s.strip_prefix(">=") {
                (CmpOp::Ge, r)
            } else if let Some(r) = s.strip_prefix('<') {
                (CmpOp::Lt, r)
            } else if let Some(r) = s.strip_prefix('>') {
                (CmpOp::Gt, r)
            } else if let Some(r) = s.strip_prefix('=') {
                (CmpOp::Eq, r)
            } else {
                (CmpOp::Eq, &s[..])
            };
            if rest.is_empty() {
                return if op == CmpOp::Ne { Crit::NonEmpty } else { Crit::Empty };
            }
            if let Some(n) = str_to_num(rest) {
                if !rest.trim().is_empty() {
                    return Crit::Num(op, n, Arc::from(rest));
                }
            }
            let wild = rest.contains('*') || rest.contains('?') || rest.contains('~');
            Crit::Text(op, rest.to_string(), wild)
        }
    }
}

pub fn wild_match(pat: &[char], s: &[char]) -> bool {
    let (mut p, mut i) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while i < s.len() {
        if p < pat.len() {
            let pc = pat[p];
            if pc == '~' && p + 1 < pat.len() {
                if eq_ci(pat[p + 1], s[i]) {
                    p += 2;
                    i += 1;
                    continue;
                }
            } else if pc == '*' {
                star = Some((p, i));
                p += 1;
                continue;
            } else if pc == '?' || eq_ci(pc, s[i]) {
                p += 1;
                i += 1;
                continue;
            }
        }
        if let Some((sp, si)) = star {
            p = sp + 1;
            i = si + 1;
            star = Some((sp, si + 1));
            continue;
        }
        return false;
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

#[inline]
fn eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn text_cmp_op(op: CmpOp, ord: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        CmpOp::Eq => ord == Equal,
        CmpOp::Ne => ord != Equal,
        CmpOp::Lt => ord == Less,
        CmpOp::Gt => ord == Greater,
        CmpOp::Le => ord != Greater,
        CmpOp::Ge => ord != Less,
    }
}

pub fn crit_match(c: &Crit, v: &Val) -> bool {
    match c {
        Crit::Empty => matches!(v, Val::Empty) || matches!(v, Val::S(s) if s.is_empty()),
        Crit::NonEmpty => !matches!(v, Val::Empty),
        Crit::Bool(b) => matches!(v, Val::B(x) if x == b),
        Crit::Num(op, n, raw) => match v {
            Val::N(x) => text_cmp_op(*op, x.partial_cmp(n).unwrap_or(std::cmp::Ordering::Equal)),
            Val::B(b) => {
                let x = if *b { 1.0 } else { 0.0 };
                text_cmp_op(*op, x.partial_cmp(n).unwrap_or(std::cmp::Ordering::Equal))
            }
            Val::S(s) => match op {
                CmpOp::Eq => s.eq_ignore_ascii_case(raw),
                CmpOp::Ne => !s.eq_ignore_ascii_case(raw),
                _ => false,
            },
            Val::Empty => *op == CmpOp::Ne,
            Val::E(_) => false,
        },
        Crit::Text(op, pat, wild) => {
            let s: Arc<str> = match v {
                Val::S(s) => s.clone(),
                Val::N(n) => {
                    if *op != CmpOp::Eq && *op != CmpOp::Ne {
                        return false;
                    }
                    Arc::from(num_to_string(*n).as_str())
                }
                Val::B(b) => {
                    if *op != CmpOp::Eq && *op != CmpOp::Ne {
                        return false;
                    }
                    Arc::from(if *b { "TRUE" } else { "FALSE" })
                }
                Val::Empty => return *op == CmpOp::Ne,
                Val::E(_) => return false,
            };
            if *wild && (*op == CmpOp::Eq || *op == CmpOp::Ne) {
                let pc: Vec<char> = pat.chars().collect();
                let sc: Vec<char> = s.chars().collect();
                let m = wild_match(&pc, &sc);
                if *op == CmpOp::Eq {
                    m
                } else {
                    !m
                }
            } else {
                text_cmp_op(*op, str_cmp(&s, pat))
            }
        }
    }
}

impl<'a> Ctx<'a> {
    pub fn formula_value(&self, fi: usize) -> Val {
        let f = &self.book.formulas[fi];
        match f.state.get() {
            2 => return f.val.borrow().clone(),
            1 => return Val::E(Er::Value), // circular
            _ => {}
        }
        f.state.set(1);
        let v = match &f.expr {
            None => Val::E(Er::Name),
            Some(e) => match self.eval(e) {
                Ok(R::V(v)) => v,
                Ok(R::Rg(rg)) => {
                    if rg.r1 == rg.r2 && rg.c1 == rg.c2 {
                        self.cell(rg.sheet, rg.r1, rg.c1)
                    } else {
                        Val::E(Er::Value)
                    }
                }
                Err(e) => Val::E(e),
            },
        };
        let v = match v {
            Val::Empty => Val::N(0.0),
            Val::N(n) if !n.is_finite() => Val::E(Er::Num),
            v => v,
        };
        *f.val.borrow_mut() = v.clone();
        f.state.set(2);
        v
    }

    #[inline]
    pub fn with_cell<T>(&self, sheet: usize, r: u32, c: u32, f: impl FnOnce(&Val) -> T) -> T {
        let sh = &self.book.sheets[sheet];
        match sh.idx(r as usize, c as usize) {
            None => f(&Val::Empty),
            Some(i) => {
                let fi = sh.fidx[i];
                if fi != u32::MAX {
                    let v = self.formula_value(fi as usize);
                    f(&v)
                } else {
                    f(&sh.cells[i])
                }
            }
        }
    }

    // bitset of cells in range matching criterion (cached)
    pub fn crit_bits(&self, rg: &Rng, cv: &Val, crit: &Crit) -> Rc<Vec<u64>> {
        let key = (rg.sheet, rg.r1, rg.c1, rg.r2, rg.c2, crit_key(cv));
        if let Some(b) = self.crit_cache.borrow().get(&key) {
            return b.clone();
        }
        let len = rg.rows() * rg.cols();
        let mut bits = vec![0u64; (len + 63) / 64];
        let mut i = 0usize;
        self.for_each(rg, |v| {
            if crit_match(crit, v) {
                bits[i >> 6] |= 1u64 << (i & 63);
            }
            i += 1;
        });
        let b = Rc::new(bits);
        self.crit_cache.borrow_mut().insert(key, b.clone());
        b
    }

    // exact-match lookup using a cached hash index; returns None if not applicable
    pub fn indexed_lookup(&self, x: &Val, rg: &Rng) -> Option<Option<usize>> {
        let len = rg.rows() * rg.cols();
        if len < 32 {
            return None;
        }
        let key = (rg.sheet, rg.r1, rg.c1, rg.r2, rg.c2);
        let idx = {
            let cache = self.idx.borrow();
            cache.get(&key).cloned()
        };
        let idx = match idx {
            Some(i) => i,
            None => {
                let mut nums: HashMap<u64, u32> = HashMap::with_capacity(len);
                let mut strs: HashMap<String, u32> = HashMap::new();
                let mut i = 0u32;
                self.for_each(rg, |v| {
                    match v {
                        Val::N(n) => {
                            let n = if *n == 0.0 { 0.0 } else { *n };
                            nums.entry(n.to_bits()).or_insert(i);
                        }
                        Val::B(b) => {
                            nums.entry((if *b { 1.0f64 } else { 0.0 }).to_bits()).or_insert(i);
                        }
                        Val::Empty => {
                            nums.entry(0.0f64.to_bits()).or_insert(i);
                        }
                        Val::S(s) => {
                            strs.entry(s.to_lowercase()).or_insert(i);
                        }
                        _ => {}
                    }
                    i += 1;
                });
                let idx = Rc::new(LookupIndex { nums, strs });
                self.idx.borrow_mut().insert(key, idx.clone());
                idx
            }
        };
        match x {
            Val::N(n) => {
                let n = if *n == 0.0 { 0.0 } else { *n };
                Some(idx.nums.get(&n.to_bits()).map(|p| *p as usize))
            }
            Val::B(b) => Some(idx.nums.get(&(if *b { 1.0f64 } else { 0.0 }).to_bits()).map(|p| *p as usize)),
            Val::S(s) => {
                if s.contains('*') || s.contains('?') || s.contains('~') {
                    return None;
                }
                Some(idx.strs.get(&s.to_lowercase()).map(|p| *p as usize))
            }
            _ => None,
        }
    }

    #[inline]
    pub fn cell(&self, sheet: usize, r: u32, c: u32) -> Val {
        let sh = &self.book.sheets[sheet];
        match sh.idx(r as usize, c as usize) {
            None => Val::Empty,
            Some(i) => {
                let fi = sh.fidx[i];
                if fi != u32::MAX {
                    let v = self.formula_value(fi as usize);
                    v
                } else {
                    sh.cells[i].clone()
                }
            }
        }
    }

    pub fn eval(&self, e: &Expr) -> Result<R, Er> {
        Ok(match e {
            Expr::Num(n) => R::V(Val::N(*n)),
            Expr::Str(s) => R::V(Val::S(s.clone())),
            Expr::Bool(b) => R::V(Val::B(*b)),
            Expr::Err(e) => R::V(Val::E(*e)),
            Expr::Ref { sheet, r, c } => R::Rg(Rng { sheet: *sheet, r1: *r, c1: *c, r2: *r, c2: *c }),
            Expr::Range { sheet, r1, c1, r2, c2 } => R::Rg(Rng { sheet: *sheet, r1: *r1, c1: *c1, r2: *r2, c2: *c2 }),
            Expr::Neg(x) => {
                let v = self.scalar(x)?;
                let n = to_num(&v)?;
                R::V(Val::N(-n))
            }
            Expr::Pct(x) => {
                let v = self.scalar(x)?;
                let n = to_num(&v)?;
                R::V(Val::N(n / 100.0))
            }
            Expr::Bin(op, a, b) => {
                let va = self.scalar(a);
                let vb = self.scalar(b);
                if !crate::funcs::ref_like(a) {
                    if let Ok(Val::E(e)) | Err(e) = va {
                        return Err(e);
                    }
                }
                let vb = match vb {
                    Ok(Val::E(e)) | Err(e) => return Err(e),
                    Ok(v) => v,
                };
                let va = match va {
                    Err(e) => return Err(e),
                    Ok(v) => v,
                };
                R::V(self.binop(*op, &va, &vb)?)
            }
            Expr::Call(name, args) => R::V(self.call(name, args)?),
        })
    }

    pub fn binop(&self, op: Op, va: &Val, vb: &Val) -> Res {
        match op {
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow => {
                let y = to_num(vb)?;
                let x = to_num(va)?;
                match op {
                    Op::Add => check(x + y),
                    Op::Sub => check(x - y),
                    Op::Mul => check(x * y),
                    Op::Div => {
                        if y == 0.0 {
                            Err(Er::Div0)
                        } else {
                            check(x / y)
                        }
                    }
                    _ => pow(x, y),
                }
            }
            Op::Cat => {
                let y = to_str(vb)?;
                let x = to_str(va)?;
                let mut s = String::with_capacity(x.len() + y.len());
                s.push_str(&x);
                s.push_str(&y);
                Ok(Val::S(Arc::from(s.as_str())))
            }
            _ => {
                let ord = cmp_vals(va, vb)?;
                use std::cmp::Ordering::*;
                Ok(Val::B(match op {
                    Op::Eq => ord == Equal,
                    Op::Ne => ord != Equal,
                    Op::Lt => ord == Less,
                    Op::Gt => ord == Greater,
                    Op::Le => ord != Greater,
                    _ => ord != Less,
                }))
            }
        }
    }

    // evaluate to scalar value (dereference single cell; multi-cell range -> #VALUE!)
    pub fn scalar(&self, e: &Expr) -> Res {
        match e {
            Expr::Ref { sheet, r, c } => Ok(self.cell(*sheet, *r, *c)),
            _ => match self.eval(e)? {
                R::V(v) => Ok(v),
                R::Rg(rg) => {
                    if rg.r1 == rg.r2 && rg.c1 == rg.c2 {
                        Ok(self.cell(rg.sheet, rg.r1, rg.c1))
                    } else {
                        Err(Er::Value)
                    }
                }
            },
        }
    }

    pub fn num(&self, e: &Expr) -> Result<f64, Er> {
        let v = self.scalar(e)?;
        to_num(&v)
    }
    pub fn string(&self, e: &Expr) -> Result<Arc<str>, Er> {
        let v = self.scalar(e)?;
        to_str(&v)
    }
    pub fn boolean(&self, e: &Expr) -> Result<bool, Er> {
        let v = self.scalar(e)?;
        to_bool(&v)
    }

    pub fn range(&self, e: &Expr) -> Result<Rng, Er> {
        match self.eval(e)? {
            R::Rg(r) => Ok(r),
            R::V(Val::E(e)) => Err(e),
            R::V(_) => Err(Er::Value),
        }
    }

    // iterate all values of a range
    pub fn for_each<F: FnMut(&Val)>(&self, rg: &Rng, mut f: F) {
        let sh = &self.book.sheets[rg.sheet];
        for r in rg.r1..=rg.r2 {
            if r as usize >= sh.nrows {
                for _ in rg.c1..=rg.c2 {
                    f(&Val::Empty);
                }
                continue;
            }
            for c in rg.c1..=rg.c2 {
                let v = self.cell(rg.sheet, r, c);
                f(&v);
            }
        }
    }

    pub fn range_vals(&self, rg: &Rng) -> Rc<Vec<Val>> {
        let len = rg.rows() * rg.cols();
        let key = (rg.sheet, rg.r1, rg.c1, rg.r2, rg.c2);
        if len >= 64 {
            if let Some(v) = self.range_cache.borrow().get(&key) {
                return v.clone();
            }
        }
        let mut v = Vec::with_capacity(len);
        self.for_each(rg, |x| v.push(x.clone()));
        let v = Rc::new(v);
        if len >= 64 {
            self.range_cache.borrow_mut().insert(key, v.clone());
        }
        v
    }

    // collect numbers from args: ranges -> numeric cells only; scalars -> coerce
    pub fn collect_nums(&self, args: &[Expr], out: &mut Vec<f64>) -> Result<(), Er> {
        for a in args {
            match self.eval(a)? {
                R::Rg(rg) => {
                    let mut err = None;
                    self.for_each(&rg, |v| match v {
                        Val::N(n) => out.push(*n),
                        Val::B(b) => out.push(if *b { 1.0 } else { 0.0 }),
                        Val::E(e) => {
                            if err.is_none() {
                                err = Some(*e)
                            }
                        }
                        _ => {}
                    });
                    if let Some(e) = err {
                        return Err(e);
                    }
                }
                R::V(v) => match v {
                    Val::N(n) => out.push(n),
                    Val::B(b) => out.push(if b { 1.0 } else { 0.0 }),
                    Val::E(e) => return Err(e),
                    Val::S(_) => return Err(Er::Value),
                    Val::Empty => {}
                },
            }
        }
        Ok(())
    }
}

#[derive(Default, Clone, Copy)]
pub struct KSum {
    sum: f64,
    err: f64,
}
impl KSum {
    #[inline]
    pub fn add(&mut self, x: f64) {
        let t = self.sum + x;
        if self.sum.abs() >= x.abs() {
            self.err += (self.sum - t) + x;
        } else {
            self.err += (x - t) + self.sum;
        }
        self.sum = t;
    }
    #[inline]
    pub fn get(&self) -> f64 {
        self.sum + self.err
    }
}
pub fn ksum(v: &[f64]) -> f64 {
    let mut k = KSum::default();
    for x in v {
        k.add(*x);
    }
    k.get()
}

pub fn pow(x: f64, y: f64) -> Res {
    if x == 0.0 && y < 0.0 {
        return Err(Er::Num);
    }
    if x < 0.0 && y.fract() != 0.0 {
        // odd root handling as LibreOffice
        let inv = 1.0 / y;
        if (inv - inv.round()).abs() < 1e-9 && (inv.round() as i64) % 2 != 0 {
            return check(-((-x).powf(y)));
        }
        return Err(Er::Num);
    }
    check(x.powf(y))
}
