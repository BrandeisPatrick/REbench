pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
pub fn serial_from_ymd(y: i64, m: i64, d: i64) -> f64 { (days_from_civil(y, m, d) + 25569) as f64 }
pub fn ymd_from_serial(s: f64) -> (i64, i64, i64) { civil_from_days(s.floor() as i64 - 25569) }

pub fn parse_num_str(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() { return None; }
    if t.eq_ignore_ascii_case("true") { return Some(1.0); }
    if t.eq_ignore_ascii_case("false") { return Some(0.0); }
    if t.contains(',') {
        let p: Vec<&str> = t.split(',').collect();
        let intpart_ok = |x: &str| !x.is_empty() && x.len() <= 3 && x.bytes().all(|c| c.is_ascii_digit());
        if p.len() >= 2 && intpart_ok(p[0]) && p[1..p.len() - 1].iter().all(|x| x.len() == 3 && x.bytes().all(|c| c.is_ascii_digit())) {
            let last = p[p.len() - 1];
            let (ip, rest) = match last.find('.') { Some(i) => (&last[..i], &last[i..]), None => (last, "") };
            if ip.len() == 3 && ip.bytes().all(|c| c.is_ascii_digit()) {
                let joined = format!("{}{}{}", p[..p.len() - 1].join(""), ip, rest);
                return joined.parse::<f64>().ok();
            }
        }
        return None;
    }
    let b = t.as_bytes();
    let mut body = t;
    let mut pct = false;
    if b[b.len() - 1] == b'%' { pct = true; body = t[..t.len() - 1].trim_end(); }
    let bb = body.as_bytes();
    if bb.is_empty() { return None; }
    let mut ok = true; let mut digits = false;
    for (i, &c) in bb.iter().enumerate() {
        match c {
            b'0'..=b'9' => digits = true,
            b'.' | b'e' | b'E' => {}
            b'+' | b'-' => { if !(i == 0 || bb[i - 1] == b'e' || bb[i - 1] == b'E') { ok = false; } }
            _ => ok = false,
        }
    }
    if ok && digits {
        if let Ok(v) = body.parse::<f64>() { return Some(if pct { v / 100.0 } else { v }); }
    }
    if let Some(v) = parse_time_str(t) { return Some(v); }
    if let Some((d, tm)) = t.split_once(' ') {
        if let (Some(dv), Some(tv)) = (parse_date_str(d.trim()).or_else(|| iso_date(d.trim())), parse_time_str(tm.trim())) { return Some(dv + tv); }
    }
    if let Some(v) = parse_date_str(t) { return Some(v); }
    // ISO date
    if !pct && bb.len() >= 8 && bb.len() <= 10 {
        let p: Vec<&str> = body.split('-').collect();
        if p.len() == 3 {
            if let (Ok(y), Ok(m), Ok(d)) = (p[0].parse::<i64>(), p[1].parse::<i64>(), p[2].parse::<i64>()) {
                if p[0].len() == 4 && (1..=12).contains(&m) && (1..=31).contains(&d) {
                    let s = serial_from_ymd(y, m, d);
                    let (y2, m2, d2) = ymd_from_serial(s);
                    if y2 == y && m2 == m && d2 == d { return Some(s); }
                }
            }
        }
    }
    None
}

