use crate::fmt::*;
use crate::*;

fn s(x: &str) -> Val {
    Val::S(Arc::from(x))
}
fn n(x: f64) -> Res {
    if x.is_nan() {
        Err(Er::Num)
    } else if x.is_infinite() {
        Err(Er::Num)
    } else {
        Ok(Val::N(x))
    }
}
fn numcell(v: &Val) -> Result<f64, ()> {
    match v {
        Val::N(x) => Ok(*x),
        Val::B(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(()),
    }
}
pub fn ref_like(e: &Expr) -> bool {
    match e {
        Expr::Ref { .. } => true,
        Expr::Call(name, args) => match name.as_str() {
            "INDEX" => true,
            "IF" | "CHOOSE" => args.iter().skip(1).any(ref_like),
            "IFERROR" | "IFNA" => args.iter().any(ref_like),
            "IFS" => args.iter().skip(1).step_by(2).any(ref_like),
            "SWITCH" => args.iter().enumerate().any(|(i, a)| i >= 2 && (i % 2 == 0 || i == args.len() - 1) && ref_like(a)),
            _ => false,
        },
        _ => false,
    }
}
fn debool(v: Val) -> Val {
    match v {
        Val::B(b) => Val::N(if b { 1.0 } else { 0.0 }),
        v => v,
    }
}
fn is_missing(e: &Expr) -> bool {
    matches!(e, Expr::Err(Er::Null))
}

fn to_date(v: f64) -> Result<(i64, u32, u32), Er> {
    if v < 0.0 {
        return Err(Er::Value);
    }
    Ok(serial_to_ymd(v.floor() as i64))
}

fn add_months(serial: i64, months: i64, eom: bool) -> f64 {
    let (y, m, d) = serial_to_ymd(serial);
    let tot = y * 12 + (m as i64 - 1) + months;
    let ny = tot.div_euclid(12);
    let nm = (tot.rem_euclid(12) + 1) as u32;
    let dim = days_in_month(ny, nm);
    let nd = if eom { dim } else { d.min(dim) };
    ymd_to_serial(ny, nm, nd) as f64
}

fn datedif(d1: i64, d2: i64, unit: &str) -> Result<f64, Er> {
    if d2 < d1 {
        return Err(Er::Value);
    }
    let (y1, m1, dd1) = serial_to_ymd(d1);
    let (y2, m2, dd2) = serial_to_ymd(d2);
    let u = unit.to_ascii_lowercase();
    Ok(match u.as_str() {
        "d" => (d2 - d1) as f64,
        "m" => {
            let mut months = (y2 - y1) * 12 + (m2 as i64 - m1 as i64);
            if dd2 < dd1 {
                months -= 1;
            }
            months as f64
        }
        "y" => {
            let mut years = y2 - y1;
            if (m2, dd2) < (m1, dd1) {
                years -= 1;
            }
            years as f64
        }
        "md" => {
            let mut r = dd2 as i64 - dd1 as i64;
            if r < 0 {
                // days in previous month of d2
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                r += days_in_month(py, pm) as i64;
            }
            r as f64
        }
        "ym" => {
            let mut months = m2 as i64 - m1 as i64;
            if dd2 < dd1 {
                months -= 1;
            }
            months.rem_euclid(12) as f64
        }
        "yd" => {
            let mut y = y2;
            if (m2, dd2) < (m1, dd1) {
                y -= 1;
            }
            let start = ymd_to_serial(y, m1, dd1.min(days_in_month(y, m1)));
            (d2 - start) as f64
        }
        _ => return Err(Er::Value),
    })
}

#[derive(Clone, Copy, PartialEq)]
enum TK {
    Num,
    Str,
    Bool,
    Other,
}
fn tk(v: &Val) -> TK {
    match v {
        Val::N(_) => TK::Num,
        Val::S(_) => TK::Str,
        Val::B(_) => TK::Bool,
        _ => TK::Other,
    }
}

// exact match (with wildcards for text)
fn exact_eq(x: &Val, v: &Val, pat: &Option<Vec<char>>) -> bool {
    match (x, v) {
        (Val::N(a), Val::N(b)) => a == b,
        (Val::B(a), Val::B(b)) => a == b,
        (Val::N(a), Val::B(b)) => *a == if *b { 1.0 } else { 0.0 },
        (Val::B(a), Val::N(b)) => *b == if *a { 1.0 } else { 0.0 },
        (Val::S(a), Val::S(b)) => {
            if let Some(p) = pat {
                let sc: Vec<char> = b.chars().collect();
                wild_match(p, &sc)
            } else {
                a.eq_ignore_ascii_case(b) || str_cmp(a, b) == std::cmp::Ordering::Equal
            }
        }
        (Val::Empty, Val::Empty) => true,
        (Val::N(a), Val::Empty) => *a == 0.0,
        (Val::S(a), Val::Empty) => a.is_empty(),
        _ => false,
    }
}

// ordering for lookups: numbers < strings < bools
fn lookup_cmp(x: &Val, v: &Val) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    fn rank(v: &Val) -> u8 {
        match v {
            Val::N(_) | Val::Empty => 0,
            Val::S(_) => 1,
            Val::B(_) => 2,
            _ => 3,
        }
    }
    let (rx, rv) = (rank(x), rank(v));
    if rx != rv {
        return rx.cmp(&rv);
    }
    match (x, v) {
        (Val::S(a), Val::S(b)) => str_cmp(a, b),
        (Val::B(a), Val::B(b)) => a.cmp(b),
        _ => {
            let a = to_num(x).unwrap_or(0.0);
            let b = to_num(v).unwrap_or(0.0);
            a.partial_cmp(&b).unwrap_or(Equal)
        }
    }
}

fn pat_of(x: &Val) -> Option<Vec<char>> {
    if let Val::S(s) = x {
        if s.contains('*') || s.contains('?') || s.contains('~') {
            return Some(s.chars().collect());
        }
    }
    None
}

