pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
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
const EPOCH: i64 = -25569; // days_from_civil(1899,12,30)
pub fn ymd_to_serial(y: i64, m: i64, d: i64) -> f64 { (days_from_civil(y, m, d) - EPOCH) as f64 }
pub fn serial_to_ymd(s: f64) -> (i64, i64, i64) { civil_from_days(s.floor() as i64 + EPOCH) }
pub fn is_leap(y: i64) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }
pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, _ => if is_leap(y) { 29 } else { 28 } }
}

const MON3: [&str; 12] = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
pub fn parse_date_str(t: &str) -> Option<f64> {
    let parts: Vec<&str> = t.split(|c| c == '-' || c == ' ' || c == '/').collect();
    if parts.len() == 3 && parts[1].len() >= 3 && parts[1].chars().all(|c| c.is_ascii_alphabetic()) {
        let ml = parts[1].to_ascii_lowercase();
        let m = MON3.iter().position(|x| ml.starts_with(x))? as i64 + 1;
        let d: i64 = parts[0].parse().ok()?;
        let mut y: i64 = parts[2].parse().ok()?;
        if parts[2].len() <= 2 { y += if y < 30 { 2000 } else { 1900 }; }
        if d < 1 || d > days_in_month(y, m) { return None; }
        return Some(ymd_to_serial(y, m, d));
    }
    if t.contains(':') {
        let p: Vec<&str> = t.split(':').collect();
        if p.len() >= 2 && p.len() <= 3 && p.iter().all(|x| !x.is_empty() && x.len() <= 2 && x.bytes().all(|c| c.is_ascii_digit())) {
            let h: f64 = p[0].parse().ok()?; let m: f64 = p[1].parse().ok()?; let sec: f64 = if p.len() == 3 { p[2].parse().ok()? } else { 0.0 };
            if m >= 60.0 || sec >= 60.0 { return None; }
            return Some((h * 3600.0 + m * 60.0 + sec) / 86400.0);
        }
        return None;
    }
    if let Some(num) = t.strip_suffix('-') {
        if !num.is_empty() && num.bytes().all(|c| c.is_ascii_digit() || c == b'.') { return num.parse::<f64>().ok().map(|x| -x); }
    }
    // yyyy-mm-dd or mm/dd/yyyy
    let parts: Vec<&str> = t.split(|c| c == '-' || c == '/').collect();
    if parts.len() != 3 { return None; }
    let n: Vec<i64> = parts.iter().filter_map(|p| p.trim().parse().ok()).collect();
    if n.len() != 3 { return None; }
    let (y, m, d) = if t.contains('-') && parts[0].len() == 4 { (n[0], n[1], n[2]) } else if t.contains('/') && !t.contains('-') && parts[0].len() <= 2 {
        let mut y = n[2]; if parts[2].len() <= 2 { y += if y < 30 { 2000 } else { 1900 }; } (y, n[0], n[1]) } else { return None };
    if m < 1 || m > 12 || d < 1 || d > days_in_month(y, m) { return None; }
    Some(ymd_to_serial(y, m, d))
}

// round to 15 significant digits
pub fn round15(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() { return x; }
    let s = format!("{:.14e}", x);
    s.parse().unwrap_or(x)
}

pub fn num_to_str(x: f64) -> String {
    if x == 0.0 { return "0".into(); }
    if !x.is_finite() { return "#NUM!".into(); }
    if x.abs() < 1e16 && x.fract() == 0.0 { return format!("{}", x as i64); }
    let s = format!("{:e}", x);
    let (mant, exp) = s.split_once('e').unwrap();
    let mut exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let mut digits: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() > 15 {
        let up = digits[15] >= b'5';
        digits.truncate(15);
        if up {
            let mut i = 15;
            loop {
                if i == 0 { digits.insert(0, b'1'); digits.truncate(15); exp += 1; break; }
                i -= 1;
                if digits[i] == b'9' { digits[i] = b'0'; } else { digits[i] += 1; break; }
            }
        }
    }
    let digits = String::from_utf8(digits).unwrap();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut o = String::new();
    if neg { o.push('-'); }
    if exp >= 15 || exp < -14 {
        o.push_str(&digits[..1]);
        if digits.len() > 1 { o.push('.'); o.push_str(&digits[1..]); }
        o.push_str(&format!("E{}{:03}", if exp < 0 { '-' } else { '+' }, exp.abs()));
        return o;
    }
    if exp < 0 {
        o.push_str("0.");
        for _ in 0..(-exp - 1) { o.push('0'); }
        o.push_str(digits);
    } else {
        let e = exp as usize;
        if digits.len() <= e + 1 {
            o.push_str(digits);
            for _ in 0..(e + 1 - digits.len()) { o.push('0'); }
        } else {
            o.push_str(&digits[..e + 1]); o.push('.'); o.push_str(&digits[e + 1..]);
        }
    }
    o
}

pub fn round_half_away(x: f64, d: i32) -> f64 {
    let p = 10f64.powi(d.abs());
    let y = if d >= 0 { round15(x * p) } else { round15(x / p) };
    let r = y.abs().round() * y.signum();
    if d >= 0 { r / p } else { r * p }
}