pub fn num_to_str(x: f64) -> String {
    if x == 0.0 { return "0".into(); }
    if !x.is_finite() { return "#NUM!".into(); }
    if x.fract() == 0.0 && x.abs() < 9007199254740992.0 { return format!("{}", x as i64); }
    let s = format!("{:e}", x.abs());
    let (m, e) = s.split_once('e').unwrap();
    let mut exp: i32 = e.parse().unwrap();
    let exp_orig = exp;
    let mut dv: Vec<u8> = m.bytes().filter(|c| *c != b'.').collect();
    if dv.len() > 15 {
        let up = dv[15] >= b'5';
        dv.truncate(15);
        if up {
            let mut i = 14i32;
            loop {
                if i < 0 { dv.insert(0, b'1'); dv.truncate(15); exp += 1; break; }
                if dv[i as usize] == b'9' { dv[i as usize] = b'0'; i -= 1; } else { dv[i as usize] += 1; break; }
            }
        }
    }
    let digits: String = String::from_utf8(dv).unwrap();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if x < 0.0 { out.push('-'); }
    if exp_orig >= 15 || exp < -14 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 { out.push('.'); out.push_str(&digits[1..]); }
        out.push('E');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:03}", exp.abs()));
    } else if exp >= 0 {
        let e = exp as usize;
        if digits.len() <= e + 1 {
            out.push_str(digits);
            for _ in 0..(e + 1 - digits.len()) { out.push('0'); }
        } else {
            out.push_str(&digits[..e + 1]); out.push('.'); out.push_str(&digits[e + 1..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exp - 1) { out.push('0'); }
        out.push_str(digits);
    }
    out
}

pub fn round_dec(x: f64, d: i32) -> f64 {
    // round half away from zero, with 15-digit correction
    if !x.is_finite() { return x; }
    let p = 10f64.powi(d.abs());
    let y = if d >= 0 { x * p } else { x / p };
    let y = if y.abs() < 1e15 { format!("{:.14e}", y).parse::<f64>().unwrap_or(y) } else { y };
    let r = y.round();
    if d >= 0 { r / p } else { r * p }
}

fn dec_digits(x: f64, d: usize) -> (String, String) {
    // x>=0; returns int part and frac part (d digits) rounded half away using 15 sig digits
    let r = round_dec(x, d as i32);
    let s = format!("{:.*}", d, r);
    match s.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (s, String::new()) }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

#[derive(Debug, Clone)]
enum Tk { Lit(String), Dig(char), Comma, Dot, Pct, Exp(bool), Y(usize), Mo(usize), D(usize), H(usize), Mi(usize), S(usize), Ampm(bool), Frac(usize) }

fn split_sections(f: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut inq = false;
    let mut esc = false;
    for c in f.chars() {
        if esc { out.last_mut().unwrap().push(c); esc = false; continue; }
        if c == '\\' { out.last_mut().unwrap().push(c); esc = true; continue; }
        if c == '"' { inq = !inq; }
        if c == ';' && !inq { out.push(String::new()); continue; }
        out.last_mut().unwrap().push(c);
    }
    out
}

fn tokenize(sec: &str) -> (Vec<Tk>, bool) {
    let cs: Vec<char> = sec.chars().collect();
    let mut i = 0; let mut t = Vec::new(); let mut date = false;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '"' => { let mut s = String::new(); i += 1; while i < cs.len() && cs[i] != '"' { s.push(cs[i]); i += 1; } i += 1; t.push(Tk::Lit(s)); }
            '\\' => { if i + 1 < cs.len() { t.push(Tk::Lit(cs[i + 1].to_string())); } i += 2; }
            '[' => { while i < cs.len() && cs[i] != ']' { i += 1; } i += 1; }
            '_' => { t.push(Tk::Lit(" ".into())); i += 2; }
            '*' => { i += 2; }
            '0' | '#' | '?' => { t.push(Tk::Dig(c)); i += 1; }
            ',' => { t.push(Tk::Comma); i += 1; }
            '.' => {
                if date && i + 1 < cs.len() && cs[i + 1] == '0' && matches!(t.last(), Some(Tk::S(_))) {
                    let mut n = 0; i += 1; while i < cs.len() && cs[i] == '0' { n += 1; i += 1; } t.push(Tk::Frac(n));
                } else { t.push(Tk::Dot); i += 1; }
            }
            '%' => { t.push(Tk::Pct); i += 1; }
            'E' | 'e' if i + 1 < cs.len() && (cs[i + 1] == '+' || cs[i + 1] == '-') => { t.push(Tk::Exp(cs[i + 1] == '+')); i += 2; }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let l = c.to_ascii_lowercase();
                let mut n = 0; while i < cs.len() && cs[i].to_ascii_lowercase() == l { n += 1; i += 1; }
                date = true;
                t.push(match l { 'y' => Tk::Y(n), 'm' => Tk::Mo(n), 'd' => Tk::D(n), 'h' => Tk::H(n), _ => Tk::S(n) });
            }
            'A' | 'a' => {
                let rest: String = cs[i..].iter().take(5).collect::<String>().to_ascii_uppercase();
                if rest.starts_with("AM/PM") { t.push(Tk::Ampm(true)); i += 5; date = true; }
                else if rest.starts_with("A/P") { t.push(Tk::Ampm(false)); i += 3; date = true; }
                else { t.push(Tk::Lit(c.to_string())); i += 1; }
            }
            _ => { t.push(Tk::Lit(c.to_string())); i += 1; }
        }
    }
    // resolve m as minutes
    for k in 0..t.len() {
        if let Tk::Mo(n) = t[k] {
            let mut prev_h = false;
            let mut j = k;
            while j > 0 { j -= 1; match &t[j] { Tk::H(_) => { prev_h = true; break; } Tk::Lit(_) => continue, _ => break } }
            let mut next_s = false;
            let mut j = k + 1;
            while j < t.len() { match &t[j] { Tk::S(_) => { next_s = true; break; } Tk::Lit(_) => { j += 1; continue; } _ => break } }
            if prev_h || next_s { t[k] = Tk::Mi(n); }
        }
    }
    (t, date)
}