// match position (0-based) in vals; mode 0 exact, 1 asc largest<=, -1 desc smallest>=
fn match_pos(x: &Val, vals: &[Val], mode: i32, wild: bool) -> Option<usize> {
    if mode == 0 {
        let pat = if wild { pat_of(x) } else { None };
        return vals.iter().position(|v| exact_eq(x, v, &pat));
    }
    use std::cmp::Ordering::*;
    if mode > 0 {
        // binary search for last element <= x
        let (mut lo, mut hi) = (0usize, vals.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if lookup_cmp(&vals[mid], x) != Greater {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            None
        } else {
            Some(lo - 1)
        }
    } else {
        let (mut lo, mut hi) = (0usize, vals.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if lookup_cmp(&vals[mid], x) != Less {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            None
        } else {
            Some(lo - 1)
        }
    }
}

fn proper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_alpha = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_alpha {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_alpha = true;
        } else {
            out.push(c);
            prev_alpha = false;
        }
    }
    out
}

fn substitute(text: &str, old: &str, new: &str, which: Option<usize>) -> String {
    if old.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    let mut count = 0;
    while let Some(p) = rest.find(old) {
        count += 1;
        out.push_str(&rest[..p]);
        if which.map_or(true, |w| w == count) {
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        rest = &rest[p + old.len()..];
    }
    out.push_str(rest);
    out
}

fn char_find(hay: &[char], needle: &[char], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start);
    }
    if hay.len() < needle.len() {
        return None;
    }
    for i in start..=(hay.len() - needle.len()) {
        if hay[i..i + needle.len()] == *needle {
            return Some(i);
        }
    }
    None
}

impl<'a> Ctx<'a> {
    fn arg_vals(&self, args: &[Expr]) -> Result<Vec<Val>, Er> {
        let mut out = Vec::new();
        for a in args {
            match self.eval(a)? {
                R::Rg(rg) => self.for_each(&rg, |v| out.push(v.clone())),
                R::V(v) => out.push(v),
            }
        }
        Ok(out)
    }

