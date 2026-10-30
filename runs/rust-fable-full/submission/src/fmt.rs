// number formatting & dates
pub fn round_sig15(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let s = format!("{:.14e}", v);
    s.parse().unwrap_or(v)
}

// LibreOffice-like general number -> string (15 significant digits)
pub fn num_to_string(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if !v.is_finite() {
        return "#NUM!".to_string();
    }
    if v == v.trunc() && v.abs() < 1e16 {
        return format!("{}", v as i64);
    }
    // emulate LibreOffice: scale to 15 significant digits with f64 arithmetic, then round
    let av = v.abs();
    let mut e10 = av.log10().floor() as i32;
    if 10f64.powi(e10 + 1) <= av {
        e10 += 1;
    } else if 10f64.powi(e10) > av {
        e10 -= 1;
    }
    let k = 14 - e10;
    let scaled = if k >= 0 { av * 10f64.powi(k) } else { av / 10f64.powi(-k) };
    let mut r = scaled.round();
    let mut e10 = e10;
    if r >= 1e15 {
        r /= 10.0;
        r = r.round();
        e10 += 1;
    }
    let s = format!("{}{:.0}e{}", if v < 0.0 { "-" } else { "" }, r, e10);
    let (mant, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let mant = mant.trim_start_matches('-');
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if exp >= 16 || exp <= -16 || (exp == 15 && v != v.trunc()) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:03}", exp.abs()));
        return out;
    }
    if exp < 0 {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        let ip = (exp + 1) as usize;
        if digits.len() <= ip {
            out.push_str(digits);
            for _ in 0..(ip - digits.len()) {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..ip]);
            out.push('.');
            out.push_str(&digits[ip..]);
        }
    }
    out
}

pub fn round_half_away(v: f64, places: i32) -> f64 {
    if !v.is_finite() {
        return v;
    }
    let fac = 10f64.powi(places.abs());
    let x = if places >= 0 { v * fac } else { v / fac };
    let x = round_sig15(x);
    let r = x.round(); // half away from zero
    if places >= 0 {
        r / fac
    } else {
        r * fac
    }
}

