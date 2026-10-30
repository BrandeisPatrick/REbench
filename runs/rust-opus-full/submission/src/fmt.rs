use crate::{serial_to_ymd, Er, V, to_num_pub};

pub fn num_to_str(n: f64) -> String {
    if n == 0.0 || !n.is_finite() { return "0".into(); }
    if n == n.trunc() && n.abs() < 1e16 { return format!("{}", n as i64); }
    let a = n.abs();
    let s17 = { let t = format!("{:e}", n); let (m, e) = t.split_once('e').unwrap(); let mut m = m.to_string(); if !m.contains('.') { m.push_str(".0"); } while m.bytes().filter(|b| b.is_ascii_digit()).count() < 17 { m.push('0'); } format!("{}e{}", m, e) };
    let s = { let v: f64 = s17.parse().unwrap(); let (m, e) = s17.split_once('e').unwrap(); let neg = m.starts_with('-'); let dg: Vec<u8> = m.bytes().filter(|b| b.is_ascii_digit()).collect();
        // round 17 digits to 15 half-up
        let mut d15: Vec<u8> = dg[..15].to_vec(); let mut ex: i32 = e.parse().unwrap();
        if dg[15] >= b'5' { let mut i = 14i32; loop { if i < 0 { d15.insert(0, b'1'); d15.pop(); ex += 1; break; } if d15[i as usize] == b'9' { d15[i as usize] = b'0'; i -= 1; } else { d15[i as usize] += 1; break; } } }
        let _ = v; format!("{}{}.{}e{}", if neg { "-" } else { "" }, d15[0] as char, std::str::from_utf8(&d15[1..]).unwrap(), ex) };
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let _ = a;
    if exp >= 16 || exp < -20 {
        // scientific
        let mut o = String::new(); if neg { o.push('-'); }
        o.push_str(&digits[..1]);
        if digits.len() > 1 { o.push('.'); o.push_str(&digits[1..]); }
        o.push('E'); o.push(if exp < 0 { '-' } else { '+' }); o.push_str(&format!("{:03}", exp.abs()));
        return o;
    }
    let mut o = String::new(); if neg { o.push('-'); }
    if exp < 0 {
        o.push_str("0."); for _ in 0..(-exp - 1) { o.push('0'); } o.push_str(digits);
    } else {
        let e = exp as usize;
        if digits.len() <= e + 1 { o.push_str(digits); for _ in 0..(e + 1 - digits.len()) { o.push('0'); } }
        else { o.push_str(&digits[..e + 1]); o.push('.'); o.push_str(&digits[e + 1..]); }
    }
    o
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn split_sections(f: &str) -> Vec<String> {
    let mut v = Vec::new(); let mut cur = String::new(); let mut q = false;
    let mut it = f.chars().peekable();
    while let Some(c) = it.next() {
        if c == '"' { q = !q; cur.push(c); continue; }
        if c == '\\' && !q { cur.push(c); if let Some(n) = it.next() { cur.push(n); } continue; }
        if c == ';' && !q { v.push(std::mem::take(&mut cur)); continue; }
        cur.push(c);
    }
    v.push(cur); v
}

pub fn text_fmt(v: &V, f: &str) -> Result<String, Er> {
    let secs = split_sections(f);
    let n = match v {
        V::S(s) => {
            if let Ok(x) = to_num_pub(v) { x } else {
                if secs.len() >= 4 { return Ok(secs[3].replace('@', s).replace('"', "")); }
                return Ok(s.to_string());
            }
        }
        _ => to_num_pub(v)?,
    };
    let (sec, n2, _neg_handled) = if n < 0.0 && secs.len() >= 2 { (secs[1].clone(), -n, true) } else if n == 0.0 && secs.len() >= 3 { (secs[2].clone(), n, true) } else { (secs[0].clone(), n, false) };
    let sec = sec.as_str();
    if sec.eq_ignore_ascii_case("general") || sec == "@" { return Ok(if n < 0.0 && secs.len() < 2 { format!("-{}", num_to_str(n2.abs())) } else { num_to_str(n2) }); }
    // detect date
    let low = strip_quoted(sec).to_lowercase();
    if low.contains('y') || low.contains('d') || (low.contains('m') && !low.contains('0') && !low.contains('#')) || low.contains('h') || low.contains('s') && !low.contains('0') {
        return Ok(date_fmt(n2, sec));
    }
    Ok(num_fmt(n2, sec, n < 0.0 && secs.len() < 2))
}

fn strip_quoted(s: &str) -> String {
    let mut o = String::new(); let mut q = false; let mut esc = false;
    for c in s.chars() { if esc { esc = false; continue; } if c == '\\' { esc = true; continue; } if c == '"' { q = !q; continue; } if !q { o.push(c); } }
    o
}

fn date_fmt(n: f64, f: &str) -> String {
    let days = n.floor() as i64;
    let (y, m, d) = serial_to_ymd(days);
    { let l = f.to_lowercase(); if y > 32767 && (l.contains('y') || l.contains('d') || (l.contains('m') && !l.contains('h') && !l.contains('s'))) { return "#FMT".into(); } }
    let secs_total = ((n - n.floor()) * 86400.0 + 1e-6).floor() as i64;
    let (hh, mi, ss) = (secs_total / 3600, (secs_total / 60) % 60, secs_total % 60);
    let wd = ((days + 6).rem_euclid(7)) as usize;
    let ch: Vec<char> = f.chars().collect();
    let mut o = String::new(); let mut i = 0;
    let has_ampm = f.to_lowercase().contains("am/pm");
    let mut last_h = false;
    while i < ch.len() {
        let c = ch[i]; let lc = c.to_ascii_lowercase();
        if c == '"' { i += 1; while i < ch.len() && ch[i] != '"' { o.push(ch[i]); i += 1; } i += 1; continue; }
        if c == '\\' { if i + 1 < ch.len() { o.push(ch[i + 1]); } i += 2; continue; }
        if lc == 'y' || lc == 'm' || lc == 'd' || lc == 'h' || lc == 's' {
            let mut k = 0; while i + k < ch.len() && ch[i + k].to_ascii_lowercase() == lc { k += 1; }
            match lc {
                'y' => if k <= 2 { o.push_str(&format!("{:02}", y % 100)) } else { o.push_str(&format!("{}", y)) },
                'd' => match k { 1 => o.push_str(&d.to_string()), 2 => o.push_str(&format!("{:02}", d)), 3 => o.push_str(&DAYS[wd][..3]), _ => o.push_str(DAYS[wd]) },
                'm' => {
                    // minutes if after h or before s
                    let rest: String = ch[i + k..].iter().collect();
                    let is_min = k <= 2 && (last_h || rest.trim_start_matches(|c: char| !c.is_alphabetic()).to_lowercase().starts_with('s'));
                    if is_min { if k == 1 { o.push_str(&mi.to_string()) } else { o.push_str(&format!("{:02}", mi)) } }
                    else { match k { 1 => o.push_str(&m.to_string()), 2 => o.push_str(&format!("{:02}", m)), 3 => o.push_str(&MONTHS[m as usize - 1][..3]), 5 => o.push_str(&MONTHS[m as usize - 1][..1]), _ => o.push_str(MONTHS[m as usize - 1]) } }
                }
                'h' => { let h = if has_ampm { let h = hh % 12; if h == 0 { 12 } else { h } } else { hh }; if k == 1 { o.push_str(&h.to_string()) } else { o.push_str(&format!("{:02}", h)) } }
                _ => { if k == 1 { o.push_str(&ss.to_string()) } else { o.push_str(&format!("{:02}", ss)) } }
            }
            last_h = lc == 'h';
            i += k; continue;
        }
        if f[..].len() > 0 && ch[i..].iter().collect::<String>().to_lowercase().starts_with("am/pm") { o.push_str(if hh < 12 { "AM" } else { "PM" }); i += 5; continue; }
        o.push(c); i += 1;
    }
    o
}

fn num_fmt(n: f64, f: &str, neg: bool) -> String {
    // parse format: prefix literal, number part, suffix literal
    let ch: Vec<char> = f.chars().collect();
    let mut prefix = String::new(); let mut suffix = String::new(); let mut numpart = String::new();
    let mut i = 0; let mut seen_num = false; let mut done_num = false;
    let mut pct = 0;
    while i < ch.len() {
        let c = ch[i];
        if c == '"' { i += 1; let mut lit = String::new(); while i < ch.len() && ch[i] != '"' { lit.push(ch[i]); i += 1; } i += 1; if seen_num { suffix.push_str(&lit); done_num = true; } else { prefix.push_str(&lit); } continue; }
        if c == '\\' { if i + 1 < ch.len() { if seen_num { suffix.push(ch[i + 1]); done_num = true; } else { prefix.push(ch[i + 1]); } } i += 2; continue; }
        if c == '*' { i += 2; continue; }
        if c == '_' { i += 2; continue; }
        if (c == '0' || c == '#' || c == '?' || c == '.' || c == ',' || ((c == 'E' || c == 'e') && seen_num) ) && !done_num {
            seen_num = true; numpart.push(c);
            if c == 'E' || c == 'e' { i += 1; while i < ch.len() && (ch[i] == '+' || ch[i] == '-' || ch[i] == '0' || ch[i] == '#') { numpart.push(ch[i]); i += 1; } continue; }
            i += 1; continue;
        }
        if c == '%' { pct += 1; }
        if seen_num { suffix.push(c); done_num = true; } else { prefix.push(c); }
        i += 1;
    }
    let mut x = n.abs() * 100f64.powi(pct);
    if !seen_num { let mut o = String::new(); if neg { o.push('-'); } o.push_str(&prefix); o.push_str(&suffix); return o; }
    let body;
    if let Some(ep) = numpart.find(|c| c == 'E' || c == 'e') {
        let mant = &numpart[..ep]; let expf = &numpart[ep + 1..];
        let (ip, dp) = mant.split_once('.').unwrap_or((mant, ""));
        let dec = dp.len();
        let mut e = if x == 0.0 { 0 } else { x.log10().floor() as i32 };
        let mut m = x / 10f64.powi(e);
        let r = (m * 10f64.powi(dec as i32)).round() / 10f64.powi(dec as i32);
        m = r; if m >= 10.0 { m /= 10.0; e += 1; }
        let _ = ip;
        let ms = format!("{:.*}", dec, m);
        let ed = expf.trim_start_matches(|c| c == '+' || c == '-').len();
        let sign = if e < 0 { "-" } else if expf.starts_with('+') { "+" } else { "" };
        body = format!("{}E{}{:0width$}", ms, sign, e.abs(), width = ed);
    } else {
        let mut np = numpart.clone();
        // trailing commas scale
        while np.ends_with(',') { np.pop(); x /= 1000.0; }
        let thousands = np.contains(',');
        let np: String = np.chars().filter(|&c| c != ',').collect();
        let (ip, dp) = np.split_once('.').unwrap_or((&np, ""));
        let has_dot = np.contains('.');
        let dmin = dp.chars().filter(|&c| c == '0').count(); let dmax = dp.len();
        let imin = ip.chars().filter(|&c| c == '0').count();
        let s = if x >= 1e15 {
            let e = format!("{:.14e}", x); let (m, ex) = e.split_once('e').unwrap(); let ex: usize = ex.parse().unwrap_or(0);
            let dg: String = m.chars().filter(|c| c.is_ascii_digit()).collect();
            let mut ip: String = dg.chars().take(ex + 1).collect(); while ip.len() < ex + 1 { ip.push('0'); }
            if dmax > 0 { format!("{}.{}", ip, "0".repeat(dmax)) } else { ip }
        } else { format!("{:.*}", dmax, round_half_up(x, dmax as i32)) };
        let (is, ds) = s.split_once('.').unwrap_or((&s, ""));
        let mut ds = ds.to_string();
        while ds.len() > dmin && ds.ends_with('0') { ds.pop(); }
        let mut is = is.trim_start_matches('0').to_string();
        while is.len() < imin { is.insert(0, '0'); }
        if thousands { let b: Vec<char> = is.chars().collect(); let mut t = String::new(); for (k, c) in b.iter().enumerate() { if k > 0 && (b.len() - k) % 3 == 0 { t.push(','); } t.push(*c); } is = t; }
        body = if has_dot && (!ds.is_empty() || dp.len() > 0) { if ds.is_empty() && dmin == 0 { is } else { format!("{}.{}", is, ds) } } else { is };
    }
    let mut o = String::new();
    let isz = body.chars().all(|c| c == '0' || c == '.' || c == ',' || c == 'E' || c == '+' || c == '-');
    if neg && !(isz && !body.contains('E')) { o.push('-'); }
    o.push_str(&prefix); o.push_str(&body); o.push_str(&suffix); o
}

fn round_half_up(x: f64, d: i32) -> f64 {
    let f = 10f64.powi(d); let y = x * f;
    let yr: f64 = format!("{:.14e}", y).parse().unwrap_or(y);
    (yr + 0.5).floor().max(0.0) / f
}
