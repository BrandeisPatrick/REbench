// number formatting helpers

pub fn sig15(x: f64) -> (String, i32) {
    // shortest round-trip digits, then round half-up to 15 significant digits
    if !x.is_finite() { return ("0".into(), 0); }
    let s = format!("{:e}", x.abs());
    let (m, e) = s.split_once('e').unwrap();
    let mut e: i32 = e.parse().unwrap();
    let mut digits: Vec<u8> = m.bytes().filter(|c| *c != b'.').map(|c| c - b'0').collect();
    if digits.len() > 15 {
        let up = digits[15] >= 5;
        digits.truncate(15);
        if up {
            let mut i = 14i32;
            loop {
                if i < 0 { digits.insert(0, 1); digits.truncate(15); e += 1; break; }
                if digits[i as usize] == 9 { digits[i as usize] = 0; i -= 1; } else { digits[i as usize] += 1; break; }
            }
        }
    }
    while digits.len() > 1 && *digits.last().unwrap() == 0 { digits.pop(); }
    (digits.iter().map(|d| (b'0' + d) as char).collect(), e)
}

pub fn num_to_str(x: f64) -> String {
    if x == 0.0 { return "0".into(); }
    if !x.is_finite() { return "#NUM!".into(); }
    let (d, e) = sig15(x);
    let mut out = String::new();
    if x < 0.0 { out.push('-'); }
    if e >= 16 || e < -14 || (e >= 15 && x.fract() != 0.0) {
        out.push_str(&d[..1]);
        if d.len() > 1 { out.push('.'); out.push_str(&d[1..]); }
        out.push('E');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&format!("{:03}", e.abs()));
    } else if e >= 0 {
        let e = e as usize;
        if d.len() <= e + 1 {
            out.push_str(&d);
            for _ in 0..(e + 1 - d.len()) { out.push('0'); }
        } else {
            out.push_str(&d[..e + 1]);
            out.push('.');
            out.push_str(&d[e + 1..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-e - 1) { out.push('0'); }
        out.push_str(&d);
    }
    out
}

pub fn r15(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 { return x; }
    let (d, e) = sig15(x);
    format!("{}{}.{}e{}", if x < 0.0 { "-" } else { "" }, &d[..1], if d.len() > 1 { &d[1..] } else { "0" }, e).parse().unwrap_or(x)
}
pub fn round_half_away(x: f64, dec: i32) -> f64 {
    if !x.is_finite() { return x; }
    if dec > 15 { return x; }
    let f = 10f64.powi(dec.abs());
    let v = if dec >= 0 { x * f } else { x / f };
    if !v.is_finite() { return x; }
    // snap to 15 significant digits to kill representation noise
    let v15: f64 = { let (d, e) = sig15(v); format!("{}{}e{}", if v < 0.0 { "-" } else { "" }, format!("{}.{}", &d[..1], if d.len() > 1 { &d[1..] } else { "0" }), e).parse().unwrap_or(v) };
    let r = v15.round();
    if dec >= 0 { r / f } else { r * f }
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}
// serial 0 = 1899-12-30
pub fn serial_to_ymd(s: i64) -> (i64, u32, u32) { civil_from_days(s + days_from_civil(1899, 12, 30)) }
pub fn ymd_to_serial(y: i64, m: u32, d: u32) -> i64 { days_from_civil(y, m, d) - days_from_civil(1899, 12, 30) }
pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30,
        _ => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 } }
}

const MONTHS: [&str; 12] = ["January","February","March","April","May","June","July","August","September","October","November","December"];
const DAYS: [&str; 7] = ["Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"];

fn split_sections(f: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut inq = false;
    let mut prev_bs = false;
    for c in f.chars() {
        if prev_bs { out.last_mut().unwrap().push(c); prev_bs = false; continue; }
        if c == '\\' { prev_bs = true; out.last_mut().unwrap().push(c); continue; }
        if c == '"' { inq = !inq; }
        if c == ';' && !inq { out.push(String::new()); } else { out.last_mut().unwrap().push(c); }
    }
    out
}

fn strip_cond(sec: &str) -> String {
    // remove [color]/[condition] prefixes
    let mut out = String::new();
    let mut inb = false;
    for c in sec.chars() {
        if c == '[' { inb = true; continue; }
        if c == ']' { inb = false; continue; }
        if !inb { out.push(c); }
    }
    out
}

