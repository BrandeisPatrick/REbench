use std::cmp::Ordering;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Val {
    Empty,
    Num(f64),
    Str(Rc<str>),
    Bool(bool),
    Err(ErrK),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrK {
    Div0,
    Value,
    NA,
    Num,
    Ref,
    Name,
    Null,
}

impl ErrK {
    pub fn text(self) -> &'static str {
        match self {
            ErrK::Div0 => "#DIV/0!",
            ErrK::Value => "#VALUE!",
            ErrK::NA => "#N/A",
            ErrK::Num => "#NUM!",
            ErrK::Ref => "#REF!",
            ErrK::Name => "#NAME?",
            ErrK::Null => "#NULL!",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Cat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
pub enum Expr {
    Lit(Val),
    Ref(usize, u32, u32),
    Range(usize, u32, u32, u32, u32),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

pub enum Arg {
    V(Val),
    R(usize, u32, u32, u32, u32),
}

pub enum Slot {
    Empty,
    Val(Val),
    F(usize),
}

pub struct Sheet {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Slot>,
}

pub struct Form {
    pub expr: Rc<Expr>,
    pub state: u8,
    pub cache: Val,
}

pub struct Book {
    pub sheets: Vec<Sheet>,
    pub forms: Vec<Form>,
    pub empty: Val,
}

pub fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if !t.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    t.parse::<f64>().ok()
}

pub fn ksum<I: Iterator<Item = f64>>(it: I) -> f64 {
    let mut sum = 0.0f64;
    let mut corr = 0.0f64;
    for x in it {
        let tmp = x - corr;
        let st = sum + tmp;
        corr = (st - sum) - tmp;
        sum = st;
    }
    sum - corr
}

pub fn fix15(x: f64) -> f64 {
    format!("{:.14e}", x).parse::<f64>().unwrap_or(x)
}

pub fn fmt_num(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    let s = format!("{:.16e}", n);
    let (mant, exps) = s.split_once('e').unwrap();
    let mut exp: i32 = exps.parse().unwrap();
    let neg = mant.starts_with('-');
    let mut digs: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).map(|c| c - b'0').collect();
    if digs.len() > 15 {
        let first = digs[15];
        let rest_nz = digs[16..].iter().any(|d| *d != 0);
        let mut keep: Vec<u8> = digs[..15].to_vec();
        let up = first > 5 || (first == 5 && (rest_nz || keep[14] % 2 == 1));
        if up {
            let mut i = 14;
            loop {
                if keep[i] == 9 {
                    keep[i] = 0;
                    if i == 0 {
                        keep.insert(0, 1);
                        keep.pop();
                        exp += 1;
                        break;
                    }
                    i -= 1;
                } else {
                    keep[i] += 1;
                    break;
                }
            }
        }
        digs = keep;
    }
    let mut ds: String = digs.iter().map(|d| (b'0' + d) as char).collect();
    while ds.len() > 1 && ds.ends_with('0') {
        ds.pop();
    }
    let digits = ds.as_str();
    let body = if exp >= 16 || exp < -9 {
        let mut m = String::new();
        m.push_str(&digits[..1]);
        if digits.len() > 1 {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        format!("{}E{}{:03}", m, if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if exp >= 0 {
        let ip = exp as usize + 1;
        if digits.len() <= ip {
            let mut s = digits.to_string();
            s.push_str(&"0".repeat(ip - digits.len()));
            s
        } else {
            format!("{}.{}", &digits[..ip], &digits[ip..])
        }
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    };
    if neg {
        format!("-{}", body)
    } else {
        body
    }
}

pub fn parse_date_text(s: &str) -> Option<f64> {
    let t = s.trim();
    let parts: Vec<&str> = t.split(|c| c == '/' || c == '-' || c == ' ').filter(|x| !x.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let num = |x: &str| x.parse::<i64>().ok();
    let (y, m, d);
    if let (Some(a), Some(b)) = (num(parts[0]), num(parts[1])) {
        if parts[0].len() == 4 {
            y = a;
            m = b;
            d = num(parts[2])?;
        } else {
            m = a;
            d = b;
            y = num(parts[2])?;
        }
    } else {
        let low = parts[1].to_ascii_lowercase();
        let names = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
        let idx = names.iter().position(|n| low.len() >= 3 && low.starts_with(n))?;
        m = idx as i64 + 1;
        d = num(parts[0])?;
        y = num(parts[2])?;
    }
    let y = if y < 100 {
        if y < 30 { y + 2000 } else { y + 1900 }
    } else {
        y
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(ymd_serial(y, m, d))
}

pub fn to_num(v: &Val) -> Result<f64, ErrK> {
    match v {
        Val::Empty => Ok(0.0),
        Val::Num(n) => Ok(*n),
        Val::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Val::Str(s) => match parse_num(s) {
            Some(n) => Ok(n),
            None => {
                if s.eq_ignore_ascii_case("TRUE") {
                    Ok(1.0)
                } else if s.eq_ignore_ascii_case("FALSE") {
                    Ok(0.0)
                } else {
                    parse_date_text(s).ok_or(ErrK::Value)
                }
            }
        },
        Val::Err(e) => Err(*e),
    }
}

pub fn to_text(v: &Val) -> String {
    match v {
        Val::Empty => String::new(),
        Val::Num(n) => fmt_num(*n),
        Val::Bool(b) => {
            if *b {
                "1".into()
            } else {
                "0".into()
            }
        }
        Val::Str(s) => s.to_string(),
        Val::Err(e) => e.text().to_string(),
    }
}

pub fn to_bool(v: &Val) -> Result<bool, ErrK> {
    match v {
        Val::Empty => Ok(false),
        Val::Num(n) => Ok(*n != 0.0),
        Val::Bool(b) => Ok(*b),
        Val::Str(s) => {
            let u = s.to_ascii_uppercase();
            if u == "TRUE" {
                Ok(true)
            } else if u == "FALSE" {
                Ok(false)
            } else {
                Err(ErrK::Value)
            }
        }
        Val::Err(e) => Err(*e),
    }
}

fn rank(v: &Val) -> u8 {
    match v {
        Val::Num(_) => 0,
        Val::Str(_) => 1,
        Val::Bool(_) => 2,
        _ => 3,
    }
}

fn default_like(v: &Val) -> Val {
    match v {
        Val::Num(_) => Val::Num(0.0),
        Val::Str(_) => Val::Str(Rc::from("")),
        Val::Bool(_) => Val::Bool(false),
        _ => Val::Empty,
    }
}

pub fn cmp_vals(l: &Val, r: &Val) -> Ordering {
    if let Val::Bool(b) = l {
        return cmp_vals(&Val::Num(if *b { 1.0 } else { 0.0 }), r);
    }
    if let Val::Bool(b) = r {
        return cmp_vals(l, &Val::Num(if *b { 1.0 } else { 0.0 }));
    }
    if let Val::Empty = l {
        if let Val::Empty = r {
            return Ordering::Equal;
        }
        return cmp_vals(&default_like(r), r);
    }
    if let Val::Empty = r {
        return cmp_vals(l, &default_like(l));
    }
    let (ra, rb) = (rank(l), rank(r));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (l, r) {
        (Val::Num(a), Val::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Val::Str(a), Val::Str(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (Val::Bool(a), Val::Bool(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

fn num_res(x: f64) -> Val {
    if x.is_finite() {
        Val::Num(if x == 0.0 { 0.0 } else { x })
    } else {
        Val::Err(ErrK::Num)
    }
}

pub fn binop(op: BinOp, l: &Val, r: &Val) -> Val {
    match (l, r) {
        (Val::Err(a), Val::Err(b)) => {
            let pick = match op {
                BinOp::Div => {
                    if *a == ErrK::Value { *a } else if *b == ErrK::Value { *b } else { *b }
                }
                BinOp::Sub | BinOp::Mul => *a,
                _ => {
                    if *b == ErrK::Value { *b } else { *a }
                }
            };
            return Val::Err(pick);
        }
        (Val::Err(a), _) => return Val::Err(*a),
        (_, Val::Err(b)) => return Val::Err(*b),
        _ => {}
    }
    match op {
        BinOp::Cat => {
            let s = format!("{}{}", to_text(l), to_text(r));
            Val::Str(Rc::from(s.as_str()))
        }
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let o = cmp_vals(l, r);
            Val::Bool(match op {
                BinOp::Eq => o == Ordering::Equal,
                BinOp::Ne => o != Ordering::Equal,
                BinOp::Lt => o == Ordering::Less,
                BinOp::Le => o != Ordering::Greater,
                BinOp::Gt => o == Ordering::Greater,
                _ => o != Ordering::Less,
            })
        }
        _ => {
            let a = match to_num(l) {
                Ok(x) => x,
                Err(e) => return Val::Err(e),
            };
            let b = match to_num(r) {
                Ok(x) => x,
                Err(e) => return Val::Err(e),
            };
            let res = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => {
                    if b == 0.0 {
                        return Val::Err(ErrK::Div0);
                    }
                    a / b
                }
                _ => {
                    if a == 0.0 && b < 0.0 {
                        return Val::Err(ErrK::Num);
                    }
                    if a < 0.0 && b.fract() != 0.0 {
                        let inv = 1.0 / b;
                        if (inv - inv.round()).abs() < 1e-9 && (inv.round() as i64).rem_euclid(2) == 1 {
                            return num_res(fix15(-(-a).powf(b)));
                        }
                        return Val::Err(ErrK::Num);
                    }
                    a.powf(b)
                }
            };
            num_res(if res.is_finite() { fix15(res) } else { res })
        }
    }
}

// ---------- dates ----------
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn serial_ymd(s: f64) -> (i64, i64, i64) {
    let n = s.floor() as i64;
    civil_from_days(n - 25569)
}

pub fn ymd_serial(y: i64, m: i64, d: i64) -> f64 {
    (days_from_civil(y, m, d) + 25569) as f64
}

fn dim(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)
}

fn sun1_weekday(serial: f64) -> i64 {
    let n = serial.floor() as i64;
    (n - 1).rem_euclid(7) + 1
}

fn wild(p: &[char], s: &[char]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    match p[0] {
        '*' => (0..=s.len()).any(|k| wild(&p[1..], &s[k..])),
        '?' => !s.is_empty() && wild(&p[1..], &s[1..]),
        '~' if p.len() > 1 => !s.is_empty() && s[0] == p[1] && wild(&p[2..], &s[1..]),
        c => !s.is_empty() && s[0] == c && wild(&p[1..], &s[1..]),
    }
}

pub fn wild_eq(p: &str, s: &str) -> bool {
    let pl: Vec<char> = p.to_lowercase().chars().collect();
    let sl: Vec<char> = s.to_lowercase().chars().collect();
    wild(&pl, &sl)
}

fn has_wild(p: &str) -> bool {
    p.contains('*') || p.contains('?') || p.contains('~')
}

// lookup equality
fn lk_eq(look: &Val, x: &Val, wc: bool) -> bool {
    match (look, x) {
        (Val::Num(a), Val::Num(b)) => a == b,
        (Val::Str(a), Val::Str(b)) => {
            if wc && has_wild(a) {
                wild_eq(a, b)
            } else {
                a.to_lowercase() == b.to_lowercase()
            }
        }
        (Val::Bool(a), Val::Bool(b)) => a == b,
        _ => false,
    }
}

fn lk_cmp(look: &Val, x: &Val) -> Option<Ordering> {
    match (look, x) {
        (Val::Num(a), Val::Num(b)) => a.partial_cmp(b),
        (Val::Str(a), Val::Str(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        (Val::Bool(a), Val::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

// criteria
#[derive(Clone)]
enum CV {
    Num(f64),
    Str(String),
    Bool(bool),
}
#[derive(Clone)]
pub struct Crit {
    op: u8,
    v: CV,
}

fn parse_crit(v: &Val) -> Crit {
    match v {
        Val::Num(n) => Crit { op: 0, v: CV::Num(*n) },
        Val::Bool(b) => Crit { op: 0, v: CV::Num(if *b { 1.0 } else { 0.0 }) },
        Val::Str(s) => {
            let (op, rest): (u8, &str) = if let Some(r) = s.strip_prefix("<=") {
                (3, r)
            } else if let Some(r) = s.strip_prefix(">=") {
                (5, r)
            } else if let Some(r) = s.strip_prefix("<>") {
                (1, r)
            } else if let Some(r) = s.strip_prefix('<') {
                (2, r)
            } else if let Some(r) = s.strip_prefix('>') {
                (4, r)
            } else if let Some(r) = s.strip_prefix('=') {
                (0, r)
            } else {
                (0, &s[..])
            };
            if let Some(n) = parse_num(rest) {
                Crit { op, v: CV::Num(n) }
            } else if rest.eq_ignore_ascii_case("TRUE") || rest.eq_ignore_ascii_case("FALSE") {
                Crit { op, v: CV::Num(if rest.eq_ignore_ascii_case("TRUE") { 1.0 } else { 0.0 }) }
            } else {
                Crit { op, v: CV::Str(rest.to_string()) }
            }
        }
        _ => Crit { op: 0, v: CV::Str(String::new()) },
    }
}

fn crit_eq(c: &Crit, x: &Val) -> bool {
    match &c.v {
        CV::Num(n) => matches!(x, Val::Num(m) if m == n),
        CV::Bool(b) => matches!(x, Val::Bool(m) if m == b),
        CV::Str(p) => {
            if !p.is_empty() && p.is_ascii() && !has_wild(p) {
                return match x {
                    Val::Str(s) => s.eq_ignore_ascii_case(p) && s.is_ascii(),
                    _ => false,
                } || match x {
                    Val::Str(s) => !s.is_ascii() && wild_eq(p, s),
                    _ => false,
                };
            }
            if p.is_empty() {
                match x {
                    Val::Empty => true,
                    Val::Str(s) => s.is_empty(),
                    _ => false,
                }
            } else {
                match x {
                    Val::Str(s) => wild_eq(p, s),
                    _ => false,
                }
            }
        }
    }
}

fn crit_match(c: &Crit, x: &Val) -> bool {
    let tmp;
    let x = match x {
        Val::Bool(b) => {
            tmp = Val::Num(if *b { 1.0 } else { 0.0 });
            &tmp
        }
        _ => x,
    };
    match c.op {
        0 => crit_eq(c, x),
        1 => !crit_eq(c, x),
        _ => {
            let o = match (&c.v, x) {
                (CV::Num(n), Val::Num(m)) => m.partial_cmp(n),
                (CV::Str(p), Val::Str(s)) => Some(s.to_lowercase().cmp(&p.to_lowercase())),
                _ => None,
            };
            match o {
                None => false,
                Some(o) => match c.op {
                    2 => o == Ordering::Less,
                    3 => o != Ordering::Greater,
                    4 => o == Ordering::Greater,
                    _ => o != Ordering::Less,
                },
            }
        }
    }
}

fn excel_round(x: f64, d: i64) -> f64 {
    let p = 10f64.powi(d.unsigned_abs() as i32);
    if d >= 0 {
        let y = fix15(x * p);
        y.round() / p
    } else {
        let y = fix15(x / p);
        y.round() * p
    }
}

fn round_dir(x: f64, d: i64, up: bool) -> f64 {
    let p = 10f64.powi(d.unsigned_abs() as i32);
    let y = if d >= 0 { fix15(x * p) } else { fix15(x / p) };
    let y = if up {
        if d >= 0 { x * p } else { x / p }
    } else if d >= 0 {
        fix15(x * p)
    } else {
        fix15(x / p)
    };
    let t = if up {
        if y == y.trunc() { y } else { y.trunc() + y.signum() }
    } else {
        y.trunc()
    };
    if d >= 0 { t / p } else { t * p }
}

fn text_of_arg(v: &Val) -> Result<String, ErrK> {
    if let Val::Err(e) = v {
        return Err(*e);
    }
    Ok(to_text(v))
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn format_date(x: f64, fmt: &str) -> String {
    let (y, m, d) = serial_ymd(x);
    let secs = ((x - x.floor()) * 86400.0).round() as i64;
    let (hh, mi, ss) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let dow = sun1_weekday(x) as usize - 1;
    let ch: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut prev_h = false;
    while i < ch.len() {
        let c = ch[i];
        if c == '"' {
            i += 1;
            while i < ch.len() && ch[i] != '"' {
                out.push(ch[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        let lc = c.to_ascii_lowercase();
        if matches!(lc, 'y' | 'm' | 'd' | 'h' | 's') {
            let mut k = 0;
            while i + k < ch.len() && ch[i + k].to_ascii_lowercase() == lc {
                k += 1;
            }
            i += k;
            match lc {
                'y' => {
                    if k >= 3 {
                        out.push_str(&format!("{:04}", y))
                    } else {
                        out.push_str(&format!("{:02}", y % 100))
                    }
                    prev_h = false;
                }
                'm' => {
                    if prev_h || (i < ch.len() && ch[i].to_ascii_lowercase() == 's' && k <= 2) {
                        if k >= 2 {
                            out.push_str(&format!("{:02}", mi))
                        } else {
                            out.push_str(&format!("{}", mi))
                        }
                    } else if k >= 4 {
                        out.push_str(MONTHS[(m - 1) as usize])
                    } else if k == 3 {
                        out.push_str(&MONTHS[(m - 1) as usize][..3])
                    } else if k == 2 {
                        out.push_str(&format!("{:02}", m))
                    } else {
                        out.push_str(&format!("{}", m))
                    }
                    prev_h = false;
                }
                'd' => {
                    if k >= 4 {
                        out.push_str(DAYS[dow])
                    } else if k == 3 {
                        out.push_str(&DAYS[dow][..3])
                    } else if k == 2 {
                        out.push_str(&format!("{:02}", d))
                    } else {
                        out.push_str(&format!("{}", d))
                    }
                    prev_h = false;
                }
                'h' => {
                    if k >= 2 {
                        out.push_str(&format!("{:02}", hh))
                    } else {
                        out.push_str(&format!("{}", hh))
                    }
                    prev_h = true;
                }
                _ => {
                    if k >= 2 {
                        out.push_str(&format!("{:02}", ss))
                    } else {
                        out.push_str(&format!("{}", ss))
                    }
                    prev_h = false;
                }
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn group3(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

fn format_number(x: f64, fmt: &str) -> String {
    let pct = fmt.contains('%');
    let x = if pct { x * 100.0 } else { x };
    let is_ph = |c: char| c == '0' || c == '#' || c == '?';
    let first = fmt.chars().position(is_ph);
    let last = fmt.chars().collect::<Vec<char>>().iter().rposition(|c| is_ph(*c));
    let (pre, body, suf) = match (first, last) {
        (Some(f), Some(l)) => (fmt[..f].to_string(), fmt.chars().skip(f).take(l - f + 1).collect::<String>(), fmt.chars().skip(l + 1).collect::<String>()),
        _ => (String::new(), String::new(), fmt.to_string()),
    };
    let (ipart, dpart) = match body.find('.') {
        Some(p) => (body[..p].to_string(), body[p + 1..].to_string()),
        None => (body.clone(), String::new()),
    };
    let decimals = dpart.chars().filter(|c| *c == '0' || *c == '#' || *c == '?').count();
    let min_int = ipart.chars().filter(|c| *c == '0').count();
    let grouping = ipart.contains(',');
    let r = excel_round(x.abs(), decimals as i64);
    let s = format!("{:.*}", decimals, r);
    let (ip, fp) = match s.find('.') {
        Some(p) => (s[..p].to_string(), s[p + 1..].to_string()),
        None => (s.clone(), String::new()),
    };
    let mut ip = if ip == "0" && min_int == 0 { String::new() } else { ip };
    while ip.len() < min_int {
        ip.insert(0, '0');
    }
    if grouping {
        ip = group3(&ip);
    }
    let mut num = ip;
    if decimals > 0 {
        let mut f = fp;
        // trim optional '#' digits
        let req = dpart.chars().filter(|c| *c == '0').count();
        while f.len() > req && f.ends_with('0') {
            f.pop();
        }
        if !f.is_empty() {
            num.push('.');
            num.push_str(&f);
        }
    }
    if num.is_empty() {
        num = "0".into();
    }
    let neg = x < 0.0 && r != 0.0;
    format!("{}{}{}{}", if neg { "-" } else { "" }, pre.replace('"', ""), num, suf.replace('"', ""))
}

fn format_sci(x: f64, fmt: &str, pos: usize) -> String {
    let mant_fmt = &fmt[..pos];
    let e_char = fmt[pos..pos + 1].to_string();
    let rest = &fmt[pos + 1..];
    let plus = rest.starts_with('+');
    let ez = rest.chars().skip(1).filter(|c| *c == '0').count().max(1);
    let mut e: i64 = if x == 0.0 { 0 } else { x.abs().log10().floor() as i64 };
    let mut m = if x == 0.0 { 0.0 } else { x / 10f64.powi(e as i32) };
    let dec = mant_fmt.split('.').nth(1).map(|d| d.chars().filter(|c| *c == '0' || *c == '#').count()).unwrap_or(0);
    if excel_round(m.abs(), dec as i64) >= 10.0 {
        e += 1;
        m /= 10.0;
    }
    let ms = format_number(m, mant_fmt);
    let sign = if e < 0 { "-" } else if plus { "+" } else { "" };
    format!("{}{}{}{:0w$}", ms, e_char, sign, e.abs(), w = ez)
}

pub fn text_fmt(x: f64, fmt: &str) -> String {
    let secs: Vec<&str> = fmt.split(';').collect();
    let (sec, x) = if x < 0.0 && secs.len() >= 2 { (secs[1], -x) } else { (secs[0], x) };
    if let Some(p) = sec.find("E+").or_else(|| sec.find("E-")) {
        return format_sci(x, sec, p);
    }
    let has_date = {
        let mut in_q = false;
        let mut f = false;
        for c in sec.chars() {
            if c == '"' {
                in_q = !in_q;
            } else if !in_q && matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's') {
                f = true;
            }
        }
        f
    };
    if has_date {
        if x >= 2958466.0 || x <= -719529.0 {
            return "#FMT".to_string();
        }
        format_date(x, sec)
    } else {
        format_number(x, sec)
    }
}

fn cells_of(a: &Arg) -> Option<(usize, u32, u32, u32, u32)> {
    if let Arg::R(s, r1, c1, r2, c2) = a {
        Some((*s, *r1, *c1, *r2, *c2))
    } else {
        None
    }
}

fn rng_dims(t: (usize, u32, u32, u32, u32)) -> (u32, u32) {
    (t.3 - t.1 + 1, t.4 - t.2 + 1)
}

fn rng_at(t: (usize, u32, u32, u32, u32), k: usize) -> (usize, u32, u32) {
    let w = (t.4 - t.2 + 1) as usize;
    (t.0, t.1 + (k / w) as u32, t.2 + (k % w) as u32)
}

fn rng_len(t: (usize, u32, u32, u32, u32)) -> usize {
    let (h, w) = rng_dims(t);
    (h as usize) * (w as usize)
}

impl Book {
    pub fn new(sheets: Vec<Sheet>, forms: Vec<Form>) -> Book {
        Book { sheets, forms, empty: Val::Empty }
    }

    pub fn pk(&self, s: usize, r: u32, c: u32) -> &Val {
        let sh = &self.sheets[s];
        let (r, c) = (r as usize, c as usize);
        if r >= sh.rows || c >= sh.cols {
            return &self.empty;
        }
        match &sh.cells[r * sh.cols + c] {
            Slot::Empty => &self.empty,
            Slot::Val(v) => v,
            Slot::F(id) => &self.forms[*id].cache,
        }
    }

    pub fn cell(&mut self, s: usize, r: u32, c: u32) -> Val {
        let sh = &self.sheets[s];
        let (ru, cu) = (r as usize, c as usize);
        if ru >= sh.rows || cu >= sh.cols {
            return Val::Empty;
        }
        let id = match &sh.cells[ru * sh.cols + cu] {
            Slot::Empty => return Val::Empty,
            Slot::Val(v) => return v.clone(),
            Slot::F(id) => *id,
        };
        self.form_val(id)
    }

    pub fn prep(&mut self, s: usize, r1: u32, c1: u32, r2: u32, c2: u32) {
        let (rows, cols) = (self.sheets[s].rows, self.sheets[s].cols);
        if rows == 0 || cols == 0 {
            return;
        }
        let r2 = (r2 as usize).min(rows - 1);
        let c2 = (c2 as usize).min(cols - 1);
        for r in (r1 as usize)..=r2 {
            for c in (c1 as usize)..=c2 {
                let id = match &self.sheets[s].cells[r * cols + c] {
                    Slot::F(id) => *id,
                    _ => continue,
                };
                self.form_val(id);
            }
        }
    }

    pub fn form_val(&mut self, id: usize) -> Val {
        match self.forms[id].state {
            2 => return self.forms[id].cache.clone(),
            1 => return Val::Err(ErrK::Ref),
            _ => {}
        }
        self.forms[id].state = 1;
        let e = self.forms[id].expr.clone();
        let a = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.eval(&e))) {
            Ok(a) => a,
            Err(_) => Arg::V(Val::Err(ErrK::Value)),
        };
        let v = match a {
            Arg::V(v) => v,
            Arg::R(s, r1, c1, r2, c2) => {
                if r1 == r2 && c1 == c2 {
                    self.pk(s, r1, c1).clone()
                } else {
                    Val::Err(ErrK::Value)
                }
            }
        };
        let v = match v {
            Val::Empty => Val::Num(0.0),
            x => x,
        };
        let f = &mut self.forms[id];
        f.cache = v.clone();
        f.state = 2;
        v
    }

    pub fn eval(&mut self, e: &Expr) -> Arg {
        match e {
            Expr::Lit(v) => Arg::V(v.clone()),
            Expr::Ref(s, r, c) => {
                self.prep(*s, *r, *c, *r, *c);
                Arg::R(*s, *r, *c, *r, *c)
            }
            Expr::Range(s, r1, c1, r2, c2) => {
                self.prep(*s, *r1, *c1, *r2, *c2);
                Arg::R(*s, *r1, *c1, *r2, *c2)
            }
            Expr::Neg(x) => {
                let v = self.eval_s(x);
                Arg::V(match to_num(&v) {
                    Ok(n) => num_res(-n),
                    Err(e) => Val::Err(e),
                })
            }
            Expr::Pct(x) => {
                let v = self.eval_s(x);
                Arg::V(match to_num(&v) {
                    Ok(n) => num_res(n / 100.0),
                    Err(e) => Val::Err(e),
                })
            }
            Expr::Bin(op, a, b) => {
                let l = self.eval_s(a);
                let r = self.eval_s(b);
                Arg::V(binop(*op, &l, &r))
            }
            Expr::Call(name, args) => {
                let mut av = Vec::with_capacity(args.len());
                for x in args {
                    av.push(self.eval(x));
                }
                let r = self.call(name, &av);
                match (name.as_str(), r) {
                    ("VLOOKUP" | "HLOOKUP" | "INDEX" | "XLOOKUP", Val::Bool(b)) => {
                        Arg::V(Val::Num(if b { 1.0 } else { 0.0 }))
                    }
                    (_, r) => Arg::V(r),
                }
            }
        }
    }

    pub fn eval_s(&mut self, e: &Expr) -> Val {
        match self.eval(e) {
            Arg::V(v) => v,
            Arg::R(s, r1, c1, r2, c2) => {
                if r1 == r2 && c1 == c2 {
                    self.pk(s, r1, c1).clone()
                } else {
                    Val::Err(ErrK::Value)
                }
            }
        }
    }

    fn sv(&self, a: &Arg) -> Val {
        match a {
            Arg::V(v) => v.clone(),
            Arg::R(s, r1, c1, r2, c2) => {
                if r1 == r2 && c1 == c2 {
                    self.pk(*s, *r1, *c1).clone()
                } else {
                    Val::Err(ErrK::Value)
                }
            }
        }
    }

    // numeric aggregate semantics
    fn nums(&self, a: &[Arg]) -> Result<Vec<f64>, ErrK> {
        let mut out = Vec::new();
        for x in a {
            match x {
                Arg::V(v) => match v {
                    Val::Empty => {}
                    Val::Num(n) => out.push(*n),
                    Val::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
                    Val::Str(_) => return Err(ErrK::Value),
                    Val::Err(e) => return Err(*e),
                },
                Arg::R(s, r1, c1, r2, c2) => {
                    for r in *r1..=*r2 {
                        for c in *c1..=*c2 {
                            match self.pk(*s, r, c) {
                                Val::Num(n) => out.push(*n),
                                Val::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
                                Val::Err(e) => return Err(*e),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn nv(&self, a: &Arg) -> Result<f64, ErrK> {
        to_num(&self.sv(a))
    }

    fn tv(&self, a: &Arg) -> Result<String, ErrK> {
        text_of_arg(&self.sv(a))
    }

    // all values of a range arg (or single scalar) in row-major order
    fn vals<'a>(&'a self, a: &Arg) -> Vec<&'a Val> {
        match a {
            Arg::V(_) => Vec::new(),
            Arg::R(s, r1, c1, r2, c2) => {
                let mut v = Vec::new();
                for r in *r1..=*r2 {
                    for c in *c1..=*c2 {
                        v.push(self.pk(*s, r, c));
                    }
                }
                v
            }
        }
    }

    fn crit_pairs(&self, a: &[Arg], start: usize) -> Result<Vec<((usize, u32, u32, u32, u32), Crit)>, ErrK> {
        let mut out = Vec::new();
        let mut i = start;
        while i + 1 < a.len() {
            let t = cells_of(&a[i]).ok_or(ErrK::Value)?;
            let cv = self.sv(&a[i + 1]);
            if let Val::Err(e) = cv {
                return Err(e);
            }
            let c = parse_crit(&cv);
            out.push((t, c));
            i += 2;
        }
        Ok(out)
    }

    // returns list of matched offsets for criteria pairs
    fn matches(&self, pairs: &[((usize, u32, u32, u32, u32), Crit)], n: usize) -> Vec<usize> {
        let mut res = Vec::new();
        for k in 0..n {
            let mut ok = true;
            for (t, c) in pairs {
                let (s, r, cc) = rng_at(*t, k);
                if !crit_match(c, self.pk(s, r, cc)) {
                    ok = false;
                    break;
                }
            }
            if ok {
                res.push(k);
            }
        }
        res
    }

    fn lookup_seq<'a>(&'a self, a: &Arg) -> Vec<&'a Val> {
        self.vals(a)
    }

    pub fn call(&self, name: &str, a: &[Arg]) -> Val {
        macro_rules! ga {
            ($i:expr) => {
                match a.get($i) {
                    Some(x) => x,
                    None => return Val::Err(ErrK::Value),
                }
            };
        }
        macro_rules! n1 {
            ($i:expr) => {
                match self.nv(ga!($i)) {
                    Ok(x) => x,
                    Err(e) => return Val::Err(e),
                }
            };
        }
        macro_rules! s1 {
            ($i:expr) => {
                match self.tv(ga!($i)) {
                    Ok(x) => x,
                    Err(e) => return Val::Err(e),
                }
            };
        }
        macro_rules! v1 {
            ($i:expr) => {
                match self.sv(ga!($i)) {
                    Val::Err(e) => return Val::Err(e),
                    v => v,
                }
            };
        }
        macro_rules! b1 {
            ($i:expr) => {
                match to_bool(&self.sv(ga!($i))) {
                    Ok(x) => x,
                    Err(e) => return Val::Err(e),
                }
            };
        }
        let na = a.len();
        let _ = na;
        match name {
            "SUM" => match self.nums(a) {
                Ok(v) => num_res(ksum(v.iter().cloned())),
                Err(e) => Val::Err(e),
            },
            "PRODUCT" => match self.nums(a) {
                Ok(v) => {
                    if v.is_empty() {
                        Val::Num(0.0)
                    } else {
                        num_res(v.iter().product())
                    }
                }
                Err(e) => Val::Err(e),
            },
            "SUMSQ" => match self.nums(a) {
                Ok(v) => num_res(v.iter().map(|x| x * x).sum()),
                Err(e) => Val::Err(e),
            },
            "AVERAGE" => match self.nums(a) {
                Ok(v) => {
                    if v.is_empty() {
                        Val::Err(ErrK::Num)
                    } else {
                        num_res(ksum(v.iter().cloned()) / v.len() as f64)
                    }
                }
                Err(e) => Val::Err(e),
            },
            "MIN" | "MAX" => match self.nums(a) {
                Ok(v) => {
                    let mut any_num = false;
                    let mut any_bool = false;
                    for x in a {
                        match x {
                            Arg::V(Val::Num(_)) => any_num = true,
                            Arg::V(Val::Bool(_)) => any_bool = true,
                            Arg::R(s2, r1, c1, r2, c2) => {
                                for r in *r1..=*r2 {
                                    for c in *c1..=*c2 {
                                        match self.pk(*s2, r, c) {
                                            Val::Num(_) => any_num = true,
                                            Val::Bool(_) => any_bool = true,
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if v.is_empty() {
                        Val::Num(0.0)
                    } else if !any_num && any_bool {
                        let r = if name == "MIN" { v.iter().cloned().fold(f64::INFINITY, f64::min) } else { v.iter().cloned().fold(f64::NEG_INFINITY, f64::max) };
                        Val::Bool(r != 0.0)
                    } else if name == "MIN" {
                        num_res(v.iter().cloned().fold(f64::INFINITY, f64::min))
                    } else {
                        num_res(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
                    }
                }
                Err(e) => Val::Err(e),
            },
            "MEDIAN" => match self.nums(a) {
                Ok(mut v) => {
                    if v.is_empty() {
                        return Val::Err(ErrK::Num);
                    }
                    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
                    let n = v.len();
                    num_res(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
                }
                Err(e) => Val::Err(e),
            },
            "LARGE" | "SMALL" => {
                let mut v = match self.nums(&a[..a.len() - 1]) {
                    Ok(v) => v,
                    Err(e) => return Val::Err(e),
                };
                let k = n1!(a.len() - 1);
                v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal));
                let k = k.trunc();
                if k < 1.0 || k as usize > v.len() {
                    return Val::Err(ErrK::Num);
                }
                let k = k as usize;
                if name == "LARGE" {
                    Val::Num(v[v.len() - k])
                } else {
                    Val::Num(v[k - 1])
                }
            }
            "VAR" | "STDEV" => match self.nums(a) {
                Ok(v) => {
                    if v.len() < 2 {
                        return Val::Err(ErrK::Div0);
                    }
                    let n = v.len() as f64;
                    let m = v.iter().sum::<f64>() / n;
                    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0);
                    num_res(if name == "VAR" { var } else { var.sqrt() })
                }
                Err(e) => Val::Err(e),
            },
            "COUNT" => {
                let mut c = 0usize;
                for x in a {
                    match x {
                        Arg::V(v) => match v {
                            Val::Num(_) | Val::Bool(_) => c += 1,
                            Val::Str(s) => {
                                if parse_num(s).is_some() {
                                    c += 1
                                }
                            }
                            _ => {}
                        },
                        Arg::R(..) => {
                            for v in self.vals(x) {
                                if let Val::Num(_) | Val::Bool(_) = v {
                                    c += 1;
                                }
                            }
                        }
                    }
                }
                Val::Num(c as f64)
            }
            "COUNTA" => {
                let mut c = 0usize;
                for x in a {
                    match x {
                        Arg::V(v) => {
                            if !matches!(v, Val::Empty) {
                                c += 1
                            }
                        }
                        Arg::R(..) => {
                            for v in self.vals(x) {
                                if !matches!(v, Val::Empty) {
                                    c += 1;
                                }
                            }
                        }
                    }
                }
                Val::Num(c as f64)
            }
            "COUNTBLANK" => {
                let mut c = 0usize;
                for v in self.vals(ga!(0)) {
                    match v {
                        Val::Empty => c += 1,
                        Val::Str(s) if s.is_empty() => c += 1,
                        _ => {}
                    }
                }
                Val::Num(c as f64)
            }
            "COUNTIF" => {
                let t = match cells_of(ga!(0)) {
                    Some(t) => t,
                    None => return Val::Err(ErrK::Value),
                };
                let c = parse_crit(&v1!(1));
                let n = rng_len(t);
                let mut cnt = 0;
                for k in 0..n {
                    let (s, r, cc) = rng_at(t, k);
                    if crit_match(&c, self.pk(s, r, cc)) {
                        cnt += 1;
                    }
                }
                Val::Num(cnt as f64)
            }
            "COUNTIFS" => {
                let pairs = match self.crit_pairs(a, 0) {
                    Ok(p) => p,
                    Err(e) => return Val::Err(e),
                };
                if pairs.is_empty() {
                    return Val::Err(ErrK::Value);
                }
                let n = rng_len(pairs[0].0);
                Val::Num(self.matches(&pairs, n).len() as f64)
            }
            "SUMIF" | "AVERAGEIF" => {
                let t = match cells_of(ga!(0)) {
                    Some(t) => t,
                    None => return Val::Err(ErrK::Value),
                };
                let st = if a.len() > 2 { cells_of(&a[2]) } else { Some(t) };
                let st = match st {
                    Some(s) => s,
                    None => return Val::Err(ErrK::Value),
                };
                let c = parse_crit(&v1!(1));
                let n = rng_len(t);
                let mut sumv: Vec<f64> = Vec::new();
                for k in 0..n {
                    let (s, r, cc) = rng_at(t, k);
                    if crit_match(&c, self.pk(s, r, cc)) {
                        let (s2, r2, c2) = rng_at(st, k);
                        match self.pk(s2, r2, c2) {
                            Val::Num(x) => sumv.push(*x),
                            Val::Err(e) => return Val::Err(*e),
                            _ => {}
                        }
                    }
                }
                let cnt = sumv.len();
                let sum = ksum(sumv.into_iter());
                if name == "SUMIF" {
                    num_res(sum)
                } else if cnt == 0 {
                    Val::Err(ErrK::Div0)
                } else {
                    num_res(sum / cnt as f64)
                }
            }
            "SUMIFS" | "AVERAGEIFS" => {
                let st = match cells_of(ga!(0)) {
                    Some(s) => s,
                    None => return Val::Err(ErrK::Value),
                };
                let pairs = match self.crit_pairs(a, 1) {
                    Ok(p) => p,
                    Err(e) => return Val::Err(e),
                };
                if pairs.is_empty() {
                    return Val::Err(ErrK::Value);
                }
                let n = rng_len(st);
                let mut sumv: Vec<f64> = Vec::new();
                for k in self.matches(&pairs, n) {
                    let (s2, r2, c2) = rng_at(st, k);
                    match self.pk(s2, r2, c2) {
                        Val::Num(x) => sumv.push(*x),
                        Val::Err(e) => return Val::Err(*e),
                        _ => {}
                    }
                }
                let cnt = sumv.len();
                let sum = ksum(sumv.into_iter());
                if name == "SUMIFS" {
                    num_res(sum)
                } else if cnt == 0 {
                    Val::Err(ErrK::Div0)
                } else {
                    num_res(sum / cnt as f64)
                }
            }
            "SUMPRODUCT" => {
                let mut arrs: Vec<Vec<&Val>> = Vec::new();
                for x in a {
                    arrs.push(self.vals(x));
                }
                if arrs.is_empty() {
                    return Val::Err(ErrK::Value);
                }
                let n = arrs[0].len();
                if arrs.iter().any(|v| v.len() != n) {
                    return Val::Err(ErrK::Value);
                }
                let mut prods: Vec<f64> = Vec::with_capacity(n);
                for i in 0..n {
                    let mut p = 1.0;
                    for arr in &arrs {
                        match arr[i] {
                            Val::Num(x) => p *= *x,
                            Val::Bool(b) => p *= if *b { 1.0 } else { 0.0 },
                            Val::Err(e) => return Val::Err(*e),
                            _ => p = 0.0,
                        }
                    }
                    prods.push(p);
                }
                num_res(ksum(prods.into_iter()))
            }
            "SUM_" => Val::Empty,
            // ---- logical ----
            "IF" => {
                let c = match to_bool(&v1!(0)) {
                    Ok(x) => x,
                    Err(e) => return Val::Err(e),
                };
                if c {
                    match a.get(1) {
                        Some(x) => self.sv(x),
                        None => Val::Bool(true),
                    }
                } else {
                    match a.get(2) {
                        Some(x) => self.sv(x),
                        None => Val::Bool(false),
                    }
                }
            }
            "IFERROR" | "IFNA" => {
                let v = self.sv(ga!(0));
                let hit = match &v {
                    Val::Err(e) => name == "IFERROR" || *e == ErrK::NA,
                    _ => false,
                };
                if hit {
                    self.sv(ga!(1))
                } else {
                    v
                }
            }
            "IFS" => {
                let mut i = 0;
                while i + 1 < na {
                    match to_bool(&self.sv(&a[i])) {
                        Ok(true) => return self.sv(&a[i + 1]),
                        Ok(false) => {}
                        Err(e) => return Val::Err(e),
                    }
                    i += 2;
                }
                Val::Err(ErrK::NA)
            }
            "SWITCH" => {
                let v = v1!(0);
                let mut i = 1;
                while i + 1 < na {
                    let cand = self.sv(&a[i]);
                    if cmp_vals(&v, &cand) == Ordering::Equal && rank(&v) == rank(&cand) {
                        return self.sv(&a[i + 1]);
                    }
                    i += 2;
                }
                if na % 2 == 0 {
                    self.sv(&a[na - 1])
                } else {
                    Val::Err(ErrK::NA)
                }
            }
            "CHOOSE" => {
                let i = n1!(0).trunc();
                if i < 1.0 || i as usize >= na {
                    return Val::Err(ErrK::Value);
                }
                self.sv(&a[i as usize])
            }
            "AND" | "OR" | "XOR" => {
                let mut cnt = 0usize;
                let mut trues = 0usize;
                for x in a {
                    match x {
                        Arg::V(v) => match v {
                            Val::Empty => {}
                            Val::Err(e) => return Val::Err(*e),
                            _ => match to_bool(v) {
                                Ok(b) => {
                                    cnt += 1;
                                    if b {
                                        trues += 1;
                                    }
                                }
                                Err(e) => return Val::Err(e),
                            },
                        },
                        Arg::R(..) => {
                            for v in self.vals(x) {
                                match v {
                                    Val::Num(n) => {
                                        cnt += 1;
                                        if *n != 0.0 {
                                            trues += 1;
                                        }
                                    }
                                    Val::Bool(b) => {
                                        cnt += 1;
                                        if *b {
                                            trues += 1;
                                        }
                                    }
                                    Val::Err(e) => return Val::Err(*e),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if cnt == 0 {
                    return Val::Err(ErrK::Value);
                }
                Val::Bool(match name {
                    "AND" => trues == cnt,
                    "OR" => trues > 0,
                    _ => trues % 2 == 1,
                })
            }
            "NOT" => Val::Bool(!b1!(0)),
            "TRUE" => Val::Bool(true),
            "FALSE" => Val::Bool(false),
            "ISBLANK" => Val::Bool(matches!(self.sv(ga!(0)), Val::Empty)),
            "ISERROR" => Val::Bool(matches!(self.sv(ga!(0)), Val::Err(_))),
            "ISNA" => Val::Bool(matches!(self.sv(ga!(0)), Val::Err(ErrK::NA))),
            "ISNUMBER" => Val::Bool(matches!(self.sv(ga!(0)), Val::Num(_) | Val::Bool(_))),
            "ISTEXT" => Val::Bool(matches!(self.sv(ga!(0)), Val::Str(_))),
            "ISLOGICAL" => Val::Bool(matches!(self.sv(ga!(0)), Val::Bool(_))),
            // ---- math ----
            "ABS" => num_res(n1!(0).abs()),
            "INT" => num_res(n1!(0).floor()),
            "SIGN" => {
                let x = n1!(0);
                Val::Num(if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 })
            }
            "SQRT" => {
                let x = n1!(0);
                if x < 0.0 {
                    Val::Err(ErrK::Value)
                } else {
                    num_res(x.sqrt())
                }
            }
            "EXP" => num_res(n1!(0).exp()),
            "LN" => {
                let x = n1!(0);
                if x <= 0.0 {
                    Val::Err(ErrK::Value)
                } else {
                    num_res(x.ln())
                }
            }
            "LOG10" => {
                let x = n1!(0);
                if x <= 0.0 {
                    Val::Err(ErrK::Value)
                } else {
                    num_res(x.log10())
                }
            }
            "LOG" => {
                let x = n1!(0);
                let b = if na > 1 { n1!(1) } else { 10.0 };
                if x <= 0.0 || b <= 0.0 || b == 1.0 {
                    return Val::Err(ErrK::Value);
                }
                if b == 10.0 {
                    num_res(x.log10())
                } else if b == 2.0 {
                    num_res(x.log2())
                } else {
                    num_res(x.ln() / b.ln())
                }
            }
            "PI" => Val::Num(std::f64::consts::PI),
            "POWER" => {
                let x = n1!(0);
                let y = n1!(1);
                binop(BinOp::Pow, &Val::Num(x), &Val::Num(y))
            }
            "MOD" => {
                let x = n1!(0);
                let d = n1!(1);
                if d == 0.0 {
                    return Val::Err(ErrK::Div0);
                }
                let q = fix15(x / d).floor();
                let r = x - d * q;
                if r.abs() < x.abs().max(d.abs()) * 1e-15 {
                    Val::Num(0.0)
                } else {
                    num_res(r)
                }
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" => {
                let x = n1!(0);
                let d = if na > 1 { n1!(1).trunc() as i64 } else { 0 };
                match name {
                    "ROUND" => num_res(excel_round(x, d)),
                    "ROUNDUP" => num_res(round_dir(x, d, true)),
                    _ => num_res(round_dir(x, d, false)),
                }
            }
            // ---- date ----
            "DATE" => {
                let mut y = n1!(0).trunc() as i64;
                let m = n1!(1).trunc() as i64;
                let d = n1!(2).trunc() as i64;
                if (0..100).contains(&y) {
                    y += if y < 30 { 2000 } else { 1900 };
                }
                y += (m - 1).div_euclid(12);
                let m = (m - 1).rem_euclid(12) + 1;
                let s = ymd_serial(y, m, 1) + (d - 1) as f64;
                Val::Num(s)
            }
            "DAY" | "MONTH" | "YEAR" | "WEEKDAY" => {
                let x = n1!(0);
                match name {
                    "DAY" => Val::Num(serial_ymd(x).2 as f64),
                    "MONTH" => Val::Num(serial_ymd(x).1 as f64),
                    "YEAR" => Val::Num(serial_ymd(x).0 as f64),
                    _ => {
                        let t = if na > 1 { n1!(1).trunc() as i64 } else { 1 };
                        let sun = sun1_weekday(x);
                        Val::Num(match t {
                            2 => ((sun + 5) % 7 + 1) as f64,
                            3 => ((sun + 5) % 7) as f64,
                            _ => sun as f64,
                        })
                    }
                }
            }
            "DAYS" => {
                let e = n1!(0).trunc();
                let s = n1!(1).trunc();
                Val::Num(e - s)
            }
            "EDATE" | "EOMONTH" => {
                let x = n1!(0);
                let mo = n1!(1).trunc() as i64;
                if x < 0.0 {
                    return Val::Err(ErrK::Num);
                }
                let (y, m, d) = serial_ymd(x);
                let total = m - 1 + mo;
                let ny = y + total.div_euclid(12);
                let nm = total.rem_euclid(12) + 1;
                if name == "EOMONTH" {
                    Val::Num(ymd_serial(ny, nm, dim(ny, nm)))
                } else {
                    let nd = d.min(dim(ny, nm));
                    Val::Num(ymd_serial(ny, nm, nd))
                }
            }
            "DATEDIF" => {
                let s = n1!(0).trunc();
                let e = n1!(1).trunc();
                let unit = s1!(2).to_uppercase();
                if s > e {
                    return Val::Err(ErrK::Value);
                }
                let (y1, m1, d1) = serial_ymd(s);
                let (y2, m2, d2) = serial_ymd(e);
                let months = (y2 - y1) * 12 + (m2 - m1) - if d2 < d1 { 1 } else { 0 };
                let r: i64 = match unit.as_str() {
                    "D" => (e - s) as i64,
                    "M" => months,
                    "Y" => months / 12,
                    "MD" => {
                        if d2 >= d1 {
                            d2 - d1
                        } else {
                            let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                            d2 + dim(py, pm) - d1
                        }
                    }
                    "YM" => months % 12,
                    "YD" => {
                    let by = if (m1, d1) > (m2, d2) { y2 - 1 } else { y2 };
                    days_from_civil(y2, m2, d2) - days_from_civil(by, m1, d1)
                }
                    _ => return Val::Err(ErrK::Num),
                };
                Val::Num(r as f64)
            }
            // ---- text ----
            "LEN" => Val::Num(s1!(0).chars().count() as f64),
            "LOWER" => Val::Str(Rc::from(s1!(0).to_lowercase().as_str())),
            "UPPER" => Val::Str(Rc::from(s1!(0).to_uppercase().as_str())),
            "TRIM" => {
                let s = s1!(0);
                let t: Vec<&str> = s.split(' ').filter(|x| !x.is_empty()).collect();
                Val::Str(Rc::from(t.join(" ").as_str()))
            }
            "PROPER" => {
                let s = s1!(0);
                let mut out = String::new();
                let mut prev = false;
                for c in s.chars() {
                    if c.is_alphabetic() {
                        if prev {
                            out.extend(c.to_lowercase());
                        } else {
                            out.extend(c.to_uppercase());
                        }
                        prev = true;
                    } else {
                        out.push(c);
                        prev = false;
                    }
                }
                Val::Str(Rc::from(out.as_str()))
            }
            "LEFT" | "RIGHT" => {
                let s = s1!(0);
                let n = if na > 1 { n1!(1) } else { 1.0 };
                if n < 0.0 {
                    return Val::Err(ErrK::Value);
                }
                let chars: Vec<char> = s.chars().collect();
                let k = (n.trunc() as usize).min(chars.len());
                let out: String = if name == "LEFT" {
                    chars[..k].iter().collect()
                } else {
                    chars[chars.len() - k..].iter().collect()
                };
                Val::Str(Rc::from(out.as_str()))
            }
            "MID" => {
                let s = s1!(0);
                let st = n1!(1).trunc();
                let n = n1!(2).trunc();
                if st < 1.0 || n < 0.0 {
                    return Val::Err(ErrK::Value);
                }
                let out: String = s.chars().skip(st as usize - 1).take(n as usize).collect();
                Val::Str(Rc::from(out.as_str()))
            }
            "REPT" => {
                let s = s1!(0);
                let n = n1!(1).trunc();
                if n < 0.0 {
                    return Val::Err(ErrK::Value);
                }
                Val::Str(Rc::from(s.repeat(n as usize).as_str()))
            }
            "CONCAT" | "CONCATENATE" | "TEXTJOIN" => {
                let (delim, ignore, start) = if name == "TEXTJOIN" {
                    let d = s1!(0);
                    let ig = b1!(1);
                    (d, ig, 2)
                } else {
                    (String::new(), false, 0)
                };
                let mut parts: Vec<String> = Vec::new();
                let mut first_err: Option<ErrK> = None;
                for x in a.iter().skip(start) {
                    match x {
                        Arg::V(Val::Err(e)) => {
                            if first_err.is_none() || *e == ErrK::Value {
                                if first_err.is_none() || first_err != Some(ErrK::Value) {
                                    first_err = Some(*e);
                                }
                            }
                        }
                        Arg::V(v) => parts.push(to_text(v)),
                        Arg::R(..) => {
                            for v in self.vals(x) {
                                match v {
                                    Val::Err(e) => {
                                        if first_err.is_none() || *e == ErrK::Value {
                                            if first_err != Some(ErrK::Value) {
                                                first_err = Some(*e);
                                            }
                                        }
                                    }
                                    Val::Bool(b) => parts.push(if *b { "1".to_string() } else { "0".to_string() }),
                                    _ => parts.push(to_text(v)),
                                }
                            }
                        }
                    }
                }
                if let Some(e) = first_err {
                    return Val::Err(e);
                }
                if name == "TEXTJOIN" {
                    if ignore {
                        parts.retain(|p| !p.is_empty());
                    }
                    Val::Str(Rc::from(parts.join(&delim).as_str()))
                } else {
                    Val::Str(Rc::from(parts.concat().as_str()))
                }
            }
            "EXACT" => {
                let x = s1!(0);
                let y = s1!(1);
                Val::Bool(x == y)
            }
            "FIND" | "SEARCH" => {
                let f = s1!(0);
                let w = s1!(1);
                let st = if na > 2 { n1!(2).trunc() as i64 } else { 1 };
                let wc: Vec<char> = w.chars().collect();
                if st < 1 || st as usize > wc.len() + 1 {
                    return Val::Err(ErrK::Value);
                }
                let start = st as usize - 1;
                if name == "FIND" {
                    let fc: Vec<char> = f.chars().collect();
                    for i in start..=wc.len() {
                        if i + fc.len() <= wc.len() && wc[i..i + fc.len()] == fc[..] {
                            return Val::Num((i + 1) as f64);
                        }
                    }
                    Val::Err(ErrK::Value)
                } else {
                    let fl: Vec<char> = f.to_lowercase().chars().collect();
                    let wl: Vec<char> = w.to_lowercase().chars().collect();
                    let pat_wild = has_wild(&f);
                    for i in start..wl.len() {
                        if pat_wild {
                            for k in 0..=(wl.len() - i) {
                                if wild(&fl, &wl[i..i + k]) {
                                    return Val::Num((i + 1) as f64);
                                }
                            }
                        } else if i + fl.len() <= wl.len() && wl[i..i + fl.len()] == fl[..] {
                            return Val::Num((i + 1) as f64);
                        }
                    }
                    if fl.is_empty() && start <= wl.len() {
                        return Val::Num((start + 1) as f64);
                    }
                    Val::Err(ErrK::Value)
                }
            }
            "SUBSTITUTE" => {
                let t = s1!(0);
                let old = s1!(1);
                let new = s1!(2);
                if old.is_empty() {
                    return Val::Str(Rc::from(t.as_str()));
                }
                if na > 3 {
                    let inst = n1!(3).trunc() as usize;
                    if inst < 1 {
                        return Val::Err(ErrK::Value);
                    }
                    let mut out = String::new();
                    let mut rest = t.as_str();
                    let mut count = 0;
                    while let Some(p) = rest.find(old.as_str()) {
                        count += 1;
                        out.push_str(&rest[..p]);
                        if count == inst {
                            out.push_str(&new);
                            out.push_str(&rest[p + old.len()..]);
                            return Val::Str(Rc::from(out.as_str()));
                        }
                        out.push_str(&old);
                        rest = &rest[p + old.len()..];
                    }
                    out.push_str(rest);
                    Val::Str(Rc::from(out.as_str()))
                } else {
                    Val::Str(Rc::from(t.replace(old.as_str(), &new).as_str()))
                }
            }
            "REPLACE" => {
                let t = s1!(0);
                let st = n1!(1).trunc();
                let n = n1!(2).trunc();
                let new = s1!(3);
                if st < 1.0 || n < 0.0 {
                    return Val::Err(ErrK::Value);
                }
                let c: Vec<char> = t.chars().collect();
                let s0 = (st as usize - 1).min(c.len());
                let e0 = (s0 + n as usize).min(c.len());
                let mut out: String = c[..s0].iter().collect();
                out.push_str(&new);
                out.extend(c[e0..].iter());
                Val::Str(Rc::from(out.as_str()))
            }
            "VALUE" => {
                let v = v1!(0);
                match v {
                    Val::Num(n) => Val::Num(n),
                    Val::Bool(_) => Val::Err(ErrK::Value),
                    Val::Empty => Val::Num(0.0),
                    _ => match to_num(&v) {
                        Ok(n) => Val::Num(n),
                        Err(e) => Val::Err(e),
                    },
                }
            }
            "N" => match v1!(0) {
                Val::Num(n) => Val::Num(n),
                Val::Bool(b) => Val::Num(if b { 1.0 } else { 0.0 }),
                Val::Err(e) => Val::Err(e),
                _ => Val::Num(0.0),
            },
            "T" => match v1!(0) {
                Val::Str(s) => Val::Str(s),
                Val::Err(e) => Val::Err(e),
                _ => Val::Str(Rc::from("")),
            },
            "TEXT" => {
                let v = v1!(0);
                let f = s1!(1);
                match v {
                    Val::Str(s) => match to_num(&Val::Str(s.clone())) {
                        Ok(n) => Val::Str(Rc::from(text_fmt(n, &f).as_str())),
                        Err(_) => Val::Str(s),
                    },
                    Val::Empty => Val::Str(Rc::from(text_fmt(0.0, &f).as_str())),
                    other => match to_num(&other) {
                        Ok(n) => Val::Str(Rc::from(text_fmt(n, &f).as_str())),
                        Err(e) => Val::Err(e),
                    },
                }
            }
            // ---- lookup ----
            "VLOOKUP" | "HLOOKUP" => {
                let look = v1!(0);
                let t = match cells_of(ga!(1)) {
                    Some(t) => t,
                    None => return Val::Err(ErrK::Value),
                };
                let idx = n1!(2).trunc() as i64;
                let approx = if na > 3 { b1!(3) } else { true };
                let vert = name == "VLOOKUP";
                let (h, w) = rng_dims(t);
                let span = if vert { h } else { w } as i64;
                if idx < 1 || idx > (if vert { w } else { h }) as i64 {
                    return Val::Err(ErrK::Ref);
                }
                let wc = !approx;
                let mut found: Option<u32> = None;
                let len = if vert { h } else { w };
                let _ = span;
                if approx {
                    for k in 0..len {
                        let (s, r, c) = if vert { (t.0, t.1 + k, t.2) } else { (t.0, t.1, t.2 + k) };
                        let x = self.pk(s, r, c);
                        match lk_cmp(&look, x) {
                            Some(Ordering::Greater) | Some(Ordering::Equal) => found = Some(k),
                            Some(Ordering::Less) => break,
                            None => {}
                        }
                    }
                } else {
                    for k in 0..len {
                        let (s, r, c) = if vert { (t.0, t.1 + k, t.2) } else { (t.0, t.1, t.2 + k) };
                        if lk_eq(&look, self.pk(s, r, c), wc) {
                            found = Some(k);
                            break;
                        }
                    }
                }
                match found {
                    Some(k) => {
                        let (s, r, c) = if vert {
                            (t.0, t.1 + k, t.2 + idx as u32 - 1)
                        } else {
                            (t.0, t.1 + idx as u32 - 1, t.2 + k)
                        };
                        self.pk(s, r, c).clone()
                    }
                    None => Val::Err(ErrK::NA),
                }
            }
            "MATCH" => {
                let look = v1!(0);
                let t = match cells_of(ga!(1)) {
                    Some(t) => t,
                    None => return Val::Err(ErrK::Value),
                };
                let mt = if na > 2 { n1!(2) } else { 1.0 };
                let vals = self.lookup_seq(ga!(1));
                let mut res: Option<usize> = None;
                if mt == 0.0 {
                    for (i, x) in vals.iter().enumerate() {
                        if lk_eq(&look, x, true) {
                            res = Some(i);
                            break;
                        }
                    }
                } else if mt > 0.0 {
                    for (i, x) in vals.iter().enumerate() {
                        match lk_cmp(&look, x) {
                            Some(Ordering::Greater) | Some(Ordering::Equal) => res = Some(i),
                            Some(Ordering::Less) => break,
                            None => {}
                        }
                    }
                } else {
                    for (i, x) in vals.iter().enumerate() {
                        match lk_cmp(&look, x) {
                            Some(Ordering::Less) | Some(Ordering::Equal) => res = Some(i),
                            Some(Ordering::Greater) => break,
                            None => {}
                        }
                    }
                }
                let _ = t;
                match res {
                    Some(i) => Val::Num((i + 1) as f64),
                    None => Val::Err(ErrK::NA),
                }
            }
            "XLOOKUP" => {
                let look = v1!(0);
                let lv = self.lookup_seq(ga!(1));
                let rv = self.lookup_seq(ga!(2));
                if lv.len() != rv.len() {
                    return Val::Err(ErrK::Value);
                }
                let nf = if na > 3 { Some(self.sv(&a[3])) } else { None };
                let mode = if na > 4 { n1!(4) as i64 } else { 0 };
                let search = if na > 5 { n1!(5) as i64 } else { 1 };
                let idxs: Vec<usize> = if search < 0 { (0..lv.len()).rev().collect() } else { (0..lv.len()).collect() };
                let mut hit: Option<usize> = None;
                if mode == 2 {
                    let pat = to_text(&look);
                    for &i in &idxs {
                        if let Val::Str(s) = lv[i] {
                            if wild_eq(&pat, s) {
                                hit = Some(i);
                                break;
                            }
                        }
                    }
                } else if mode == 0 {
                    for &i in &idxs {
                        if lk_eq(&look, lv[i], false) {
                            hit = Some(i);
                            break;
                        }
                    }
                } else {
                    // -1: exact or next smaller; 1: exact or next larger
                    let mut exact: Option<usize> = None;
                    let mut best: Option<usize> = None;
                    for &i in &idxs {
                        match lk_cmp(&look, lv[i]) {
                            Some(Ordering::Equal) => {
                                if exact.is_none() {
                                    exact = Some(i);
                                }
                            }
                            Some(Ordering::Greater) if mode == -1 => {
                                // lv < look
                                match best {
                                    None => best = Some(i),
                                    Some(b) => {
                                        if lk_cmp(lv[i], lv[b]) == Some(Ordering::Greater) {
                                            best = Some(i)
                                        }
                                    }
                                }
                            }
                            Some(Ordering::Less) if mode == 1 => {
                                match best {
                                    None => best = Some(i),
                                    Some(b) => {
                                        if lk_cmp(lv[i], lv[b]) == Some(Ordering::Less) {
                                            best = Some(i)
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    hit = exact.or(best);
                }
                match hit {
                    Some(i) => rv[i].clone(),
                    None => match nf {
                        Some(v) => v,
                        None => Val::Err(ErrK::NA),
                    },
                }
            }
            "INDEX" => {
                let t = match cells_of(ga!(0)) {
                    Some(t) => t,
                    None => return Val::Err(ErrK::Value),
                };
                let (h, w) = rng_dims(t);
                let r = if na > 1 { n1!(1).trunc() as i64 } else { 1 };
                let c = if na > 2 { n1!(2).trunc() as i64 } else { 0 };
                let (ri, ci) = if na == 2 {
                    if h == 1 {
                        (1, r)
                    } else {
                        (r, 1)
                    }
                } else {
                    (r, c)
                };
                if ri == 0 || ci == 0 {
                    return Val::Err(ErrK::Value);
                }
                if ri < 1 || ci < 1 || ri > h as i64 || ci > w as i64 {
                    return Val::Err(ErrK::Ref);
                }
                self.pk(t.0, t.1 + ri as u32 - 1, t.2 + ci as u32 - 1).clone()
            }
            _ => Val::Err(ErrK::Name),
        }
    }
}

fn rank_of(v: &Val) -> u8 {
    rank(v)
}

fn sun1_pub(s: f64) -> i64 {
    sun1_weekday(s)
}

#[allow(dead_code)]
pub fn unused() {
    let _ = rank_of;
    let _ = sun1_pub;
}

pub enum _Unused {}