pub fn text_format(x: f64, fmt: &str) -> String {
    if fmt == "General" { return num_to_str(x); }
    if fmt == "@" { return num_to_str(x); }
    let secs = split_sections(fmt);
    let (sec, neg_auto) = if secs.len() == 1 { (&secs[0], x < 0.0) }
        else if x < 0.0 { (&secs[1], false) }
        else if x == 0.0 && secs.len() >= 3 { (&secs[2], false) }
        else { (&secs[0], false) };
    let ax = x.abs();
    let (toks, date) = tokenize(sec);
    if toks.is_empty() { return String::new(); }
    if date {
        let toks: Vec<Tk> = toks.into_iter().map(|t| match t { Tk::Comma => Tk::Lit(",".into()), Tk::Dot => Tk::Lit(".".into()), Tk::Pct => Tk::Lit("%".into()), Tk::Dig(c) => Tk::Lit(c.to_string()), t => t }).collect();
        return fmt_date(x, &toks);
    }
    let mut out = String::new();
    let sign_pos = out.len();
    // numeric
    let mut v = ax;
    let npct = toks.iter().filter(|t| matches!(t, Tk::Pct)).count();
    for _ in 0..npct { v *= 100.0; }
    let first_dig = toks.iter().position(|t| matches!(t, Tk::Dig(_)));
    let last_dig = toks.iter().rposition(|t| matches!(t, Tk::Dig(_)));
    if first_dig.is_none() {
        for t in &toks { match t { Tk::Lit(s) => out.push_str(s), Tk::Pct => out.push('%'), Tk::Dot => out.push('.'), Tk::Comma => out.push(','), _ => {} } }
        return out;
    }
    let fd = first_dig.unwrap(); let ld = last_dig.unwrap();
    // trailing commas scaling
    let mut k = ld + 1; 
    while k < toks.len() { if matches!(toks[k], Tk::Comma) { v /= 1000.0; k += 1; } else if matches!(toks[k], Tk::Lit(_)) { k += 1; } else { break; } }
    let has_group = toks[fd..=ld].iter().any(|t| matches!(t, Tk::Comma));
    let exp_pos = toks.iter().position(|t| matches!(t, Tk::Exp(_)));
    let dot_pos = toks.iter().position(|t| matches!(t, Tk::Dot));
    let int_digs: Vec<char> = toks.iter().enumerate().filter(|(i, t)| matches!(t, Tk::Dig(_)) && dot_pos.map_or(true, |d| *i < d) && exp_pos.map_or(true, |e| *i < e)).map(|(_, t)| if let Tk::Dig(c) = t { *c } else { '0' }).collect();
    let frac_digs: Vec<char> = match dot_pos { Some(d) => toks.iter().enumerate().filter(|(i, t)| matches!(t, Tk::Dig(_)) && *i > d && exp_pos.map_or(true, |e| *i < e)).map(|(_, t)| if let Tk::Dig(c) = t { *c } else { '0' }).collect(), None => vec![] };
    let nd = frac_digs.len();
    let (mut ip, mut fp, mut expstr) = (String::new(), String::new(), String::new());
    if let Some(ep) = exp_pos {
        let plus = if let Tk::Exp(p) = toks[ep] { p } else { true };
        let expdigits = toks[ep + 1..].iter().filter(|t| matches!(t, Tk::Dig(_))).count();
        let ni = int_digs.len().max(1) as i32;
        let mut e = if v == 0.0 { 0 } else { v.log10().floor() as i32 - (ni - 1) };
        let mut m = v / 10f64.powi(e);
        let (a, b) = dec_digits(m, nd);
        if a.len() as i32 > ni { e += 1; m = v / 10f64.powi(e); let r = dec_digits(m, nd); ip = r.0; fp = r.1; } else { ip = a; fp = b; }
        let sign = if e < 0 { "-" } else if plus { "+" } else { "" };
        expstr = format!("E{}{:0>w$}", sign, e.abs(), w = expdigits);
    } else {
        let (a, b) = dec_digits(v, nd);
        ip = a; fp = b;
    }
    // integer part: strip leading zeros then pad
    let nz = int_digs.iter().filter(|c| **c == '0').count();
    let mut ipd = ip.trim_start_matches('0').to_string();
    if ipd.len() < nz { ipd = format!("{}{}", "0".repeat(nz - ipd.len()), ipd); }
    let nq = int_digs.iter().filter(|c| **c == '?').count();
    if nq > 0 && ipd.len() < int_digs.len() { let pad = int_digs.len() - ipd.len(); if int_digs.iter().filter(|c| **c != '#').count() > 0 { ipd = format!("{}{}", " ".repeat(pad.min(nq)), ipd); } }
    if has_group {
        let b: Vec<char> = ipd.chars().collect();
        let mut g = String::new();
        for (i, ch) in b.iter().enumerate() { if i > 0 && (b.len() - i) % 3 == 0 && ch.is_ascii_digit() && b[i-1].is_ascii_digit() { g.push(','); } g.push(*ch); }
        ipd = g;
    }
    // fractional: trim trailing for '#'
    let mut fc: Vec<char> = fp.chars().collect();
    let mut idx = fc.len();
    while idx > 0 && frac_digs.get(idx - 1) == Some(&'#') && fc[idx - 1] == '0' { idx -= 1; }
    let mut tail = String::new();
    while idx < fc.len() { if frac_digs[idx] == '?' && fc[idx] == '0' { tail.push(' '); } idx += 1; }
    fc.truncate(fc.len() - tail.len().min(fc.len()));
    let mut fcs: String = fc.iter().collect();
    let mut keep = fcs.len();
    while keep > 0 && frac_digs.get(keep - 1) == Some(&'#') && fcs.as_bytes()[keep - 1] == b'0' { keep -= 1; }
    fcs.truncate(keep);
    // emit
    let mut int_done = false; let mut frac_done = false; let mut dot_emitted = false;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tk::Lit(s) => out.push_str(s),
            Tk::Pct => out.push('%'),
            Tk::Comma => { if i < fd || i > ld { /* scaling or literal */ } }
            Tk::Dot => { if !fcs.is_empty() || frac_digs.iter().any(|c| *c == '0') && false { } if !fcs.is_empty() { out.push('.'); dot_emitted = true; } else if frac_digs.is_empty() && false { } }
            Tk::Dig(_) => {
                let is_frac = dot_pos.map_or(false, |d| i > d) && exp_pos.map_or(true, |e| i < e);
                let is_exp = exp_pos.map_or(false, |e| i > e);
                if is_exp { continue; }
                if is_frac { if !frac_done { out.push_str(&fcs); out.push_str(&tail); frac_done = true; } }
                else if !int_done { out.push_str(&ipd); int_done = true; }
            }
            Tk::Exp(_) => out.push_str(&expstr),
            _ => {}
        }
    }
    let _ = dot_emitted;
    if neg_auto && out[sign_pos..].bytes().any(|c| (b'1'..=b'9').contains(&c)) { out.insert(sign_pos, '-'); }
    out
}