pub fn text_format(x: f64, fmt: &str) -> Option<String> {
    let secs = split_sections(fmt);
    let (sec, use_abs, neg_default) = if x < 0.0 && secs.len() >= 2 { (secs[1].clone(), true, false) }
        else if x == 0.0 && secs.len() >= 3 { (secs[2].clone(), true, false) }
        else { (secs[0].clone(), x < 0.0, true) };
    let sec = strip_cond(&sec);
    let _ = neg_default;
    let val = if use_abs && !(x < 0.0 && secs.len() < 2) && x < 0.0 { -x } else { x };
    let neg_sign = x < 0.0 && secs.len() < 2;
    let low = sec.to_ascii_lowercase();
    if sec.trim().is_empty() && !fmt.is_empty() { return Some(String::new()); }
    if sec.eq_ignore_ascii_case("general") { return Some(num_to_str(x)); }
    // detect date format: letters y m d h s outside quotes
    let mut isdate = false;
    {
        let mut inq = false; let mut bs = false;
        for c in low.chars() {
            if bs { bs = false; continue; }
            if c == '\\' { bs = true; continue; }
            if c == '"' { inq = !inq; continue; }
            if inq { continue; }
            if matches!(c, 'y' | 'd' | 'h' | 's') || c == 'm' { isdate = true; }
            if c == 'e' { isdate = false; break; }
        }
        if low.contains("am/pm") { isdate = true; }
    }
    if isdate { return Some(date_format(x, &sec)); }
    // numeric
    let chars: Vec<char> = sec.chars().collect();
    let mut pct = 0;
    // find numeric core
    let mut i = 0;
    let mut prefix = String::new();
    let mut core = String::new();
    let mut suffix = String::new();
    let mut state = 0; // 0 prefix, 1 core, 2 suffix
    let mut sci = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                let mut j = i + 1; let mut t = String::new();
                while j < chars.len() && chars[j] != '"' { t.push(chars[j]); j += 1; }
                i = j;
                if state == 0 { prefix.push_str(&t); } else { state = 2; suffix.push_str(&t); }
            }
            '\\' => {
                i += 1;
                if i < chars.len() { if state == 0 { prefix.push(chars[i]); } else { state = 2; suffix.push(chars[i]); } }
            }
            '0' | '#' | '?' => {
                if state == 2 { return None; }
                state = 1; core.push(c);
            }
            '.' | ',' if state <= 1 && (state == 1 || chars.get(i + 1).map_or(false, |n| matches!(n, '0' | '#' | '?'))) => { state = 1; core.push(c); }
            'E' | 'e' if state == 1 && matches!(chars.get(i + 1), Some('+') | Some('-')) => {
                sci = true;
                core.push('E'); core.push(chars[i + 1]);
                i += 1;
                while i + 1 < chars.len() && chars[i + 1] == '0' { core.push('0'); i += 1; }
            }
            '%' => { pct += 1; if state == 0 { prefix.push('%'); } else { state = 2; suffix.push('%'); } }
            '_' => { i += 1; if state == 0 { prefix.push(' '); } else { state = 2; suffix.push(' '); } }
            '*' => { i += 1; }
            '@' => { return Some(num_to_str(x)); }
            _ => { if state == 0 { prefix.push(c); } else { state = 2; suffix.push(c); } }
        }
        i += 1;
    }
    let mut v = val;
    for _ in 0..pct { v *= 100.0; }
    let mut res = String::new();
    if neg_sign && v != 0.0 { res.push('-'); }
    res.push_str(&prefix);
    if sci {
        let (mant_pat, exp_pat) = core.split_once('E').unwrap();
        let dec = mant_pat.split_once('.').map_or(0, |(_, d)| d.chars().filter(|c| matches!(c, '0' | '#' | '?')).count());
        let intd = mant_pat.split('.').next().unwrap().chars().filter(|c| matches!(c, '0' | '#' | '?')).count().max(1);
        let mut e = 0i32;
        let mut m = v.abs();
        if m != 0.0 {
            e = m.log10().floor() as i32 - (intd as i32 - 1);
            m = v.abs() / 10f64.powi(e);
            let mr = round_half_away(m, dec as i32);
            if mr >= 10f64.powi(intd as i32) { e += 1; m = v.abs() / 10f64.powi(e); }
        }
        let mr = round_half_away(m, dec as i32);
        res.push_str(&format!("{:.*}", dec, mr));
        res.push('E');
        res.push(if e < 0 { '-' } else if exp_pat.starts_with('+') { '+' } else { '+' });
        let ed = exp_pat.len() - 1;
        res.push_str(&format!("{:0w$}", e.abs(), w = ed.max(1)));
    } else {
        let (ip, dp) = match core.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (core.clone(), String::new()) };
        let dec = dp.chars().filter(|c| matches!(c, '0' | '#' | '?')).count();
        let thousands = ip.contains(',');
        let minint = ip.chars().filter(|c| *c == '0').count();
        let mut scale_div = 0;
        // trailing commas in ip scale by 1000 (ignored)
        let _ = &mut scale_div;
        let r = round_half_away(v.abs(), dec as i32);
        let s = format!("{:.*}", dec, r);
        let (is, ds) = match s.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (s.clone(), String::new()) };
        let mut is = if is == "0" && minint == 0 { String::new() } else { is };
        while is.len() < minint { is.insert(0, '0'); }
        if thousands {
            let mut o = String::new();
            let n = is.len();
            for (k, c) in is.chars().enumerate() {
                if k > 0 && (n - k) % 3 == 0 { o.push(','); }
                o.push(c);
            }
            is = o;
        }
        res.push_str(&is);
        if !dp.is_empty() || dec > 0 {
            // drop trailing '#' zeros
            let mut ds = ds;
            let pat: Vec<char> = dp.chars().filter(|c| matches!(c, '0' | '#' | '?')).collect();
            while ds.len() > 0 && ds.len() > 0 && pat.get(ds.len() - 1) != Some(&'0') && ds.ends_with('0') { ds.pop(); }
            if !ds.is_empty() || core.ends_with('.') || (dec > 0 && false) { res.push('.'); res.push_str(&ds); }
            else if dec == 0 && core.contains('.') { res.push('.'); }
        }
    }
    res.push_str(&suffix);
    Some(res)
}