    fn if_count(&self, args: &[Expr], avg: bool, sum: bool, countif_style: bool) -> Res {
        // SUMIFS(sum_range, c_range, c, ...) / COUNTIFS(c_range, c, ...) / AVERAGEIFS
        let (sum_rg, rest) = if countif_style { (None, args) } else { (Some(self.range(&args[0])?), &args[1..]) };
        if rest.len() % 2 != 0 || rest.is_empty() {
            return Err(Er::Value);
        }
        let mut crits = Vec::new();
        let mut rgs = Vec::new();
        let mut cvs = Vec::new();
        for i in (0..rest.len()).step_by(2) {
            let rg = self.range(&rest[i])?;
            let cv = self.scalar(&rest[i + 1])?;
            crits.push(parse_crit(&cv));
            cvs.push(cv);
            rgs.push(rg);
        }
        let nrows = rgs[0].rows();
        let ncols = rgs[0].cols();
        if let Some(sr) = &sum_rg {
            if sr.rows() != nrows || sr.cols() != ncols {
                return Err(Er::Value);
            }
        }
        for rg in &rgs {
            if rg.rows() != nrows || rg.cols() != ncols {
                return Err(Er::Value);
            }
        }
        let len = nrows * ncols;
        let mut total = KSum::default();
        let mut cnt = 0usize;
        if len >= 256 {
            // bitset path
            let mut acc: Vec<u64> = (*self.crit_bits(&rgs[0], &cvs[0], &crits[0])).clone();
            for k in 1..rgs.len() {
                let b = self.crit_bits(&rgs[k], &cvs[k], &crits[k]);
                for (a, x) in acc.iter_mut().zip(b.iter()) {
                    *a &= *x;
                }
            }
            if sum_rg.is_none() {
                cnt = acc.iter().map(|w| w.count_ones() as usize).sum();
            } else {
                let sr = sum_rg.as_ref().unwrap();
                for (wi, w) in acc.iter().enumerate() {
                    let mut w = *w;
                    while w != 0 {
                        let bit = w.trailing_zeros() as usize;
                        w &= w - 1;
                        let i = wi * 64 + bit;
                        let (r, c) = ((i / ncols) as u32, (i % ncols) as u32);
                        if let Some(x) = self.with_cell(sr.sheet, sr.r1 + r, sr.c1 + c, |v| numcell(v).ok()) {
                            total.add(x);
                            cnt += 1;
                        }
                    }
                }
            }
        } else {
            for r in 0..nrows as u32 {
                for c in 0..ncols as u32 {
                    let mut ok = true;
                    for (rg, cr) in rgs.iter().zip(crits.iter()) {
                        if !self.with_cell(rg.sheet, rg.r1 + r, rg.c1 + c, |v| crit_match(cr, v)) {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        if let Some(sr) = &sum_rg {
                            let v = self.cell(sr.sheet, sr.r1 + r, sr.c1 + c);
                            if let Ok(x) = numcell(&v) {
                                total.add(x);
                                cnt += 1;
                            }
                        } else {
                            cnt += 1;
                        }
                    }
                }
            }
        }
        if avg {
            if cnt == 0 {
                return Err(Er::Div0);
            }
            return n(total.get() / cnt as f64);
        }
        if sum {
            return n(total.get());
        }
        n(cnt as f64)
    }

    pub fn call(&self, name: &str, args: &[Expr]) -> Res {
        let na = args.len();
        // LibreOffice-like error precedence among eagerly evaluated arguments
        if na > 1
            && !matches!(
                name,
                "IF" | "IFS" | "SWITCH" | "CHOOSE" | "IFERROR" | "IFNA" | "ISERROR" | "ISNA" | "ISBLANK" | "ISNUMBER" | "ISTEXT" | "ISLOGICAL" | "COUNT" | "COUNTA" | "N" | "T" | "XLOOKUP" | "SUMIF" | "SUMIFS" | "COUNTIF" | "COUNTIFS" | "AVERAGEIF" | "AVERAGEIFS"
            )
        {
            if matches!(name, "AND" | "OR" | "XOR") {
                for (ai, a) in args.iter().enumerate() {
                    if matches!(a, Expr::Str(_)) {
                        return Err(Er::Value);
                    }
                    if name == "XOR" && ai > 0 {
                        if let Expr::Call(fname, fargs) = a {
                            if fname == "ISLOGICAL" && fargs.len() == 1 && matches!(fargs[0], Expr::Str(_)) {
                                return Ok(Val::B(true));
                            }
                        }
                    }
                }
            }
            let first_ref_wins = matches!(name, "CONCATENATE" | "CONCAT" | "TEXTJOIN");
            let mut ref_err: Option<Er> = None;
            for a in args {
                if matches!(a, Expr::Range { .. }) {
                    continue;
                }
                if ref_like(a) {
                    if let Ok(Val::E(e)) | Err(e) = self.scalar(a) {
                        if !(first_ref_wins && ref_err.is_some()) {
                            ref_err = Some(e);
                        }
                    }
                } else {
                    match self.eval(a) {
                        Err(e) => return Err(e),
                        Ok(R::V(Val::E(e))) => return Err(e),
                        _ => {}
                    }
                }
            }
            if let Some(e) = ref_err {
                return Err(e);
            }
        }
        macro_rules! need {
            ($k:expr) => {
                if na < $k {
                    return Err(Er::Value);
                }
            };
        }
        match name {
            "TRUE" => Ok(Val::B(true)),
            "FALSE" => Ok(Val::B(false)),
            "PI" => Ok(Val::N(std::f64::consts::PI)),
            "NA" => Ok(Val::E(Er::NA)),
            "ABS" => {
                need!(1);
                n(self.num(&args[0])?.abs())
            }
            "SIGN" => {
                need!(1);
                let x = self.num(&args[0])?;
                n(if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    0.0
                })
            }
            "EXP" => {
                need!(1);
                n(self.num(&args[0])?.exp())
            }
            "LN" => {
                need!(1);
                let x = self.num(&args[0])?;
                if x <= 0.0 {
                    return Err(Er::Value);
                }
                n(x.ln())
            }
            "LOG10" => {
                need!(1);
                let x = self.num(&args[0])?;
                if x <= 0.0 {
                    return Err(Er::Value);
                }
                n(x.log10())
            }
            "LOG" => {
                need!(1);
                let x = self.num(&args[0])?;
                let b = if na > 1 && !is_missing(&args[1]) { self.num(&args[1])? } else { 10.0 };
                if x <= 0.0 || b <= 0.0 || b == 1.0 {
                    return Err(Er::Value);
                }
                let r = if b == 10.0 { x.log10() } else { x.ln() / b.ln() };
                n(round_sig15(r))
            }
            "SQRT" => {
                need!(1);
                let x = self.num(&args[0])?;
                if x < 0.0 {
                    return Err(Er::Value);
                }
                n(x.sqrt())
            }
            "POWER" => {
                need!(2);
                pow(self.num(&args[0])?, self.num(&args[1])?)
            }
            "INT" => {
                need!(1);
                n(round_sig15(self.num(&args[0])?).floor())
            }
            "MOD" => {
                need!(2);
                let a = self.num(&args[0])?;
                let b = self.num(&args[1])?;
                if b == 0.0 {
                    return Err(Er::Div0);
                }
                let mut r = a % b;
                if r != 0.0 && ((r < 0.0) != (b < 0.0)) {
                    r += b;
                }
                n(r)
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "TRUNC" => {
                need!(1);
                let x = self.num(&args[0])?;
                let d = if na > 1 && !is_missing(&args[1]) { self.num(&args[1])?.trunc() as i32 } else { 0 };
                let fac = 10f64.powi(d.abs());
                let scaled = if d >= 0 { x * fac } else { x / fac };
                let scaled = round_sig15(scaled);
                let r = match name {
                    "ROUND" => scaled.round(),
                    "ROUNDUP" => scaled.abs().ceil() * scaled.signum(),
                    _ => scaled.trunc(),
                };
                let r = if d >= 0 { r / fac } else { r * fac };
                n(if r == 0.0 { 0.0 } else { r })
            }
            "SUM" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                n(ksum(&v))
            }
            "PRODUCT" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                if v.is_empty() {
                    return Ok(Val::N(0.0));
                }
                n(v.iter().product())
            }
            "SUMSQ" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                n(ksum(&v.iter().map(|x| x * x).collect::<Vec<_>>()))
            }
            "AVERAGE" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                if v.is_empty() {
                    return Err(Er::Div0);
                }
                n(ksum(&v) / v.len() as f64)
            }
            "MIN" | "MAX" => {
                let mut v: Vec<(f64, bool)> = Vec::new();
                for a in args {
                    match self.eval(a)? {
                        R::Rg(rg) => {
                            let mut err = None;
                            self.for_each(&rg, |x| match x {
                                Val::N(k) => v.push((*k, false)),
                                Val::B(b) => v.push((if *b { 1.0 } else { 0.0 }, true)),
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
                        R::V(x) => match x {
                            Val::N(k) => v.push((k, false)),
                            Val::B(b) => v.push((if b { 1.0 } else { 0.0 }, true)),
                            Val::E(e) => return Err(e),
                            Val::S(_) => return Err(Er::Value),
                            Val::Empty => {}
                        },
                    }
                }
                if v.is_empty() {
                    return Ok(Val::N(0.0));
                }
                let mut best = v[0];
                for x in &v[1..] {
                    if (name == "MIN" && x.0 < best.0) || (name == "MAX" && x.0 > best.0) {
                        best = *x;
                    }
                }
                if v[0].1 && best.1 {
                    Ok(Val::B(best.0 != 0.0))
                } else {
                    n(best.0)
                }
            }
            "MEDIAN" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                if v.is_empty() {
                    return Err(Er::Value);
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let m = v.len() / 2;
                n(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
            }
            "VAR" | "STDEV" => {
                let mut v = Vec::new();
                self.collect_nums(args, &mut v)?;
                if v.len() < 2 {
                    return Err(Er::Div0);
                }
                let mean = ksum(&v) / v.len() as f64;
                let var = ksum(&v.iter().map(|x| (x - mean) * (x - mean)).collect::<Vec<_>>()) / (v.len() - 1) as f64;
                n(if name == "VAR" { var } else { var.sqrt() })
            }
            "LARGE" | "SMALL" => {
                need!(2);
                let mut v = Vec::new();
                self.collect_nums(&args[..1], &mut v)?;
                let k = self.num(&args[1])?.trunc() as i64;
                if k < 1 || k as usize > v.len() {
                    return Err(Er::Value);
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let k = k as usize;
                n(if name == "SMALL" { v[k - 1] } else { v[v.len() - k] })
            }
            "COUNT" => {
                let mut c = 0;
                for a in args {
                    let r = match self.eval(a) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    match r {
                        R::Rg(rg) => self.for_each(&rg, |v| {
                            if matches!(v, Val::N(_) | Val::B(_)) {
                                c += 1
                            }
                        }),
                        R::V(v) => match v {
                            Val::N(_) | Val::B(_) => c += 1,
                            Val::S(s) => {
                                if str_to_num(&s).is_some() {
                                    c += 1
                                }
                            }
                            _ => {}
                        },
                    }
                }
                n(c as f64)
            }
            "COUNTA" => {
                let mut c = 0;
                for a in args {
                    let r = match self.eval(a) {
                        Ok(r) => r,
                        Err(_) => {
                            c += 1;
                            continue;
                        }
                    };
                    match r {
                        R::Rg(rg) => self.for_each(&rg, |v| {
                            if !matches!(v, Val::Empty) {
                                c += 1
                            }
                        }),
                        R::V(_) => c += 1,
                    }
                }
                n(c as f64)
            }
            "COUNTBLANK" => {
                need!(1);
                let rg = self.range(&args[0])?;
                let mut c = 0;
                self.for_each(&rg, |v| {
                    if matches!(v, Val::Empty) || matches!(v, Val::S(s) if s.is_empty()) {
                        c += 1
                    }
                });
                n(c as f64)
            }
            "SUMPRODUCT" => {
                need!(1);
                let mut arrays: Vec<Vec<f64>> = Vec::new();
                for a in args {
                    match self.eval(a)? {
                        R::Rg(rg) => {
                            let mut v = Vec::with_capacity(rg.rows() * rg.cols());
                            let mut err = None;
                            self.for_each(&rg, |x| {
                                v.push(match x {
                                    Val::N(x) => *x,
                                    Val::B(b) => {
                                        if *b {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    Val::E(e) => {
                                        err = Some(*e);
                                        0.0
                                    }
                                    _ => 0.0,
                                })
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                            arrays.push(v)
                        }
                        R::V(v) => arrays.push(vec![match v {
                            Val::N(x) => x,
                            Val::B(b) => {
                                if b {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            Val::E(e) => return Err(e),
                            _ => 0.0,
                        }]),
                    }
                }
                let len = arrays[0].len();
                if arrays.iter().any(|a| a.len() != len) {
                    return Err(Er::Value);
                }
                let mut total = KSum::default();
                for i in 0..len {
                    let mut p = 1.0;
                    for a in &arrays {
                        p *= a[i];
                    }
                    total.add(p);
                }
                n(total.get())
            }
            "SUMIF" => {
                need!(2);
                let crg = self.range(&args[0])?;
                let cv = self.scalar(&args[1])?;
                let crit = parse_crit(&cv);
                let srg = if na > 2 { self.range(&args[2])? } else { crg };
                let mut total = KSum::default();
                if crg.rows() * crg.cols() >= 256 {
                    let bits = self.crit_bits(&crg, &cv, &crit);
                    let ncols = crg.cols();
                    for (wi, w) in bits.iter().enumerate() {
                        let mut w = *w;
                        while w != 0 {
                            let bit = w.trailing_zeros() as usize;
                            w &= w - 1;
                            let i = wi * 64 + bit;
                            let (r, c) = ((i / ncols) as u32, (i % ncols) as u32);
                            if let Some(x) = self.with_cell(srg.sheet, srg.r1 + r, srg.c1 + c, |v| numcell(v).ok()) {
                                total.add(x);
                            }
                        }
                    }
                    return n(total.get());
                }
                for r in 0..crg.rows() as u32 {
                    for c in 0..crg.cols() as u32 {
                        if self.with_cell(crg.sheet, crg.r1 + r, crg.c1 + c, |v| crit_match(&crit, v)) {
                            if let Ok(x) = numcell(&self.cell(srg.sheet, srg.r1 + r, srg.c1 + c)) {
                                total.add(x);
                            }
                        }
                    }
                }
                n(total.get())
            }
            "AVERAGEIF" => {
                need!(2);
                let crg = self.range(&args[0])?;
                let cv = self.scalar(&args[1])?;
                let crit = parse_crit(&cv);
                let srg = if na > 2 { self.range(&args[2])? } else { crg };
                let mut total = KSum::default();
                let mut cnt = 0;
                if crg.rows() * crg.cols() >= 256 {
                    let bits = self.crit_bits(&crg, &cv, &crit);
                    let ncols = crg.cols();
                    for (wi, w) in bits.iter().enumerate() {
                        let mut w = *w;
                        while w != 0 {
                            let bit = w.trailing_zeros() as usize;
                            w &= w - 1;
                            let i = wi * 64 + bit;
                            let (r, c) = ((i / ncols) as u32, (i % ncols) as u32);
                            if let Some(x) = self.with_cell(srg.sheet, srg.r1 + r, srg.c1 + c, |v| numcell(v).ok()) {
                                total.add(x);
                                cnt += 1;
                            }
                        }
                    }
                    if cnt == 0 {
                        return Err(Er::Div0);
                    }
                    return n(total.get() / cnt as f64);
                }
                for r in 0..crg.rows() as u32 {
                    for c in 0..crg.cols() as u32 {
                        if self.with_cell(crg.sheet, crg.r1 + r, crg.c1 + c, |v| crit_match(&crit, v)) {
                            if let Ok(x) = numcell(&self.cell(srg.sheet, srg.r1 + r, srg.c1 + c)) {
                                total.add(x);
                                cnt += 1;
                            }
                        }
                    }
                }
                if cnt == 0 {
                    return Err(Er::Div0);
                }
                n(total.get() / cnt as f64)
            }
            "COUNTIF" => {
                need!(2);
                let crg = self.range(&args[0])?;
                let cv = self.scalar(&args[1])?;
                let crit = parse_crit(&cv);
                if crg.rows() * crg.cols() >= 256 {
                    let bits = self.crit_bits(&crg, &cv, &crit);
                    let cnt: usize = bits.iter().map(|w| w.count_ones() as usize).sum();
                    return n(cnt as f64);
                }
                let mut cnt = 0;
                self.for_each(&crg, |v| {
                    if crit_match(&crit, v) {
                        cnt += 1
                    }
                });
                n(cnt as f64)
            }
            "SUMIFS" => {
                need!(3);
                self.if_count(args, false, true, false)
            }
            "AVERAGEIFS" => {
                need!(3);
                self.if_count(args, true, false, false)
            }
            "COUNTIFS" => {
                need!(2);
                self.if_count(args, false, false, true)
            }
            "IF" => {
                need!(1);
                let c = self.boolean(&args[0])?;
                if c {
                    if na > 1 && !is_missing(&args[1]) {
                        self.scalar(&args[1])
                    } else {
                        Ok(Val::B(true))
                    }
                } else if na > 2 && !is_missing(&args[2]) {
                    self.scalar(&args[2])
                } else {
                    Ok(Val::B(false))
                }
            }
            "IFERROR" => {
                need!(2);
                match self.scalar(&args[0]) {
                    Ok(Val::E(_)) | Err(_) => self.scalar(&args[1]),
                    Ok(v) => Ok(v),
                }
            }
            "IFNA" => {
                need!(2);
                match self.scalar(&args[0]) {
                    Ok(Val::E(Er::NA)) | Err(Er::NA) => self.scalar(&args[1]),
                    Ok(v) => Ok(v),
                    Err(e) => Err(e),
                }
            }
            "IFS" => {
                need!(2);
                let mut i = 0;
                while i + 1 < na {
                    if self.boolean(&args[i])? {
                        return self.scalar(&args[i + 1]);
                    }
                    i += 2;
                }
                Err(Er::NA)
            }
            "SWITCH" => {
                need!(3);
                let x = self.scalar(&args[0])?;
                if let Val::E(e) = x {
                    return Err(e);
                }
                let mut i = 1;
                while i + 1 < na {
                    let v = self.scalar(&args[i])?;
                    let eq = match (&x, &v) {
                        (Val::N(_), Val::S(sv)) => to_str(&x)?.eq_ignore_ascii_case(sv),
                        (Val::S(sx), Val::N(_)) => to_str(&v)?.eq_ignore_ascii_case(sx),
                        _ => cmp_vals(&x, &v)? == std::cmp::Ordering::Equal,
                    };
                    if eq {
                        return self.scalar(&args[i + 1]);
                    }
                    i += 2;
                }
                if i < na {
                    return self.scalar(&args[i]);
                }
                Err(Er::NA)
            }
            "CHOOSE" => {
                need!(2);
                let k = self.num(&args[0])?.trunc() as i64;
                if k < 1 || k as usize >= na {
                    return Err(Er::Value);
                }
                self.scalar(&args[k as usize])
            }
            "AND" | "OR" | "XOR" => {
                let mut acc: Option<bool> = None;
                let mut cnt = 0;
                for (ai, a) in args.iter().enumerate() {
                    if matches!(a, Expr::Str(_)) {
                        return Err(Er::Value);
                    }
                    // LibreOffice quirk: ISLOGICAL(<string literal>) as a later XOR argument makes the result TRUE
                    if name == "XOR" && ai > 0 {
                        if let Expr::Call(fname, fargs) = a {
                            if fname == "ISLOGICAL" && fargs.len() == 1 && matches!(fargs[0], Expr::Str(_)) {
                                return Ok(Val::B(true));
                            }
                        }
                    }
                    match self.eval(a)? {
                        R::Rg(rg) => {
                            let mut err = None;
                            self.for_each(&rg, |v| {
                                let b = match v {
                                    Val::N(x) => Some(*x != 0.0),
                                    Val::B(b) => Some(*b),
                                    Val::E(e) => {
                                        err = Some(*e);
                                        None
                                    }
                                    _ => None,
                                };
                                if let Some(b) = b {
                                    cnt += 1;
                                    acc = Some(match (name, acc) {
                                        (_, None) => b,
                                        ("AND", Some(x)) => x && b,
                                        ("OR", Some(x)) => x || b,
                                        (_, Some(x)) => x ^ b,
                                    });
                                }
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        }
                        R::V(v) => {
                            let b = to_bool(&v)?;
                            cnt += 1;
                            acc = Some(match (name, acc) {
                                (_, None) => b,
                                ("AND", Some(x)) => x && b,
                                ("OR", Some(x)) => x || b,
                                (_, Some(x)) => x ^ b,
                            });
                        }
                    }
                }
                if cnt == 0 {
                    return Err(Er::Value);
                }
                Ok(Val::B(acc.unwrap()))
            }
            "NOT" => {
                need!(1);
                Ok(Val::B(!self.boolean(&args[0])?))
            }
            "ISBLANK" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::Empty))))
            }
            "ISERROR" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::E(_)) | Err(_))))
            }
            "ISNA" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::E(Er::NA)) | Err(Er::NA))))
            }
            "ISNUMBER" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::N(_)) | Ok(Val::B(_)))))
            }
            "ISTEXT" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::S(_)))))
            }
            "ISLOGICAL" => {
                need!(1);
                Ok(Val::B(matches!(self.scalar(&args[0]), Ok(Val::B(_)))))
            }
            "N" => {
                need!(1);
                Ok(match self.scalar(&args[0])? {
                    Val::N(x) => Val::N(x),
                    Val::B(b) => Val::N(if b { 1.0 } else { 0.0 }),
                    Val::E(e) => Val::E(e),
                    _ => Val::N(0.0),
                })
            }
            "T" => {
                need!(1);
                Ok(match self.scalar(&args[0])? {
                    Val::S(x) => Val::S(x),
                    Val::E(e) => Val::E(e),
                    _ => s(""),
                })
            }
            "VALUE" => {
                need!(1);
                match self.scalar(&args[0])? {
                    Val::S(x) => n(str_to_num(&x).ok_or(Er::Value)?),
                    Val::B(b) => Ok(Val::B(b)),
                    v => n(to_num(&v)?),
                }
            }
            "LEN" => {
                need!(1);
                n(self.string(&args[0])?.chars().count() as f64)
            }
            "UPPER" => {
                need!(1);
                Ok(s(&self.string(&args[0])?.to_uppercase()))
            }
            "LOWER" => {
                need!(1);
                Ok(s(&self.string(&args[0])?.to_lowercase()))
            }
            "PROPER" => {
                need!(1);
                Ok(s(&proper(&self.string(&args[0])?)))
            }
            "TRIM" => {
                need!(1);
                let st = self.string(&args[0])?;
                Ok(s(&st.split(' ').filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")))
            }
            "EXACT" => {
                need!(2);
                let a = self.string(&args[0])?;
                let b = self.string(&args[1])?;
                Ok(Val::B(a == b))
            }
            "LEFT" | "RIGHT" => {
                need!(1);
                let st = self.string(&args[0])?;
                let k = if na > 1 && !is_missing(&args[1]) { self.num(&args[1])? } else { 1.0 };
                if k < 0.0 {
                    return Err(Er::Value);
                }
                let k = k.trunc() as usize;
                let chars: Vec<char> = st.chars().collect();
                let k = k.min(chars.len());
                Ok(s(&if name == "LEFT" { chars[..k].iter().collect::<String>() } else { chars[chars.len() - k..].iter().collect::<String>() }))
            }
            "MID" => {
                need!(3);
                let st = self.string(&args[0])?;
                let start = self.num(&args[1])?;
                let len = self.num(&args[2])?;
                if start < 1.0 || len < 0.0 {
                    return Err(Er::Value);
                }
                let start = start.trunc() as usize - 1;
                let len = len.trunc() as usize;
                let chars: Vec<char> = st.chars().collect();
                if start >= chars.len() {
                    return Ok(s(""));
                }
                let end = (start + len).min(chars.len());
                Ok(s(&chars[start..end].iter().collect::<String>()))
            }
            "REPT" => {
                need!(2);
                let st = self.string(&args[0])?;
                let k = self.num(&args[1])?;
                if k < 0.0 {
                    return Err(Er::Value);
                }
                Ok(s(&st.repeat(k.trunc() as usize)))
            }
            "REPLACE" => {
                need!(4);
                let st = self.string(&args[0])?;
                let start = self.num(&args[1])?;
                let len = self.num(&args[2])?;
                let new = self.string(&args[3])?;
                if start < 1.0 || len < 0.0 {
                    return Err(Er::Value);
                }
                let chars: Vec<char> = st.chars().collect();
                let start = ((start.trunc() as usize) - 1).min(chars.len());
                let end = (start + len.trunc() as usize).min(chars.len());
                let mut out: String = chars[..start].iter().collect();
                out.push_str(&new);
                out.extend(chars[end..].iter());
                Ok(s(&out))
            }
            "SUBSTITUTE" => {
                need!(3);
                let st = self.string(&args[0])?;
                let old = self.string(&args[1])?;
                let new = self.string(&args[2])?;
                let which = if na > 3 && !is_missing(&args[3]) {
                    let k = self.num(&args[3])?;
                    if k < 1.0 {
                        return Err(Er::Value);
                    }
                    Some(k.trunc() as usize)
                } else {
                    None
                };
                Ok(s(&substitute(&st, &old, &new, which)))
            }
            "FIND" | "SEARCH" => {
                need!(2);
                let needle = self.string(&args[0])?;
                let hay = self.string(&args[1])?;
                let start = if na > 2 && !is_missing(&args[2]) { self.num(&args[2])? } else { 1.0 };
                let hc: Vec<char> = hay.chars().collect();
                if start < 1.0 || (start.trunc() as usize) > hc.len() + 0 && !(hc.is_empty() && start == 1.0) {
                    if start < 1.0 || start.trunc() as usize > hc.len() {
                        return Err(Er::Value);
                    }
                }
                let st = start.trunc() as usize - 1;
                if needle.is_empty() {
                    return Err(Er::Value);
                }
                if name == "FIND" {
                    let nc: Vec<char> = needle.chars().collect();
                    match char_find(&hc, &nc, st) {
                        Some(p) => n((p + 1) as f64),
                        None => Err(Er::Value),
                    }
                } else {
                    let nl: Vec<char> = needle.to_lowercase().chars().collect();
                    let hl: Vec<char> = hay.to_lowercase().chars().collect();
                    let wild = nl.iter().any(|c| *c == '*' || *c == '?' || *c == '~');
                    if wild {
                        // find first position where pattern matches a prefix of remaining text
                        for i in st..hl.len() {
                            for j in i..=hl.len() {
                                if wild_match(&nl, &hl[i..j]) {
                                    return n((i + 1) as f64);
                                }
                            }
                        }
                        Err(Er::Value)
                    } else {
                        match char_find(&hl, &nl, st) {
                            Some(p) => n((p + 1) as f64),
                            None => Err(Er::Value),
                        }
                    }
                }
            }
            "CONCATENATE" => {
                let mut out = String::new();
                for a in args {
                    out.push_str(&self.string(a)?);
                }
                Ok(s(&out))
            }
            "CONCAT" => {
                let mut out = String::new();
                for a in args {
                    match self.eval(a)? {
                        R::Rg(rg) => {
                            let mut err = None;
                            self.for_each(&rg, |v| match to_str(v) {
                                Ok(x) => out.push_str(&x),
                                Err(e) => err = Some(e),
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        }
                        R::V(v) => out.push_str(&to_str(&v)?),
                    }
                }
                Ok(s(&out))
            }
            "TEXTJOIN" => {
                need!(3);
                let delim = self.string(&args[0])?;
                let ignore_empty = self.boolean(&args[1])?;
                let mut parts: Vec<Arc<str>> = Vec::new();
                for a in &args[2..] {
                    match self.eval(a)? {
                        R::Rg(rg) => {
                            let mut err = None;
                            self.for_each(&rg, |v| match to_str(v) {
                                Ok(x) => parts.push(x),
                                Err(e) => err = Some(e),
                            });
                            if let Some(e) = err {
                                return Err(e);
                            }
                        }
                        R::V(v) => parts.push(to_str(&v)?),
                    }
                }
                let joined: Vec<&str> = parts.iter().map(|p| &**p).filter(|p| !(ignore_empty && p.is_empty())).collect();
                Ok(s(&joined.join(&delim)))
            }
            "TEXT" => {
                need!(2);
                let v = self.scalar(&args[0])?;
                let f = self.string(&args[1])?;
                let x = match v {
                    Val::S(st) => match str_to_num(&st) {
                        Some(x) => x,
                        None => return Ok(Val::S(st)),
                    },
                    v => to_num(&v)?,
                };
                match format_number(x, &f) {
                    Some(r) => Ok(s(&r)),
                    None => Err(Er::Value),
                }
            }
            "DATE" => {
                need!(3);
                let y = self.num(&args[0])?;
                let m = self.num(&args[1])?;
                let d = self.num(&args[2])?;
                match date_serial(y.trunc() as i64, m.trunc() as i64, d.trunc() as i64) {
                    Some(v) => n(v),
                    None => Err(Er::Value),
                }
            }
            "YEAR" | "MONTH" | "DAY" => {
                need!(1);
                let v = self.num(&args[0])?;
                let (y, m, d) = to_date(v)?;
                n(match name {
                    "YEAR" => y as f64,
                    "MONTH" => m as f64,
                    _ => d as f64,
                })
            }
            "WEEKDAY" => {
                need!(1);
                let v = self.num(&args[0])?;
                if v < 0.0 {
                    return Err(Er::Value);
                }
                let t = if na > 1 && !is_missing(&args[1]) { self.num(&args[1])?.trunc() as i64 } else { 1 };
                let serial = v.floor() as i64;
                let wd_mon0 = (serial + 5).rem_euclid(7); // 0 = Monday
                n(match t {
                    1 | 17 => ((wd_mon0 + 1) % 7 + 1) as f64,
                    2 | 11 => (wd_mon0 + 1) as f64,
                    3 => wd_mon0 as f64,
                    12..=16 => ((wd_mon0 - (t - 11)).rem_euclid(7) + 1) as f64,
                    _ => return Err(Er::Value),
                })
            }
            "DAYS" => {
                need!(2);
                let a = self.num(&args[0])?;
                let b = self.num(&args[1])?;
                n(a.trunc() - b.trunc())
            }
            "EDATE" | "EOMONTH" => {
                need!(2);
                let v = self.num(&args[0])?;
                if v < 0.0 {
                    return Err(Er::Value);
                }
                let m = self.num(&args[1])?.trunc() as i64;
                n(add_months(v.floor() as i64, m, name == "EOMONTH"))
            }
            "DATEDIF" => {
                need!(3);
                let a = self.num(&args[0])?;
                let b = self.num(&args[1])?;
                let u = self.string(&args[2])?;
                if a < 0.0 || b < 0.0 {
                    return Err(Er::Value);
                }
                n(datedif(a.floor() as i64, b.floor() as i64, &u)?)
            }
            "INDEX" => {
                need!(2);
                let rg = self.range(&args[0])?;
                let r = self.num(&args[1])?.trunc() as i64;
                let c = if na > 2 && !is_missing(&args[2]) { self.num(&args[2])?.trunc() as i64 } else { 0 };
                let (rows, cols) = (rg.rows() as i64, rg.cols() as i64);
                let (ri, ci) = if na <= 2 || (na > 2 && is_missing(&args[2])) {
                    if rows == 1 && cols > 1 {
                        (0, r)
                    } else {
                        (r, 1)
                    }
                } else {
                    (r, c)
                };
                let (ri, ci) = if rows == 1 && cols > 1 && ci == 0 && ri >= 1 && na <= 2 { (1, ri) } else { (ri, ci) };
                let ri = if ri == 0 && rows == 1 { 1 } else { ri };
                let ci = if ci == 0 && cols == 1 { 1 } else { ci };
                if ri < 0 || ci < 0 {
                    return Err(Er::Value);
                }
                if ri == 0 || ci == 0 {
                    return Err(Er::Value);
                }
                if ri > rows || ci > cols {
                    return Err(Er::Ref);
                }
                Ok(self.cell(rg.sheet, rg.r1 + (ri - 1) as u32, rg.c1 + (ci - 1) as u32))
            }
            "MATCH" => {
                need!(2);
                let x = self.scalar(&args[0])?;
                if let Val::E(e) = x {
                    return Err(e);
                }
                let rg = self.range(&args[1])?;
                let mode = if na > 2 && !is_missing(&args[2]) { self.num(&args[2])? } else { 1.0 };
                let mode = if mode > 0.0 {
                    1
                } else if mode < 0.0 {
                    -1
                } else {
                    0
                };
                if mode == 0 {
                    if let Some(p) = self.indexed_lookup(&x, &rg) {
                        return match p {
                            Some(p) => n((p + 1) as f64),
                            None => Err(Er::NA),
                        };
                    }
                }
                let vals = self.range_vals(&rg);
                match match_pos(&x, &vals, mode, true) {
                    Some(p) => n((p + 1) as f64),
                    None => Err(Er::NA),
                }
            }
            "VLOOKUP" | "HLOOKUP" => {
                need!(3);
                let x = self.scalar(&args[0])?;
                if let Val::E(e) = x {
                    return Err(e);
                }
                let rg = self.range(&args[1])?;
                let idx = self.num(&args[2])?.trunc() as i64;
                let approx = if na > 3 && !is_missing(&args[3]) { self.boolean(&args[3])? } else { true };
                let vertical = name == "VLOOKUP";
                let (len, width) = if vertical { (rg.rows(), rg.cols()) } else { (rg.cols(), rg.rows()) };
                if idx < 1 || idx as usize > width {
                    return Err(Er::Value);
                }
                if !approx {
                    let krg = if vertical { Rng { sheet: rg.sheet, r1: rg.r1, c1: rg.c1, r2: rg.r2, c2: rg.c1 } } else { Rng { sheet: rg.sheet, r1: rg.r1, c1: rg.c1, r2: rg.r1, c2: rg.c2 } };
                    if let Some(p) = self.indexed_lookup(&x, &krg) {
                        return match p {
                            Some(p) => Ok(debool(if vertical { self.cell(rg.sheet, rg.r1 + p as u32, rg.c1 + (idx - 1) as u32) } else { self.cell(rg.sheet, rg.r1 + (idx - 1) as u32, rg.c1 + p as u32) })),
                            None => Err(Er::NA),
                        };
                    }
                }
                let mut vals = Vec::with_capacity(len);
                for i in 0..len as u32 {
                    vals.push(if vertical { self.cell(rg.sheet, rg.r1 + i, rg.c1) } else { self.cell(rg.sheet, rg.r1, rg.c1 + i) });
                }
                match match_pos(&x, &vals, if approx { 1 } else { 0 }, true) {
                    Some(p) => Ok(debool(if vertical { self.cell(rg.sheet, rg.r1 + p as u32, rg.c1 + (idx - 1) as u32) } else { self.cell(rg.sheet, rg.r1 + (idx - 1) as u32, rg.c1 + p as u32) })),
                    None => Err(Er::NA),
                }
            }
            "XLOOKUP" => {
                need!(3);
                let x = self.scalar(&args[0])?;
                if let Val::E(e) = x {
                    return Err(e);
                }
                let lrg = self.range(&args[1])?;
                let rrg = self.range(&args[2])?;
                let mode = if na > 4 && !is_missing(&args[4]) { self.num(&args[4])?.trunc() as i64 } else { 0 };
                let smode = if na > 5 && !is_missing(&args[5]) { self.num(&args[5])?.trunc() as i64 } else { 1 };
                let vertical = lrg.rows() >= lrg.cols();
                let rlen = if vertical { rrg.rows() } else { rrg.cols() };
                if rlen != lrg.rows() * lrg.cols() {
                    return Err(Er::Value);
                }
                let mut fast: Option<Option<usize>> = None;
                if mode == 0 && smode >= 0 {
                    fast = self.indexed_lookup(&x, &lrg);
                }
                let vals = if fast.is_some() { Rc::new(Vec::new()) } else { self.range_vals(&lrg) };
                let pos: Option<usize> = if let Some(p) = fast { p } else { match mode {
                    0 | 2 => {
                        let pat = if mode == 2 { pat_of(&x) } else { None };
                        if smode < 0 {
                            vals.iter().rposition(|v| exact_eq(&x, v, &pat))
                        } else {
                            vals.iter().position(|v| exact_eq(&x, v, &pat))
                        }
                    }
                    -1 => {
                        // exact or next smaller
                        let mut best: Option<usize> = None;
                        for (i, v) in vals.iter().enumerate() {
                            if tk(v) != tk(&x) && !(matches!(v, Val::Empty) && tk(&x) == TK::Num) {
                                continue;
                            }
                            let o = lookup_cmp(v, &x);
                            if o == std::cmp::Ordering::Equal {
                                best = Some(i);
                                break;
                            }
                            if o == std::cmp::Ordering::Less {
                                match best {
                                    None => best = Some(i),
                                    Some(b) => {
                                        if lookup_cmp(v, &vals[b]) == std::cmp::Ordering::Greater {
                                            best = Some(i)
                                        }
                                    }
                                }
                            }
                        }
                        best
                    }
                    1 => {
                        let mut best: Option<usize> = None;
                        for (i, v) in vals.iter().enumerate() {
                            if tk(v) != tk(&x) && !(matches!(v, Val::Empty) && tk(&x) == TK::Num) {
                                continue;
                            }
                            let o = lookup_cmp(v, &x);
                            if o == std::cmp::Ordering::Equal {
                                best = Some(i);
                                break;
                            }
                            if o == std::cmp::Ordering::Greater {
                                match best {
                                    None => best = Some(i),
                                    Some(b) => {
                                        if lookup_cmp(v, &vals[b]) == std::cmp::Ordering::Less {
                                            best = Some(i)
                                        }
                                    }
                                }
                            }
                        }
                        best
                    }
                    _ => return Err(Er::Value),
                } };
                match pos {
                    Some(p) => Ok(if vertical { self.cell(rrg.sheet, rrg.r1 + p as u32, rrg.c1) } else { self.cell(rrg.sheet, rrg.r1, rrg.c1 + p as u32) }),
                    None => {
                        if na > 3 && !is_missing(&args[3]) {
                            self.scalar(&args[3])
                        } else {
                            Err(Er::NA)
                        }
                    }
                }
            }
            _ => Err(Er::Name),
        }
    }
}