fn fmt_date(x: f64, toks: &[Tk]) -> String {
    if x >= 2958466.0 || x < -657435.0 { return "#FMT".into(); }
    let has_ampm = toks.iter().any(|t| matches!(t, Tk::Ampm(_)));
    let mut serial = x.floor();
    let mut secs = ((x - serial) * 86400.0 * 1000.0).round() / 1000.0;
    let nfrac = toks.iter().find_map(|t| if let Tk::Frac(n) = t { Some(*n) } else { None }).unwrap_or(0);
    if nfrac == 0 { secs = secs.round(); }
    if secs >= 86400.0 { secs -= 86400.0; serial += 1.0; }
    let (y, m, d) = ymd_from_serial(serial);
    let wd = (((serial as i64 + 6) % 7) + 7) % 7;
    let h = (secs / 3600.0).floor() as i64;
    let mi = ((secs - h as f64 * 3600.0) / 60.0).floor() as i64;
    let s = secs - h as f64 * 3600.0 - mi as f64 * 60.0;
    let mut out = String::new();
    for t in toks {
        match t {
            Tk::Lit(s) => out.push_str(s),
            Tk::Y(n) => if *n <= 2 { out.push_str(&format!("{:02}", y.rem_euclid(100))) } else { out.push_str(&format!("{:04}", y)) },
            Tk::Mo(n) => match n { 1 => out.push_str(&m.to_string()), 2 => out.push_str(&format!("{:02}", m)), 3 => out.push_str(&MONTHS[(m - 1) as usize][..3]), 4 => out.push_str(MONTHS[(m - 1) as usize]), _ => out.push_str(&MONTHS[(m - 1) as usize][..1]) },
            Tk::D(n) => match n { 1 => out.push_str(&d.to_string()), 2 => out.push_str(&format!("{:02}", d)), 3 => out.push_str(&DAYS[wd as usize][..3]), _ => out.push_str(DAYS[wd as usize]) },
            Tk::H(n) => { let hh = if has_ampm { let v = h % 12; if v == 0 { 12 } else { v } } else { h }; if *n >= 2 { out.push_str(&format!("{:02}", hh)) } else { out.push_str(&hh.to_string()) } }
            Tk::Mi(n) => if *n >= 2 { out.push_str(&format!("{:02}", mi)) } else { out.push_str(&mi.to_string()) },
            Tk::S(n) => { let si = s.floor() as i64; if *n >= 2 { out.push_str(&format!("{:02}", si)) } else { out.push_str(&si.to_string()) } }
            Tk::Frac(n) => { let f = s - s.floor(); let v = (f * 10f64.powi(*n as i32)).floor() as i64; out.push('.'); out.push_str(&format!("{:0>w$}", v, w = *n)); }
            Tk::Ampm(long) => { let pm = h >= 12; out.push_str(if *long { if pm { "PM" } else { "AM" } } else if pm { "P" } else { "A" }); }
            _ => {}
        }
    }
    out
}