fn date_format(x: f64, sec: &str) -> String {
    let total = x;
    let mut days = total.floor() as i64;
    if total.abs() > 11900000.0 { let l = sec.to_ascii_lowercase(); if l.contains('y') || l.contains('d') || !(l.contains('h') || l.contains('s')) { return "#FMT".to_string(); } return "00:00:00".to_string(); }
    let mut secs_f = ((total - total.floor()) * 86400.0 + 1e-6).floor() as i64;
    if secs_f >= 86400 { secs_f -= 86400; days += 1; }
    let (y, m, d) = serial_to_ymd(days);
    let wd = (((days - 1) % 7) + 7) % 7;
    let hh = secs_f / 3600; let mi = (secs_f / 60) % 60; let ss = secs_f % 60;
    let ampm = sec.to_ascii_lowercase().contains("am/pm") || sec.to_ascii_lowercase().contains("a/p");
    let chars: Vec<char> = sec.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last_h = false;
    while i < chars.len() {
        let c = chars[i];
        let lc = c.to_ascii_lowercase();
        let mut run = 1;
        while i + run < chars.len() && chars[i + run].to_ascii_lowercase() == lc { run += 1; }
        match lc {
            'y' => { if run <= 2 { out.push_str(&format!("{:02}", y % 100)); } else { out.push_str(&format!("{:04}", y)); } i += run; last_h = false; }
            'm' => {
                // minutes if previous token was h or next is s
                let mut nxt = i + run;
                while nxt < chars.len() && !chars[nxt].is_ascii_alphabetic() { nxt += 1; }
                let is_min = last_h || (nxt < chars.len() && chars[nxt].to_ascii_lowercase() == 's');
                if is_min { out.push_str(&format!("{:02}", mi)); }
                else { match run { 1 => out.push_str(&m.to_string()), 2 => out.push_str(&format!("{:02}", m)),
                    3 => out.push_str(&MONTHS[m as usize - 1][..3]), 4 => out.push_str(MONTHS[m as usize - 1]),
                    _ => out.push_str(&MONTHS[m as usize - 1][..1]) } }
                i += run; last_h = false;
            }
            'd' => { match run { 1 => out.push_str(&d.to_string()), 2 => out.push_str(&format!("{:02}", d)),
                3 => out.push_str(&DAYS[wd as usize][..3]), _ => out.push_str(DAYS[wd as usize]) } i += run; last_h = false; }
            'h' => { let h = if ampm { let h12 = hh % 12; if h12 == 0 { 12 } else { h12 } } else { hh };
                if run == 1 { out.push_str(&h.to_string()); } else { out.push_str(&format!("{:02}", h)); } i += run; last_h = true; }
            's' => { if run == 1 { out.push_str(&ss.to_string()); } else { out.push_str(&format!("{:02}", ss)); } i += run; last_h = false; }
            'a' => {
                let rest: String = chars[i..].iter().collect::<String>().to_ascii_lowercase();
                if rest.starts_with("am/pm") { out.push_str(if hh < 12 { "AM" } else { "PM" }); i += 5; }
                else if rest.starts_with("a/p") { out.push_str(if hh < 12 { "A" } else { "P" }); i += 3; }
                else { out.push(c); i += 1; }
            }
            '"' => { let mut j = i + 1; while j < chars.len() && chars[j] != '"' { out.push(chars[j]); j += 1; } i = j + 1; }
            '\\' => { if i + 1 < chars.len() { out.push(chars[i + 1]); } i += 2; }
            _ => { out.push(c); i += 1; }
        }
    }
    out
}