// days since 1899-12-30
pub fn serial_to_ymd(serial: i64) -> (i64, u32, u32) {
    // civil_from_days with unix epoch shift: 1899-12-30 is -25569 unix days
    let z = serial - 25569 + 719468;
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

pub fn ymd_to_serial(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468 + 25569
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

// DATE(y,m,d) with overflow normalization
pub fn date_serial(y: i64, m: i64, d: i64) -> Option<f64> {
    if y < 0 || y > 32767 {
        return None;
    }
    let mut y = y;
    if y < 100 {
        // LO: two-digit years -> 19xx/20xx? keep simple: add 1900 for < 100? LO uses 1930 pivot
        y += if y < 30 { 2000 } else { 1900 };
    }
    let m0 = m - 1;
    let yy = y + m0.div_euclid(12);
    let mm = (m0.rem_euclid(12) + 1) as u32;
    if yy < 0 {
        return None;
    }
    let s = ymd_to_serial(yy, mm, 1) + d - 1;
    Some(s as f64)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn is_date_format(f: &str) -> bool {
    let lf = f.to_ascii_lowercase();
    let b = lf.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += 1;
                }
            }
            b'y' | b'd' | b'h' | b's' => return true,
            b'm' => return true,
            b'e' => {
                if i + 1 < b.len() && (b[i + 1] == b'+' || b[i + 1] == b'-') {
                    i += 1;
                } else {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn fmt_date(v: f64, f: &str) -> Option<String> {
    let serial = v.floor() as i64;
    let frac = v - v.floor();
    let (y, m, d) = serial_to_ymd(serial);
    let secs = (frac * 86400.0).round() as i64;
    let hh = secs / 3600;
    let mi = (secs / 60) % 60;
    let ss = secs % 60;
    let wd = ((serial + 6).rem_euclid(7)) as usize; // 0=Sunday; serial 0 = Saturday(6)
    let lf: Vec<char> = f.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // detect 'am/pm'
    while i < lf.len() {
        let c = lf[i];
        let lc = c.to_ascii_lowercase();
        if c == '"' {
            i += 1;
            while i < lf.len() && lf[i] != '"' {
                out.push(lf[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '\\' {
            if i + 1 < lf.len() {
                out.push(lf[i + 1]);
            }
            i += 2;
            continue;
        }
        if "ymdhs".contains(lc) {
            let mut n = 1;
            while i + n < lf.len() && lf[i + n].to_ascii_lowercase() == lc {
                n += 1;
            }
            match lc {
                'y' => {
                    if n <= 2 {
                        out.push_str(&format!("{:02}", y.rem_euclid(100)));
                    } else {
                        out.push_str(&format!("{}", y));
                    }
                }
                'm' => {
                    // minutes if preceded by h or followed by s
                    let prev_h = {
                        let mut j = i;
                        let mut found = false;
                        while j > 0 {
                            j -= 1;
                            let pc = lf[j].to_ascii_lowercase();
                            if pc == 'h' {
                                found = true;
                                break;
                            }
                            if "ymds".contains(pc) {
                                break;
                            }
                        }
                        found
                    };
                    if prev_h && n <= 2 {
                        if n == 1 {
                            out.push_str(&format!("{}", mi));
                        } else {
                            out.push_str(&format!("{:02}", mi));
                        }
                    } else {
                        match n {
                            1 => out.push_str(&format!("{}", m)),
                            2 => out.push_str(&format!("{:02}", m)),
                            3 => out.push_str(&MONTHS[(m - 1) as usize][..3]),
                            4 => out.push_str(MONTHS[(m - 1) as usize]),
                            _ => out.push_str(&MONTHS[(m - 1) as usize][..1]),
                        }
                    }
                }
                'd' => match n {
                    1 => out.push_str(&format!("{}", d)),
                    2 => out.push_str(&format!("{:02}", d)),
                    3 => out.push_str(&DAYS[wd][..3]),
                    _ => out.push_str(DAYS[wd]),
                },
                'h' => {
                    if n == 1 {
                        out.push_str(&format!("{}", hh));
                    } else {
                        out.push_str(&format!("{:02}", hh));
                    }
                }
                's' => {
                    if n == 1 {
                        out.push_str(&format!("{}", ss));
                    } else {
                        out.push_str(&format!("{:02}", ss));
                    }
                }
                _ => {}
            }
            i += n;
            continue;
        }
        out.push(c);
        i += 1;
    }
    Some(out)
}

fn group_thousands(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    for (i, &c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c as char);
    }
    out
}

pub fn format_number(v: f64, f: &str) -> Option<String> {
    if f.is_empty() {
        return Some(num_to_string(v));
    }
    // sections separated by ';' : use first for positive, second for negative
    let sections: Vec<&str> = f.split(';').collect();
    let (sec, mut v) = if v < 0.0 && sections.len() > 1 { (sections[1], -v) } else { (sections[0], v) };
    if sec.to_ascii_uppercase() == "GENERAL" {
        return Some(num_to_string(v));
    }
    if is_date_format(sec) {
        return fmt_date(v, sec);
    }
    // numeric format
    let chars: Vec<char> = sec.chars().collect();
    let mut has_percent = false;
    let mut has_comma = false;
    let mut int_digits = 0; // count of 0 in integer part
    let mut dec_digits = 0; // count of 0 after '.'
    let mut dec_hash = 0;
    let mut exp_digits = 0i32;
    let mut in_dec = false;
    let mut in_exp = false;
    let mut exp_plus = false;
    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut seen_digit = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '0' | '#' | '?' => {
                seen_digit = true;
                if in_exp {
                    exp_digits += 1;
                } else if in_dec {
                    if c == '0' {
                        dec_digits += 1;
                    } else {
                        dec_hash += 1;
                    }
                } else if c == '0' {
                    int_digits += 1;
                }
            }
            '.' => in_dec = true,
            ',' => has_comma = true,
            '%' => {
                has_percent = true;
                if seen_digit {
                    suffix.push('%');
                } else {
                    prefix.push('%');
                }
            }
            'E' | 'e' => {
                if i + 1 < chars.len() && (chars[i + 1] == '+' || chars[i + 1] == '-') {
                    in_exp = true;
                    in_dec = false;
                    exp_plus = chars[i + 1] == '+';
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if seen_digit {
                        suffix.push(chars[i]);
                    } else {
                        prefix.push(chars[i]);
                    }
                    i += 1;
                }
            }
            '\\' => {
                i += 1;
                if i < chars.len() {
                    if seen_digit {
                        suffix.push(chars[i]);
                    } else {
                        prefix.push(chars[i]);
                    }
                }
            }
            _ => {
                if seen_digit {
                    suffix.push(c);
                } else {
                    prefix.push(c);
                }
            }
        }
        i += 1;
    }
    if has_percent {
        v *= 100.0;
    }
    let neg = v < 0.0;
    let av = v.abs();
    let mut body = String::new();
    if in_exp || exp_digits > 0 {
        // scientific
        let mut e = if av == 0.0 { 0 } else { av.log10().floor() as i32 };
        let mut mant = if av == 0.0 { 0.0 } else { av / 10f64.powi(e) };
        let mut ms = format!("{:.*}", dec_digits, round_half_away(mant, dec_digits as i32));
        if ms.starts_with("10") {
            e += 1;
            mant /= 10.0;
            ms = format!("{:.*}", dec_digits, round_half_away(mant, dec_digits as i32));
        }
        body.push_str(&ms);
        body.push('E');
        if e < 0 {
            body.push('-');
        } else if exp_plus {
            body.push('+');
        }
        body.push_str(&format!("{:0width$}", e.abs(), width = exp_digits.max(1) as usize));
    } else {
        let r = round_half_away(av, dec_digits as i32 + dec_hash as i32);
        let mut s = format!("{:.*}", dec_digits + dec_hash, r);
        if dec_hash > 0 {
            // strip optional trailing zeros
            if s.contains('.') {
                while s.ends_with('0') && s.len() > 0 {
                    let decs = s.len() - s.find('.').unwrap() - 1;
                    if decs <= dec_digits {
                        break;
                    }
                    s.pop();
                }
                if s.ends_with('.') {
                    s.pop();
                }
            }
        }
        let (ip, dp) = match s.split_once('.') {
            Some((a, b)) => (a.to_string(), Some(b.to_string())),
            None => (s.clone(), None),
        };
        let mut ip = ip;
        if ip == "0" && int_digits == 0 {
            ip = String::new();
        }
        while ip.len() < int_digits {
            ip.insert(0, '0');
        }
        if has_comma {
            ip = group_thousands(&ip);
        }
        body.push_str(&ip);
        if let Some(dp) = dp {
            body.push('.');
            body.push_str(&dp);
        }
        if neg && body.chars().any(|c| c.is_ascii_digit() && c != '0') {
            body.insert(0, '-');
        } else if neg && body.is_empty() {
        }
    }
    if neg && (in_exp || exp_digits > 0) {
        body.insert(0, '-');
    }
    Some(format!("{}{}{}", prefix, body, suffix))
}