fn parse_date_str(t: &str) -> Option<f64> {
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let fix = |y: i64, len: usize| -> i64 { if len <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y } };
    let chk = |y: i64, m: i64, d: i64| -> Option<f64> {
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) { return None; }
        let s = serial_from_ymd(y, m, d); let (y2, m2, d2) = ymd_from_serial(s);
        if y2 == y && m2 == m && d2 == d { Some(s) } else { None }
    };
    if t.contains('/') {
        let p: Vec<&str> = t.split('/').collect();
        if p.len() == 3 {
            if let (Ok(m), Ok(d), Ok(y)) = (p[0].trim().parse::<i64>(), p[1].trim().parse::<i64>(), p[2].trim().parse::<i64>()) {
                return chk(fix(y, p[2].trim().len()), m, d);
            }
        }
        return None;
    }
    let p: Vec<&str> = t.split('-').collect();
    if p.len() == 3 {
        if let (Ok(d), Ok(y)) = (p[0].trim().parse::<i64>(), p[2].trim().parse::<i64>()) {
            let ml = p[1].trim().to_lowercase();
            if ml.len() >= 3 { if let Some(mi) = months.iter().position(|m| ml.starts_with(m)) { return chk(fix(y, p[2].trim().len()), mi as i64 + 1, d); } }
        }
    }
    None
}

fn iso_date(t: &str) -> Option<f64> {
    let p: Vec<&str> = t.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 {
        if let (Ok(y), Ok(m), Ok(d)) = (p[0].parse::<i64>(), p[1].parse::<i64>(), p[2].parse::<i64>()) {
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                let s = serial_from_ymd(y, m, d); let (y2, m2, d2) = ymd_from_serial(s);
                if y2 == y && m2 == m && d2 == d { return Some(s); }
            }
        }
    }
    None
}
fn parse_time_str(t: &str) -> Option<f64> {
    let up = t.to_ascii_uppercase();
    let (body, pm) = if let Some(b) = up.strip_suffix("PM") { (b.trim(), Some(true)) } else if let Some(b) = up.strip_suffix("AM") { (b.trim(), Some(false)) } else { (up.as_str(), None) };
    let p: Vec<&str> = body.split(':').collect();
    if p.len() < 2 || p.len() > 3 { return None; }
    let mut v = [0f64; 3];
    for (i, x) in p.iter().enumerate() {
        if x.is_empty() || !x.bytes().all(|c| c.is_ascii_digit() || (i == 2 && c == b'.')) { return None; }
        v[i] = x.parse::<f64>().ok()?;
    }
    if v[1] >= 60.0 || v[2] >= 60.0 { return None; }
    let mut h = v[0];
    if let Some(pm) = pm { if h < 1.0 || h > 12.0 { return None; } if h == 12.0 { h = 0.0; } if pm { h += 12.0; } }
    Some((h * 3600.0 + v[1] * 60.0 + v[2]) / 86400.0)
}