fn group(int: &str) -> String {
    let b = int.as_bytes();
    let mut o = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 { o.push(','); }
        o.push(*c as char);
    }
    o
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn fmt_number(x: f64, f: &str) -> Option<String> {
    // supports [#,]0[.0+][%] and 0.00E+00
    let mut f = f.to_string();
    let mut x = x;
    let pct = f.ends_with('%');
    if pct { f.pop(); x *= 100.0; }
    if let Some(ep) = f.to_uppercase().find("E+") {
        let mpart = &f[..ep];
        let dec = mpart.split_once('.').map(|(_, d)| d.len()).unwrap_or(0);
        let edig = f.len() - ep - 2;
        if x == 0.0 { return Some(format!("{}E+{}", format!("{:.*}", dec, 0.0), "0".repeat(edig))); }
        let mut e = x.abs().log10().floor() as i32;
        let mut m = round_half_away(x / 10f64.powi(e), dec as i32);
        if m.abs() >= 10.0 { e += 1; m = round_half_away(x / 10f64.powi(e), dec as i32); }
        let es = format!("{:0w$}", e.abs(), w = edig);
        return Some(format!("{:.*}E{}{}", dec, m, if e < 0 { '-' } else { '+' }, es));
    }
    if !f.chars().all(|c| matches!(c, '#' | '0' | ',' | '.')) { return None; }
    let (ip, dp) = match f.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (f.clone(), String::new()) };
    let dec = dp.len();
    let grp = ip.contains(',');
    let minint = ip.chars().filter(|c| *c == '0').count();
    let r = round_half_away(x, dec as i32);
    let neg = r < 0.0;
    let s = format!("{:.*}", dec, r.abs());
    let (is, ds) = match s.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (s.clone(), String::new()) };
    let mut is = is.trim_start_matches('0').to_string();
    while is.len() < minint { is.insert(0, '0'); }
    if grp { is = group(&is); }
    let mut o = String::new();
    if neg && (r != 0.0) { o.push('-'); }
    o.push_str(&is);
    if dec > 0 { o.push('.'); o.push_str(&ds); }
    if pct { o.push('%'); }
    Some(o)
}

fn fmt_date(x: f64, f: &str) -> String {
    if x.abs() > 2958465.0 { return "#FMT".to_string(); }
    let (y, m, d) = serial_to_ymd(x);
    let wd = ((x.floor() as i64 % 7) + 7 + 6) % 7; // serial 1 = Sunday(LO: 1899-12-31 is Sunday)
    let frac = x - x.floor();
    let secs = (frac * 86400.0).round() as i64;
    let (hh, mi, ss) = (secs / 3600, (secs / 60) % 60, secs % 60);
    let b: Vec<char> = f.chars().collect();
    let mut o = String::new();
    let mut i = 0;
    let mut last_h = false;
    while i < b.len() {
        let c = b[i].to_ascii_lowercase();
        let mut n = 1;
        while i + n < b.len() && b[i + n].to_ascii_lowercase() == c { n += 1; }
        match c {
            'y' => { if n <= 2 { o.push_str(&format!("{:02}", y % 100)); } else { o.push_str(&format!("{}", y)); } }
            'm' => {
                if last_h || (i + n < b.len() && b[i + n..].iter().take_while(|c| !c.is_alphabetic()).count() < 3 && b[i + n..].iter().find(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase() == 's').unwrap_or(false)) {
                    o.push_str(&if n >= 2 { format!("{:02}", mi) } else { format!("{}", mi) });
                } else {
                    match n { 1 => o.push_str(&m.to_string()), 2 => o.push_str(&format!("{:02}", m)), 3 => o.push_str(&MONTHS[(m - 1) as usize][..3]), 5 => o.push_str(&MONTHS[(m - 1) as usize][..1]), _ => o.push_str(MONTHS[(m - 1) as usize]) }
                }
            }
            'd' => { match n { 1 => o.push_str(&d.to_string()), 2 => o.push_str(&format!("{:02}", d)), 3 => o.push_str(&DAYS[wd as usize][..3]), _ => o.push_str(DAYS[wd as usize]) } }
            'h' => { o.push_str(&if n >= 2 { format!("{:02}", hh) } else { hh.to_string() }); last_h = true; i += n; continue; }
            's' => { o.push_str(&if n >= 2 { format!("{:02}", ss) } else { ss.to_string() }); }
            _ => { for _ in 0..n { o.push(b[i]); } i += n; continue; }
        }
        last_h = false;
        i += n;
    }
    o
}

pub fn text_format(x: f64, f: &str) -> String {
    if f.is_empty() { return String::new(); }
    let lf = f.to_lowercase();
    if lf.contains('y') || lf.contains('d') || (lf.contains('m') && !lf.contains('0')) || lf.contains('h') {
        if lf.chars().any(|c| matches!(c, 'y' | 'd' | 'm' | 'h' | 's')) && !lf.contains('#') && !lf.contains('0') {
            return fmt_date(x, f);
        }
    }
    if let Some(s) = fmt_number(x, f) { return s; }
    if f.is_empty() || f.eq_ignore_ascii_case("general") { return num_to_str(x); }
    // literal text formats
    let mut o = String::new();
    let b: Vec<char> = f.chars().collect();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            '\\' => { if i + 1 < b.len() { o.push(b[i + 1]); } i += 2; continue; }
            '*' => { i += 2; continue; }
            '_' => { i += 2; continue; }
            '"' => { i += 1; while i < b.len() && b[i] != '"' { o.push(b[i]); i += 1; } }
            c => o.push(c),
        }
        i += 1;
    }
    o
}
