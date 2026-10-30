import sys, json, re, math, zipfile, threading, time
from itertools import compress
import xml.etree.ElementTree as ET
from datetime import date, timedelta
from decimal import Decimal, ROUND_HALF_UP, ROUND_DOWN, ROUND_UP

M = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
R_ID = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"


class Err:
    __slots__ = ('c',)

    def __init__(self, c):
        self.c = c

    def __repr__(self):
        return self.c


E_DIV = Err('#DIV/0!')
E_VAL = Err('#VALUE!')
E_NA = Err('#N/A')
E_NUM = Err('#NUM!')
E_REF = Err('#REF!')
E_NAME = Err('#NAME?')
E_NULL = Err('#NULL!')
E_CIRC = Err('#VALUE!')
ERRMAP = {e.c: e for e in (E_DIV, E_VAL, E_NA, E_NUM, E_REF, E_NAME, E_NULL)}


class Rng:
    __slots__ = ('sh', 'r1', 'c1', 'r2', 'c2')

    def __init__(self, sh, r1, c1, r2, c2):
        self.sh = sh
        self.r1 = r1
        self.c1 = c1
        self.r2 = r2
        self.c2 = c2


class FC:
    __slots__ = ('fn', 'st', 'val', 'bl')

    def __init__(self, fn):
        self.fn = fn
        self.st = 0
        self.val = None
        self.bl = False


BOOLS = {}


def isbool(sh, r, c):
    x = CELLS[sh].get((r, c))
    if x.__class__ is FC:
        return x.bl
    return (r, c) in BOOLS.get(sh, ())


CELLS = {}
EPOCH = date(1899, 12, 30)


def cellval(sh, r, c):
    d = CELLS[sh]
    x = d.get((r, c))
    if x.__class__ is FC:
        if x.st == 2:
            return x.val
        if x.st == 1:
            return E_CIRC
        x.st = 1
        try:
            raw = x.fn()
            if raw.__class__ is Rng:
                v = cellval(raw.sh, raw.r1, raw.c1)
                x.bl = isbool(raw.sh, raw.r1, raw.c1)
            else:
                v = raw
                x.bl = raw.__class__ is bool
        except RecursionError:
            raise
        except Exception:
            v = E_VAL
            x.bl = False
        if v.__class__ is bool:
            v = 1.0 if v else 0.0
        if v is None:
            v = 0.0
        x.val = v
        x.st = 2
        return v
    return x


def deref(v):
    c = v.__class__
    if c is Rng:
        v = cellval(v.sh, v.r1, v.c1)
        c = v.__class__
    if c is bool:
        return 1.0 if v else 0.0
    return v


RCACHE = {}
IDXCACHE = {}
MCACHE = {}


def rng_values(r):
    key = (r.sh, r.r1, r.c1, r.r2, r.c2)
    got = RCACHE.get(key)
    if got is not None:
        return got
    d = CELLS[r.sh]
    out = []
    for rr in range(r.r1, r.r2 + 1):
        for cc in range(r.c1, r.c2 + 1):
            x = d.get((rr, cc))
            if x.__class__ is FC:
                x = cellval(r.sh, rr, cc)
            elif x.__class__ is bool:
                x = 1.0 if x else 0.0
            out.append(x)
    RCACHE[key] = out
    return out


def text_values(r):
    key = ('T', r.sh, r.r1, r.c1, r.r2, r.c2)
    got = RCACHE.get(key)
    if got is not None:
        return got
    d = CELLS[r.sh]
    bs = BOOLS.get(r.sh, ())
    out = []
    for rr in range(r.r1, r.r2 + 1):
        for cc in range(r.c1, r.c2 + 1):
            x = d.get((rr, cc))
            if x.__class__ is FC:
                v = cellval(r.sh, rr, cc)
                if x.bl:
                    v = 'TRUE' if v else 'FALSE'
            else:
                v = x
                if (rr, cc) in bs:
                    v = 'TRUE' if v else 'FALSE'
            out.append(v)
    RCACHE[key] = out
    return out


def rng_mask(r, crit):
    key = (r.sh, r.r1, r.c1, r.r2, r.c2, crit.__class__.__name__, crit if crit.__class__ is not float else ('f', crit))
    got = MCACHE.get(key)
    if got is not None:
        return got
    pred = make_matcher(crit)
    if crit.__class__ is str and parse_num(crit) is None and crit.upper() not in ('TRUE', 'FALSE') and not crit.startswith(('<', '>')):
        vals = text_values(r)
    else:
        vals = rng_values(r)
    spec = getattr(pred, 'spec', None)
    if spec is not None:
        op, lo_b, hi_b = spec
        if op == '>':
            m = [v.__class__ is float and v > hi_b for v in vals]
        elif op == '>=':
            m = [v.__class__ is float and v >= lo_b for v in vals]
        elif op == '<':
            m = [v.__class__ is float and v < lo_b for v in vals]
        else:
            m = [v.__class__ is float and v <= hi_b for v in vals]
    else:
        m = [pred(v) for v in vals]
    m = bytes(m)
    MCACHE[key] = m
    return m


def numinfo(r):
    key = ('N', r.sh, r.r1, r.c1, r.r2, r.c2)
    got = RCACHE.get(key)
    if got is not None:
        return got
    vals = rng_values(r)
    nums = [v if v.__class__ is float else 0.0 for v in vals]
    isn = bytes(1 if v.__class__ is float else 0 for v in vals)
    haserr = any(v.__class__ is Err for v in vals)
    got = (nums, isn, haserr)
    RCACHE[key] = got
    return got


def _mand(a, b):
    n = len(a)
    return (int.from_bytes(a, 'little') & int.from_bytes(b, 'little')).to_bytes(n, 'little')


def rng_cells(r):
    # yields (value) for each cell in row-major order
    return rng_values(r)


def tonum(v):
    c = v.__class__
    if c is float:
        return v
    if v is None:
        return 0.0
    if c is bool:
        return 1.0 if v else 0.0
    if c is str:
        p = parse_num(v)
        return E_VAL if p is None else p
    if c is Err:
        return v
    if c is int:
        return float(v)
    return E_VAL


MON3 = {m: i + 1 for i, m in enumerate(['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'])}


def parse_datestr(t):
    m = re.fullmatch(r'(\d{1,2})-([A-Za-z]{3})-(\d{2}|\d{4})', t)
    if m:
        d, mo, y = int(m.group(1)), MON3.get(m.group(2).upper()), int(m.group(3))
        if mo is None:
            return None
        if y < 100:
            y += 2000 if y < 30 else 1900
    else:
        m = re.fullmatch(r'(\d{4})-(\d{1,2})-(\d{1,2})', t)
        if m:
            y, mo, d = int(m.group(1)), int(m.group(2)), int(m.group(3))
        else:
            m = re.fullmatch(r'(\d{1,2})/(\d{1,2})/(\d{4}|\d{2})', t)
            if not m:
                return None
            mo, d, y = int(m.group(1)), int(m.group(2)), int(m.group(3))
            if y < 100:
                y += 2000 if y < 30 else 1900
    try:
        return float((date(y, mo, d) - EPOCH).days)
    except ValueError:
        return None


def parse_num(s):
    t = s.strip()
    if not t:
        return None
    u = t.upper()
    if u.startswith('$') and len(t) > 1 and (t[1].isdigit() or t[1] in '.-'):
        t = t[1:].strip()
        u = t.upper()
    if u == 'TRUE':
        return 1.0
    if u == 'FALSE':
        return 0.0
    if t[0].isdigit() and ('-' in t or '/' in t):
        dv = parse_datestr(t)
        if dv is not None:
            return dv
    pct = False
    if t.endswith('%'):
        pct = True
        t = t[:-1].strip()
    if not re.fullmatch(r'[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?', t):
        if re.fullmatch(r'[+-]?\d{1,3}(,\d{3})+(\.\d*)?', t):
            t = t.replace(',', '')
        else:
            return None
    f = float(t)
    if pct:
        f /= 100.0
    return f


def tostr(v):
    c = v.__class__
    if c is str:
        return v
    if v is None:
        return ""
    if c is bool:
        return "1" if v else "0"
    if c is float:
        return num2str(v)
    if c is Err:
        return v
    if c is int:
        return str(v)
    return ""


def num2str(x):
    if x == int(x) and abs(x) < 1e15:
        return str(int(x))
    ax = abs(x)
    if 1e-15 <= ax < 1e15:
        ex = int(math.floor(math.log10(ax)))
        dec = max(0, 14 - ex)
        t = ('%.*f' % (dec, x))
        if '.' in t:
            t = t.rstrip('0').rstrip('.')
        return t
    s = '%.15g' % x
    if 'e' in s:
        m, e = s.split('e')
        ev = int(e)
        return m + 'E' + ('+' if ev >= 0 else '-') + ('%03d' % abs(ev))
    return s


def tobool(v):
    c = v.__class__
    if c is bool:
        return v
    if c is float:
        return v != 0
    if c is int:
        return v != 0
    if v is None:
        return False
    if c is str:
        u = v.upper()
        if u == 'TRUE':
            return True
        if u == 'FALSE':
            return False
        p = parse_num(v)
        if p is None:
            return E_VAL
        return p != 0
    return v


def approx_eq(a, b):
    if a == b:
        return True
    return abs(a - b) <= 3.6e-15 * max(abs(a), abs(b))


def type_rank(v):
    c = v.__class__
    if c is float or c is int:
        return 0
    if c is str:
        return 1
    return 2


def cmp_vals(a, b):
    # returns -1,0,1 or Err
    if a.__class__ is Err:
        return a
    if b.__class__ is Err:
        return b
    if a is None:
        a = 0.0 if b.__class__ is float else ('' if b.__class__ is str else False)
    if b is None:
        b = 0.0 if a.__class__ is float else ('' if a.__class__ is str else False)
    ra, rb = type_rank(a), type_rank(b)
    if ra != rb:
        return -1 if ra < rb else 1
    if ra == 0:
        if approx_eq(a, b):
            return 0
        return -1 if a < b else 1
    if ra == 1:
        x, y = a.lower(), b.lower()
        return 0 if x == y else (-1 if x < y else 1)
    x, y = int(a), int(b)
    return 0 if x == y else (-1 if x < y else 1)


def eq_key(v):
    if v.__class__ is str:
        return v.lower()
    return v


# ---------- criteria ----------
def wild_re(p):
    out = []
    i = 0
    while i < len(p):
        ch = p[i]
        if ch == '~' and i + 1 < len(p) and p[i + 1] in '*?~':
            out.append(re.escape(p[i + 1]))
            i += 2
            continue
        if ch == '*':
            out.append('.*')
        elif ch == '?':
            out.append('.')
        else:
            out.append(re.escape(ch))
        i += 1
    return re.compile('^' + ''.join(out) + '$', re.I | re.S)


_wcache = {}


def get_wild(p):
    r = _wcache.get(p)
    if r is None:
        r = wild_re(p)
        _wcache[p] = r
    return r


def make_matcher(crit):
    """returns predicate on cell value"""
    if crit.__class__ is Err:
        return lambda v: False
    if crit.__class__ is float or crit.__class__ is int:
        x = float(crit)
        return lambda v: v.__class__ is float and approx_eq(v, x)
    if crit.__class__ is bool:
        crit = 1.0 if crit else 0.0
        return lambda v: v.__class__ is float and approx_eq(v, crit)
    if crit is None:
        return lambda v: False
    s = crit
    op = '='
    for o in ('<=', '>=', '<>', '<', '>', '='):
        if s.startswith(o):
            op = o
            s = s[len(o):]
            break
    num = parse_num(s) if s.strip() != '' else None
    if num is not None and not (s.strip().upper() in ('TRUE', 'FALSE')):
        x = num
        if op == '=':
            return lambda v: (v.__class__ is float and approx_eq(v, x)) or (v.__class__ is str and parse_num(v) is not None and approx_eq(parse_num(v), x))
        if op == '<>':
            return lambda v: not ((v.__class__ is float and approx_eq(v, x)) or (v.__class__ is str and parse_num(v) is not None and approx_eq(parse_num(v), x)))
        tol = 3.6e-15 * abs(x)
        lo_b, hi_b = x - tol, x + tol

        def f(v, op=op, x=x):
            if v.__class__ is not float:
                return False
            c = 0 if approx_eq(v, x) else (-1 if v < x else 1)
            return {'<': c < 0, '>': c > 0, '<=': c <= 0, '>=': c >= 0}[op]
        f.spec = (op, lo_b, hi_b)
        return f
    if s.upper() in ('TRUE', 'FALSE') and op in ('=', '<>'):
        b = 1.0 if s.upper() == 'TRUE' else 0.0
        if op == '=':
            return lambda v: v.__class__ is float and v == b
        return lambda v: not (v.__class__ is float and v == b)
    if s == '':
        if op == '=':
            return lambda v: v is None or v == ''
        if op == '<>':
            return lambda v: not (v is None or v == '')
    if op == '=':
        rx = get_wild(s)
        return lambda v: v.__class__ is str and rx.match(v) is not None
    if op == '<>':
        rx = get_wild(s)
        return lambda v: not (v.__class__ is str and rx.match(v) is not None)
    sl = s.lower()
    def g(v, op=op, sl=sl):
        if v.__class__ is not str:
            return False
        vl = v.lower()
        return {'<': vl < sl, '>': vl > sl, '<=': vl <= sl, '>=': vl >= sl}[op]
    return g


def flat_vals(a):
    if a.__class__ is Rng:
        return rng_values(a)
    return [a]


def sum_nums(args):
    acc = []
    args = [a() for a in args]
    for a in args:
        if a.__class__ is Rng:
            for v in rng_values(a):
                if v.__class__ is float:
                    acc.append(v)
                elif v.__class__ is Err:
                    return v
        else:
            if a.__class__ is str:
                return E_VAL
            v = tonum(a)
            if v.__class__ is Err:
                return v
            acc.append(v)
    return ksum(acc)


def collect_nums(args):
    out = []
    args = [a() for a in args] if (args and callable(args[0])) else args
    for a in args:
        if a.__class__ is Rng:
            for v in rng_values(a):
                if v.__class__ is float:
                    out.append(v)
                elif v.__class__ is Err:
                    return v
        else:
            if a.__class__ is Err:
                return a
            if a is None:
                continue
            if a.__class__ is str:
                return E_VAL
            v = tonum(a)
            if v.__class__ is Err:
                return v
            out.append(v)
    return out


# ---------- rounding ----------
import decimal as _dec
_CTX = _dec.Context(prec=400)


def dround(x, n, mode):
    n = int(n)
    if abs(x) >= 1e15 and n >= 0:
        return x
    try:
        d = _dec.Decimal('%.15g' % float(x))
        q = _dec.Decimal(1).scaleb(-n)
        r = d.quantize(q, rounding=mode, context=_CTX)
    except Exception:
        return x
    return float(r)


def to_date(v):
    return EPOCH + timedelta(days=int(math.floor(v)))


def serial_of(d):
    return float((d - EPOCH).days)


def days_in_month(y, m):
    if m == 12:
        return 31
    return (date(y, m + 1, 1) - date(y, m, 1)).days


def add_months(d, n):
    m0 = d.month - 1 + n
    y = d.year + m0 // 12
    m = m0 % 12 + 1
    day = min(d.day, days_in_month(y, m))
    return date(y, m, day)


MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December']
DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
TEXT_RE = re.compile(r'"([^"]*)"|(yyyy|yy|mmmm|mmm|mm|m|dddd|ddd|dd|d|hh|h|ss|s|AM/PM|am/pm|A/P|a/p)|(.)', re.I | re.S)


def fmt_date(v, fmt):
    if v is None:
        v = 0.0
    serial = float(v)
    days = int(math.floor(serial))
    frac = serial - days
    secs = int(round(frac * 86400)) % 86400
    d = EPOCH + timedelta(days=days)
    hh, mi, ss = secs // 3600, (secs // 60) % 60, secs % 60
    # tokenize
    toks = []
    for m in TEXT_RE.finditer(fmt):
        if m.group(1) is not None:
            toks.append(('lit', m.group(1)))
        elif m.group(2) is not None:
            toks.append(('tok', m.group(2).lower()))
        else:
            toks.append(('lit', m.group(3)))
    # minute vs month: 'm' / 'mm' immediately after h or before s is minute
    has_ampm = any(t == ('tok', 'am/pm') or t == ('tok', 'a/p') for t in toks)
    out = []
    for i, (k, t) in enumerate(toks):
        if k == 'lit':
            out.append(t)
            continue
        if t in ('m', 'mm', 'mmm', 'mmmm'):
            prev = None
            for kk, tt in reversed(toks[:i]):
                if kk == 'tok':
                    prev = tt
                    break
            nxt = None
            for kk, tt in toks[i + 1:]:
                if kk == 'tok':
                    nxt = tt
                    break
            if prev in ('h', 'hh') or nxt in ('s', 'ss'):
                out.append(('%02d' % mi) if t == 'mm' else str(mi))
                continue
            if t == 'mm':
                out.append('%02d' % d.month)
            elif t == 'm':
                out.append(str(d.month))
            else:
                out.append(MONTHS[d.month - 1] if t == 'mmmm' else MONTHS[d.month - 1][:3])
        elif t == 'yyyy':
            out.append('%04d' % d.year)
        elif t == 'yy':
            out.append('%02d' % (d.year % 100))
        elif t == 'dddd':
            out.append(DAYS[d.weekday()])
        elif t == 'ddd':
            out.append(DAYS[d.weekday()][:3])
        elif t == 'dd':
            out.append('%02d' % d.day)
        elif t == 'd':
            out.append(str(d.day))
        elif t in ('hh', 'h'):
            h = hh
            if has_ampm:
                h = hh % 12
                if h == 0:
                    h = 12
            out.append(('%02d' % h) if t == 'hh' else str(h))
        elif t == 'ss':
            out.append('%02d' % ss)
        elif t == 's':
            out.append(str(ss))
        elif t in ('am/pm',):
            out.append('AM' if hh < 12 else 'PM')
        elif t in ('a/p',):
            out.append('A' if hh < 12 else 'P')
    return ''.join(out)


def fmt_num(x, fmt):
    sections = fmt.split(';')
    f = sections[0]
    if x < 0 and len(sections) > 1:
        f = sections[1]
        x = -x
    neg = x < 0
    if neg and round(abs(x), 15) == 0:
        neg = False
        x = 0.0
    # collect literal pieces
    lit = []
    core_chars = []
    pct = False
    i = 0
    s = f
    buf_core = None
    parts = []  # list of ('lit', str) or ('core', str)
    while i < len(s):
        ch = s[i]
        if ch == '"':
            j = s.find('"', i + 1)
            if j < 0:
                j = len(s)
            parts.append(('lit', s[i + 1:j]))
            i = j + 1
            continue
        if ch == '\\' and i + 1 < len(s):
            parts.append(('lit', s[i + 1]))
            i += 2
            continue
        if ch == '%':
            pct = True
            parts.append(('lit', '%'))
            i += 1
            continue
        if ch in '0#?,.' or (ch == 'E' and parts and parts[-1][0] == 'core') or (ch in '+-' and parts and parts[-1][0] == 'core' and parts[-1][1].endswith('E')):
            if parts and parts[-1][0] == 'core':
                parts[-1] = ('core', parts[-1][1] + ch)
            else:
                parts.append(('core', ch))
            i += 1
            continue
        if ch == 'E' or ch == 'e':
            parts.append(('core', ch))
            i += 1
            continue
        parts.append(('lit', ch))
        i += 1
    cores = [p for p in parts if p[0] == 'core']
    if not cores:
        return fmt.replace('"', '') if False else ''.join(p[1] for p in parts)
    core = cores[0][1]
    if pct:
        x = x * 100
    if 'E' in core.upper():
        ei = core.upper().index('E')
        mant, ex = core[:ei], core[ei + 1:]
        dec = len(mant.split('.')[1]) if '.' in mant else 0
        digits_ex = len(ex.lstrip('+-'))
        ax = abs(x)
        if ax == 0:
            e = 0
        else:
            e = int(math.floor(math.log10(ax)))
        mv = ax / (10 ** e) if e != 0 else ax
        ms = dround(mv, dec, ROUND_HALF_UP)
        if ms >= 10:
            e += 1
            mv = ax / (10 ** e)
            ms = dround(mv, dec, ROUND_HALF_UP)
        ms_s = ('%.' + str(dec) + 'f') % ms
        es = ('-' if e < 0 else ('+' if '+' in ex else ''))
        es += ('%0' + str(max(digits_ex, 1)) + 'd') % abs(e)
        body = ms_s + 'E' + es
    else:
        if '.' in core:
            ip, fp = core.split('.', 1)
            dec = len(fp.replace(',', ''))
        else:
            ip, fp = core, ''
            dec = 0
        minint = ip.count('0')
        thousands = ',' in ip
        r = dround(abs(x), dec, ROUND_HALF_UP)
        s2 = ('%.' + str(dec) + 'f') % r
        if '.' in s2:
            si, sf = s2.split('.')
        else:
            si, sf = s2, ''
        if si == '0' and minint == 0:
            si = ''
        if len(si) < minint:
            si = '0' * (minint - len(si)) + si
        if thousands and si:
            g = []
            while len(si) > 3:
                g.insert(0, si[-3:])
                si = si[:-3]
            g.insert(0, si)
            si = ','.join(g)
        # decimal part: keep trailing zeros for '0', strip for '#'
        if dec > 0:
            fs = fp.replace(',', '')
            keep = 0
            for k, cch in enumerate(fs):
                if cch == '0':
                    keep = k + 1
            sf2 = sf[:keep] + sf[keep:].rstrip('0') if False else sf
            # trim trailing '#' digits that are zero
            sf2 = sf[:keep]
            rest = sf[keep:]
            # include non-zero trailing digits
            rest_t = rest.rstrip('0')
            sf2 = sf2 + rest_t if len(rest_t) > 0 else sf2
            body = si + ('.' + sf2 if sf2 else '')
            if fs and sf2 == '' and fs.count('0') == 0:
                body = si
        else:
            body = si
        pass
    # rebuild with prefix/suffix
    out = []
    placed = False
    for k, p in parts:
        if k == 'core':
            if not placed:
                out.append(body)
                placed = True
        else:
            out.append(p)
    res = ''.join(out)
    if neg and body.strip('0.,'):
        res = '-' + res
    return res


def text_fmt(v, fmt):
    v = deref(v)
    if v.__class__ is Err:
        return v
    if fmt.strip().lower() == 'general':
        if v.__class__ is str:
            return v
        if v is None:
            return '0'
        return num2str(float(v)) if v.__class__ is float else tostr(v)
    if v.__class__ is str:
        # text section if present
        parts = fmt.split(';')
        if len(parts) >= 4:
            return parts[3].replace('@', v)
        if '@' in fmt:
            return fmt.replace('@', v)
        p = parse_num(v)
        if p is None:
            return v
        v = p
    if v.__class__ is bool:
        return 'TRUE' if v else 'FALSE'
    x = tonum(v)
    if x.__class__ is Err:
        return x
    if v is None:
        x = 0.0
    if re.search(r'(?<!")[yYdDhHsS]|[mM]', re.sub(r'"[^"]*"', '', fmt)) and re.search(r'[yYdDhHsSmM]', re.sub(r'"[^"]*"', '', fmt).replace('0', '').replace('#', '').replace('?', '')):
        return fmt_date(x, fmt)
    return fmt_num(x, fmt)


def excel_date_parts(serial):
    return to_date(serial)


# ---------- tokenizer / parser ----------
TOK = re.compile(r'''\s*(?:
 (?P<str>"(?:[^"]|"")*")
|(?P<err>\#(?:DIV/0!|VALUE!|N/A|NUM!|REF!|NAME\?|NULL!))
|(?P<func>[A-Za-z_][A-Za-z0-9_.]*(?=\())
|(?P<sref>(?:'(?:[^']|'')+'|[A-Za-z_][A-Za-z0-9_.]*)!\$?[A-Za-z]{1,3}\$?\d+)
|(?P<ref>\$?[A-Za-z]{1,3}\$?\d+(?![A-Za-z0-9_(!]))
|(?P<num>(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)
|(?P<name>[A-Za-z_][A-Za-z0-9_.]*)
|(?P<op><>|<=|>=|[-+*/^&=<>%(),:;{}])
)''', re.X)


def col2n(s):
    n = 0
    for ch in s.upper():
        n = n * 26 + ord(ch) - 64
    return n


def parse_ref(t):
    t = t.replace('$', '')
    m = re.fullmatch(r'([A-Za-z]+)(\d+)', t)
    return int(m.group(2)), col2n(m.group(1))


TOKF = re.compile(TOK.pattern, re.X)


def tokenize(s):
    toks = []
    app = toks.append
    pos = 0
    for m in TOKF.finditer(s):
        if m.start() != pos and s[pos:m.start()].strip():
            raise SyntaxError('bad token in %r' % s)
        pos = m.end()
        k = m.lastgroup
        if k is None:
            continue
        app((k, m.group(k)))
    if s[pos:].strip():
        raise SyntaxError('bad tail in %r' % s)
    return toks


PREC = {'=': 1, '<>': 1, '<': 1, '>': 1, '<=': 1, '>=': 1, '&': 2, '+': 3, '-': 3, '*': 4, '/': 4, '^': 5}


class Parser:
    def __init__(self, toks, cursheet):
        self.t = toks
        self.i = 0
        self.sh = cursheet

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else (None, None)

    def next(self):
        x = self.peek()
        self.i += 1
        return x

    def expect(self, v):
        k, t = self.next()
        if t != v:
            raise SyntaxError('expected %s got %s' % (v, t))

    def parse(self):
        e = self.cmp()
        if self.i != len(self.t):
            raise SyntaxError('trailing tokens')
        return e

    def cmp(self):
        return self.binop(1)

    def binop(self, minp):
        left = self.unary()
        t_ = self.t
        while True:
            if self.i >= len(t_):
                return left
            k, t = t_[self.i]
            if k != 'op':
                return left
            p = PREC.get(t)
            if p is None or p < minp:
                return left
            self.i += 1
            right = self.binop(p + 1)
            if p == 1:
                left = mk_cmp(t, left, right)
            elif p == 2:
                left = mk_concat(left, right)
            else:
                left = mk_arith(t, left, right)

    def unary(self):
        k, t = self.peek()
        if k == 'op' and t == '-':
            self.next()
            e = self.unary()
            return lambda: _neg(e())
        if k == 'op' and t == '+':
            self.next()
            return self.unary()
        return self.postfix()

    def postfix(self):
        e = self.primary()
        while True:
            k, t = self.peek()
            if k == 'op' and t == '%':
                self.next()
                e = mk_arith('%', e, None)
            else:
                return e

    def primary(self):
        k, t = self.next()
        if k == 'num':
            v = float(t)
            return lambda: v
        if k == 'str':
            v = t[1:-1].replace('""', '"')
            return lambda: v
        if k == 'err':
            v = ERRMAP[t]
            return lambda: v
        if k == 'name':
            u = t.upper()
            if u == 'TRUE':
                return lambda: True
            if u == 'FALSE':
                return lambda: False
            raise SyntaxError('unknown name ' + t)
        if k in ('ref', 'sref'):
            return self.refnode(k, t)
        if k == 'func':
            name = t.upper()
            if name.startswith('_XLFN.'):
                name = name[6:]
            self.expect('(')
            args = []
            k2, t2 = self.peek()
            if k2 == 'op' and t2 == ')':
                self.next()
            else:
                while True:
                    k2, t2 = self.peek()
                    if k2 == 'op' and t2 in (',', ')'):
                        args.append(lambda: 0.0)
                    else:
                        args.append(self.cmp())
                    k3, t3 = self.next()
                    if t3 == ')':
                        break
                    if t3 != ',':
                        raise SyntaxError('bad arg sep')
            f = FUNCS.get(name)
            if f is None:
                raise SyntaxError('unknown function ' + name)
            return lambda: f(args)
        if k == 'op' and t == '(':
            e = self.cmp()
            self.expect(')')
            return e
        raise SyntaxError('unexpected %s' % t)

    def refnode(self, k, t):
        sh = self.sh
        if k == 'sref':
            idx = t.rindex('!')
            sh = t[:idx]
            if sh.startswith("'"):
                sh = sh[1:-1].replace("''", "'")
            t = t[idx + 1:]
        r1, c1 = parse_ref(t)
        if self.peek() == ('op', ':'):
            save = self.i
            self.next()
            k2, t2 = self.next()
            if k2 == 'ref':
                r2, c2 = parse_ref(t2)
                r = Rng(sh, min(r1, r2), min(c1, c2), max(r1, r2), max(c1, c2))
                return lambda: r
            self.i = save
        r = Rng(sh, r1, c1, r1, c1)
        return lambda: r


def _neg(v):
    v = deref(v)
    x = tonum(v)
    if x.__class__ is Err:
        return x
    return -x


def mk_arith(op, a, b):
    if op == '%':
        def f():
            x = tonum(deref(a()))
            if x.__class__ is Err:
                return x
            return x / 100.0
        return f
    if op == '+':
        def f():
            xr = a()
            yr = b()
            x = deref(xr)
            y = deref(yr)
            if x.__class__ is Err:
                return y if (y.__class__ is Err and xr.__class__ is Rng) else x
            if y.__class__ is Err:
                return y
            x = tonum(x)
            if x.__class__ is Err:
                return x
            y = tonum(y)
            if y.__class__ is Err:
                return y
            return x + y
        return f
    if op == '-':
        def f():
            xr = a()
            yr = b()
            x = deref(xr)
            y = deref(yr)
            if x.__class__ is Err:
                return y if (y.__class__ is Err and xr.__class__ is Rng) else x
            if y.__class__ is Err:
                return y
            x = tonum(x)
            if x.__class__ is Err:
                return x
            y = tonum(y)
            if y.__class__ is Err:
                return y
            return x - y
        return f
    if op == '*':
        def f():
            xr = a()
            yr = b()
            x = deref(xr)
            y = deref(yr)
            if x.__class__ is Err:
                return y if (y.__class__ is Err and xr.__class__ is Rng) else x
            if y.__class__ is Err:
                return y
            x = tonum(x)
            if x.__class__ is Err:
                return x
            y = tonum(y)
            if y.__class__ is Err:
                return y
            r = x * y
            if r == _INF or r == -_INF:
                return E_NUM
            return r
        return f
    if op == '/':
        def f():
            xr = a()
            yr = b()
            x = deref(xr)
            y = deref(yr)
            if x.__class__ is Err:
                return y if (y.__class__ is Err and xr.__class__ is Rng) else x
            if y.__class__ is Err:
                return y
            x = tonum(x)
            if x.__class__ is Err:
                return x
            y = tonum(y)
            if y.__class__ is Err:
                return y
            if y == 0:
                return E_DIV
            return x / y
        return f
    if op == '^':
        def f():
            xr = a()
            yr = b()
            x = deref(xr)
            y = deref(yr)
            if x.__class__ is Err:
                return y if (y.__class__ is Err and xr.__class__ is Rng) else x
            if y.__class__ is Err:
                return y
            x = tonum(x)
            if x.__class__ is Err:
                return x
            y = tonum(y)
            if y.__class__ is Err:
                return y
            return fpow(x, y)
        return f
    raise SyntaxError(op)


def fpow(x, y):
    if x == 0 and y < 0:
        return E_NUM
    if x < 0 and y != int(y):
        try:
            inv = 1.0 / y
            if abs(inv - round(inv)) < 1e-9 and int(round(inv)) % 2 == 1:
                return -((-x) ** y)
        except Exception:
            pass
        return E_NUM
    try:
        r = x ** y
    except OverflowError:
        return E_NUM
    except ZeroDivisionError:
        return E_DIV
    if isinstance(r, complex):
        return E_NUM
    return float(r)


def mk_concat(a, b):
    def f():
        x = deref(a())
        y = deref(b())
        if x.__class__ is Err:
            return x
        if y.__class__ is Err:
            return y
        return tostr(x) + tostr(y)
    return f


def mk_cmp(op, a, b):
    def f():
        xr = a()
        yr = b()
        x = deref(xr)
        y = deref(yr)
        if x.__class__ is Err:
            return y if (y.__class__ is Err and xr.__class__ is Rng) else x
        if y.__class__ is Err:
            return y
        c = cmp_vals(x, y)
        if c.__class__ is Err:
            return c
        if op == '=':
            return c == 0
        if op == '<>':
            return c != 0
        if op == '<':
            return c < 0
        if op == '>':
            return c > 0
        if op == '<=':
            return c <= 0
        return c >= 0
    return f


FUNCS = {}


def fn(*names):
    def deco(f):
        for n in names:
            FUNCS[n] = f
        return f
    return deco


def scal(args, i):
    """deref arg i (or None if missing)"""
    if i >= len(args):
        return None
    return deref(args[i]())


def ev(args, i):
    if i >= len(args):
        return None
    return args[i]()


_INF = float('inf')


def first_err(vals):
    for v in vals:
        if v.__class__ is Err:
            return v
    return None


# ---------- logical ----------
@fn('IF')
def f_if(args):
    c = scal(args, 0)
    if c.__class__ is Err:
        return c
    b = tobool(c)
    if b.__class__ is Err:
        return b
    if b:
        return ev(args, 1) if len(args) > 1 else True
    if len(args) > 2:
        v = args[2]()
        return False if v is None else v
    return False


@fn('IFERROR')
def f_iferror(args):
    v = args[0]()
    if deref(v).__class__ is Err:
        return ev(args, 1)
    return v


@fn('IFNA')
def f_ifna(args):
    v = args[0]()
    d = deref(v)
    if d.__class__ is Err and d.c == '#N/A':
        return ev(args, 1)
    return v


@fn('IFS')
def f_ifs(args):
    for i in range(0, len(args), 2):
        c = scal(args, i)
        if c.__class__ is Err:
            return c
        b = tobool(c)
        if b.__class__ is Err:
            return b
        if b:
            return ev(args, i + 1)
    return E_NA


@fn('SWITCH')
def f_switch(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    n = len(args)
    i = 1
    while i + 1 < n:
        cand = scal(args, i)
        c = cmp_vals(v, cand)
        if c.__class__ is Err:
            return c
        if c == 0:
            return ev(args, i + 1)
        i += 2
    if i < n:
        return ev(args, i)
    return E_NA


@fn('CHOOSE')
def f_choose(args):
    x = scal(args, 0)
    if x.__class__ is Err:
        return x
    x = tonum(x)
    if x.__class__ is Err:
        return x
    k = int(x)
    if k < 1 or k >= len(args):
        return E_VAL
    return ev(args, k)


@fn('AND', 'OR', 'XOR')
def f_andor(args):
    raise NotImplementedError


def _logic(args, mode):
    vals = []
    for a in args:
        v = a()
        if v.__class__ is Rng:
            for x in rng_values(v):
                if x.__class__ is Err:
                    return x
                if x.__class__ is bool:
                    vals.append(x)
                elif x.__class__ is float:
                    vals.append(x != 0)
        else:
            v = deref(v)
            if v.__class__ is Err:
                return v
            if v is None:
                continue
            b = tobool(v)
            if b.__class__ is Err:
                return b
            vals.append(b)
    if not vals:
        return E_VAL
    if mode == 'and':
        return all(vals)
    if mode == 'or':
        return any(vals)
    return sum(1 for x in vals if x) % 2 == 1


FUNCS['AND'] = lambda args: _logic(args, 'and')
FUNCS['OR'] = lambda args: _logic(args, 'or')
FUNCS['XOR'] = lambda args: _logic(args, 'xor')


@fn('NOT')
def f_not(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    b = tobool(v)
    if b.__class__ is Err:
        return b
    return not b


@fn('TRUE')
def f_true(args):
    return True


@fn('FALSE')
def f_false(args):
    return False


# ---------- info ----------
def _ref_or_val(args, i):
    if i >= len(args):
        return None
    return args[i]()


@fn('ISBLANK')
def f_isblank(args):
    v = _ref_or_val(args, 0)
    if v.__class__ is Rng:
        v = cellval(v.sh, v.r1, v.c1) if (v.r1 == v.r2 and v.c1 == v.c2) else 0.0
        return v is None
    return False


@fn('ISERROR')
def f_iserror(args):
    return deref(_ref_or_val(args, 0)).__class__ is Err


@fn('ISNA')
def f_isna(args):
    v = deref(_ref_or_val(args, 0))
    return v.__class__ is Err and v.c == '#N/A'


@fn('ISNUMBER')
def f_isnumber(args):
    return deref(_ref_or_val(args, 0)).__class__ in (float, bool)


@fn('ISTEXT')
def f_istext(args):
    return deref(_ref_or_val(args, 0)).__class__ is str


@fn('ISLOGICAL')
def f_islogical(args):
    v = _ref_or_val(args, 0)
    if v.__class__ is Rng:
        cellval(v.sh, v.r1, v.c1)
        return isbool(v.sh, v.r1, v.c1)
    return v.__class__ is bool


@fn('N')
def f_n(args):
    v = deref(_ref_or_val(args, 0))
    c = v.__class__
    if c is float:
        return v
    if c is float or c is bool:
        return deref(v)
    if c is Err:
        return v
    return 0.0


@fn('T')
def f_t(args):
    v = deref(_ref_or_val(args, 0))
    if v.__class__ is Err:
        return v
    return v if v.__class__ is str else ''


# ---------- math ----------
@fn('SUM')
def f_sum(args):
    return sum_nums(args)


@fn('PRODUCT')
def f_product(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    if not vals:
        return 0.0
    p = 1.0
    for v in vals:
        p *= v
    if p == _INF or p == -_INF:
        return E_NUM
    return p


@fn('SUMSQ')
def f_sumsq(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    return sum(v * v for v in vals)


@fn('AVERAGE')
def f_average(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    if not vals:
        return E_DIV
    return ksum(vals) / len(vals)


def _minmax(args, want_max):
    best = None
    best_b = False
    cnt = 0
    first_b = False
    first_seen = False
    for ai, a in enumerate(args):
        v = a()
        if ai == 0 and v.__class__ is Rng:
            first_b = isbool(v.sh, v.r1, v.c1)
        if v.__class__ is Rng:
            vals = rng_values(v)
            d = CELLS[v.sh]
            for idx, x in enumerate(vals):
                if x.__class__ is Err:
                    return x
                if x.__class__ is not float:
                    continue
                cnt += 1
                cw = v.c2 - v.c1 + 1
                rr = v.r1 + idx // cw
                cc = v.c1 + idx % cw
                if ai == 0 and not first_seen:
                    first_seen = True
                    first_b = isbool(v.sh, rr, cc)
                if best is None or (x > best if want_max else x < best):
                    best = x
                    best_b = isbool(v.sh, rr, cc)
        else:
            if v.__class__ is Err:
                return v
            if v is None:
                continue
            if v.__class__ is str:
                return E_VAL
            x = tonum(v)
            if x.__class__ is Err:
                return x
            cnt += 1
            if best is None or (x > best if want_max else x < best):
                best = x
                best_b = False
    if best is None:
        return 0.0
    if best_b and first_b:
        return best != 0
    return best


@fn('MIN')
def f_min(args):
    return _minmax(args, False)


@fn('MAX')
def f_max(args):
    return _minmax(args, True)


@fn('COUNT')
def f_count(args):
    n = 0
    for a in args:
        v = a()
        if v.__class__ is Rng:
            n += sum(1 for x in rng_values(v) if x.__class__ is float)
        elif v.__class__ is float:
            n += 1
        elif v.__class__ is str and parse_num(v) is not None:
            n += 1
        elif v.__class__ is bool:
            n += 1
    return float(n)


@fn('COUNTA')
def f_counta(args):
    n = 0
    for a in args:
        v = a()
        if v.__class__ is Rng:
            n += sum(1 for x in rng_values(v) if x is not None)
        elif v is not None:
            n += 1
    return float(n)


@fn('COUNTBLANK')
def f_countblank(args):
    r = args[0]()
    n = 0
    for v in rng_values(r):
        if v is None or v == '':
            n += 1
    return float(n)


@fn('MEDIAN')
def f_median(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    if not vals:
        return E_NUM
    s = sorted(vals)
    n = len(s)
    if n % 2:
        return s[n // 2]
    return (s[n // 2 - 1] + s[n // 2]) / 2.0


def _var(vals, sample=True):
    n = len(vals)
    if n < (2 if sample else 1):
        return None
    mu = ksum(vals) / n
    return ksum([(v - mu) ** 2 for v in vals]) / (n - 1 if sample else n)


@fn('VAR', 'VAR.S')
def f_var(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    r = _var(vals)
    return E_DIV if r is None else r


@fn('STDEV', 'STDEV.S')
def f_stdev(args):
    vals = collect_nums(args)
    if vals.__class__ is Err:
        return vals
    r = _var(vals)
    return E_DIV if r is None else math.sqrt(r)


@fn('LARGE', 'SMALL')
def f_large(args):
    raise NotImplementedError


def _kth(args, large):
    vals = collect_nums([args[0]()])
    if vals.__class__ is Err:
        return vals
    k = scal(args, 1)
    if k.__class__ is Err:
        return k
    k = tonum(k)
    if k.__class__ is Err:
        return k
    k = int(k)
    if k < 1 or k > len(vals):
        return E_VAL
    s = sorted(vals, reverse=large)
    return s[k - 1]


FUNCS['LARGE'] = lambda args: _kth(args, True)
FUNCS['SMALL'] = lambda args: _kth(args, False)


@fn('ABS')
def f_abs(args):
    x = tonum(scal(args, 0))
    return x if x.__class__ is Err else abs(x)


@fn('INT')
def f_int(args):
    x = tonum(scal(args, 0))
    return x if x.__class__ is Err else float(math.floor(x))


@fn('TRUNC')
def f_trunc(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    n = 0
    if len(args) > 1:
        n = tonum(scal(args, 1))
        if n.__class__ is Err:
            return n
    return dround(x, int(n), ROUND_DOWN)


@fn('ROUND')
def f_round(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    n = tonum(scal(args, 1)) if len(args) > 1 else 0.0
    if n.__class__ is Err:
        return n
    return dround(x, int(n), ROUND_HALF_UP)


@fn('ROUNDDOWN')
def f_rounddown(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    n = tonum(scal(args, 1)) if len(args) > 1 else 0.0
    if n.__class__ is Err:
        return n
    return dround(x, int(n), ROUND_DOWN)


@fn('ROUNDUP')
def f_roundup(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    n = tonum(scal(args, 1)) if len(args) > 1 else 0.0
    if n.__class__ is Err:
        return n
    return dround(x, int(n), ROUND_UP)


@fn('SIGN')
def f_sign(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    return 1.0 if x > 0 else (-1.0 if x < 0 else 0.0)


@fn('MOD')
def f_mod(args):
    n = tonum(scal(args, 0))
    if n.__class__ is Err:
        return n
    d = tonum(scal(args, 1))
    if d.__class__ is Err:
        return d
    if d == 0:
        return E_DIV
    return n - d * math.floor(n / d)


@fn('SQRT')
def f_sqrt(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    if x < 0:
        return E_VAL
    return math.sqrt(x)


@fn('EXP')
def f_exp(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    try:
        return math.exp(x)
    except OverflowError:
        return E_NUM


@fn('LN')
def f_ln(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    if x <= 0:
        return E_VAL
    return math.log(x)


@fn('LOG10')
def f_log10(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    if x <= 0:
        return E_VAL
    return math.log10(x)


@fn('LOG')
def f_log(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    b = 10.0
    if len(args) > 1:
        b = tonum(scal(args, 1))
        if b.__class__ is Err:
            return b
    if b <= 0:
        return E_VAL
    if b == 1:
        return E_DIV
    if x <= 0:
        return E_VAL
    return math.log(x) / math.log(b)


@fn('POWER')
def f_power(args):
    x = tonum(scal(args, 0))
    if x.__class__ is Err:
        return x
    y = tonum(scal(args, 1))
    if y.__class__ is Err:
        return y
    return fpow(x, y)


@fn('NA')
def f_na(args):
    return E_NA


@fn('PI')
def f_pi(args):
    return math.pi


@fn('SUMPRODUCT')
def f_sumproduct(args):
    arrs = []
    for a in args:
        v = a()
        if v.__class__ is Rng:
            arrs.append((rng_values(v), v.r2 - v.r1 + 1, v.c2 - v.c1 + 1))
        else:
            v = deref(v)
            if v.__class__ is Err:
                return v
            arrs.append(([v], 1, 1))
    n = len(arrs[0][0])
    for a in arrs:
        if len(a[0]) != n:
            return E_VAL
    prods = []
    for i in range(n):
        p = 1.0
        for a in arrs:
            v = a[0][i]
            if v.__class__ is Err:
                return v
            if v.__class__ is float:
                p *= v
            else:
                p *= 0.0
        prods.append(p)
    return ksum(prods)


# ---------- text ----------
@fn('LEN')
def f_len(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    return float(len(tostr(v)))


@fn('LOWER')
def f_lower(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    return tostr(v).lower()


@fn('UPPER')
def f_upper(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    return tostr(v).upper()


@fn('PROPER')
def f_proper(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    s = tostr(v)
    out = []
    prev_alpha = False
    for ch in s:
        if ch.isalpha():
            out.append(ch.lower() if prev_alpha else ch.upper())
            prev_alpha = True
        else:
            out.append(ch)
            prev_alpha = False
    return ''.join(out)


@fn('TRIM')
def f_trim(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    return ' '.join(tostr(v).split(' '))


def _trim_xl(s):
    return re.sub(' +', ' ', s).strip(' ')


FUNCS['TRIM'] = lambda args: (lambda v: v if v.__class__ is Err else _trim_xl(tostr(v)))(scal(args, 0))


@fn('LEFT')
def f_left(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    n = 1.0
    if len(args) > 1:
        n = tonum(scal(args, 1))
        if n.__class__ is Err:
            return n
    if n < 0:
        return E_VAL
    return tostr(v)[:int(n)]


@fn('RIGHT')
def f_right(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    n = 1.0
    if len(args) > 1:
        n = tonum(scal(args, 1))
        if n.__class__ is Err:
            return n
    if n < 0:
        return E_VAL
    s = tostr(v)
    k = int(n)
    return s[max(0, len(s) - k):] if k > 0 else ''


@fn('MID')
def f_mid(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    st = tonum(scal(args, 1))
    if st.__class__ is Err:
        return st
    n = tonum(scal(args, 2))
    if n.__class__ is Err:
        return n
    st = int(st)
    n = int(n)
    if st < 1 or n < 0:
        return E_VAL
    return tostr(v)[st - 1:st - 1 + n]


@fn('REPT')
def f_rept(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    n = tonum(scal(args, 1))
    if n.__class__ is Err:
        return n
    n = int(n)
    if n < 0:
        return E_VAL
    return tostr(v) * n


@fn('CONCATENATE')
def f_concatenate(args):
    out = []
    for i in range(len(args)):
        v = scal(args, i)
        if v.__class__ is Err:
            return v
        out.append(tostr(v))
    return ''.join(out)


@fn('CONCAT')
def f_concat(args):
    out = []
    for a in args:
        v = a()
        for x in flat_vals(v):
            x = deref(x) if x.__class__ is Rng else x
            if x.__class__ is Err:
                return x
            out.append(tostr(x))
    return ''.join(out)


@fn('TEXTJOIN')
def f_textjoin(args):
    d = scal(args, 0)
    if d.__class__ is Err:
        return d
    d = tostr(d)
    ign = tobool(scal(args, 1))
    if ign.__class__ is Err:
        return ign
    out = []
    for a in args[2:]:
        v = a()
        for x in flat_vals(v):
            if x.__class__ is Err:
                return x
            if ign and (x is None or x == ''):
                continue
            out.append(tostr(x))
    return d.join(out)


def _find(args, ci):
    f = scal(args, 0)
    w = scal(args, 1)
    if f.__class__ is Err:
        return f
    if w.__class__ is Err:
        return w
    st = 1.0
    if len(args) > 2:
        st = tonum(scal(args, 2))
        if st.__class__ is Err:
            return st
    f = tostr(f)
    w = tostr(w)
    st = int(st)
    if st < 1 or st > len(w) + 1:
        return E_VAL
    if ci:
        i = w.lower().find(f.lower(), st - 1)
    else:
        i = w.find(f, st - 1)
    if i < 0:
        return E_VAL
    return float(i + 1)


@fn('FIND')
def f_find(args):
    return _find(args, False)


@fn('SEARCH')
def f_search(args):
    f = scal(args, 0)
    w = scal(args, 1)
    if f.__class__ is Err:
        return f
    if w.__class__ is Err:
        return w
    st = 1.0
    if len(args) > 2:
        st = tonum(scal(args, 2))
        if st.__class__ is Err:
            return st
    st = int(st)
    f = tostr(f)
    w = tostr(w)
    if st < 1 or st > len(w) + 1:
        return E_VAL
    rx = get_wild(f)
    for i in range(st - 1, len(w)):
        for L in range(len(w) - i, -1, -1):
            pass
        # wildcard match of f against w[i:i+len(f)] (or any length for * patterns)
        m = None
        if '*' in f or '?' in f:
            for L in range(0, len(w) - i + 1):
                if rx.match(w[i:i + L]):
                    m = True
                    break
        else:
            m = w[i:i + len(f)].lower() == f.lower() and len(f) > 0 or (len(f) == 0 and i >= 0)
            if len(f) == 0:
                m = True
        if m:
            return float(i + 1)
    return E_VAL


@fn('SUBSTITUTE')
def f_substitute(args):
    t = scal(args, 0)
    o = scal(args, 1)
    n = scal(args, 2)
    if t.__class__ is Err:
        return t
    if o.__class__ is Err:
        return o
    if n.__class__ is Err:
        return n
    t = tostr(t)
    o = tostr(o)
    n = tostr(n)
    if len(args) > 3:
        inst = tonum(scal(args, 3))
        if inst.__class__ is Err:
            return inst
        inst = int(inst)
        if inst < 1:
            return E_VAL
        if o == '':
            return t
        parts = t.split(o)
        if inst >= len(parts):
            return t
        return o.join(parts[:inst]) + n + o.join(parts[inst:]) if False else (o.join(parts[:inst]) + n + o.join(parts[inst:]))
    if o == '':
        return t
    return t.replace(o, n)


@fn('REPLACE')
def f_replace(args):
    t = scal(args, 0)
    if t.__class__ is Err:
        return t
    st = tonum(scal(args, 1))
    if st.__class__ is Err:
        return st
    n = tonum(scal(args, 2))
    if n.__class__ is Err:
        return n
    new = scal(args, 3)
    if new.__class__ is Err:
        return new
    t = tostr(t)
    st = int(st)
    n = int(n)
    if st < 1 or n < 0:
        return E_VAL
    return t[:st - 1] + tostr(new) + t[st - 1 + n:]


@fn('EXACT')
def f_exact(args):
    a = scal(args, 0)
    b = scal(args, 1)
    if a.__class__ is Err:
        return a
    if b.__class__ is Err:
        return b
    return tostr(a) == tostr(b)


@fn('VALUE')
def f_value(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    if v.__class__ is float:
        return v
    if v is None:
        return 0.0
    if v.__class__ is bool:
        return E_VAL
    p = parse_num(tostr(v))
    if p is None:
        return E_VAL
    return p


@fn('TEXT')
def f_text(args):
    v = scal(args, 0)
    if v.__class__ is Err:
        return v
    f = scal(args, 1)
    if f.__class__ is Err:
        return f
    return text_fmt(v, tostr(f))


# ---------- criteria functions ----------
def _pairs(args, start):
    pairs = []
    for i in range(start, len(args), 2):
        r = args[i]()
        c = deref(args[i + 1]()) if i + 1 < len(args) else None
        pairs.append((r, c))
    return pairs


def _mask_rng(rng_list, crit_list, n_rows, n_cols):
    """rng_list: list of Rng; returns list of row-major indices mask"""
    return None


def ksum(vals):
    tot = 0.0
    c = 0.0
    for x in vals:
        y = x - c
        t = tot + y
        c = (t - tot) - y
        tot = t
    return tot


def _and2(a, b):
    return a and b


def _cells_match(pairs):
    """Yield positional index lists matching all criteria. Returns (list of (r,c) offsets) as list of bools"""
    base = pairs[0][0]
    if base.__class__ is not Rng:
        return None
    h = base.r2 - base.r1 + 1
    w = base.c2 - base.c1 + 1
    mask = None
    for r, crit in pairs:
        if r.__class__ is not Rng:
            return None
        if r.r2 - r.r1 != base.r2 - base.r1 or r.c2 - r.c1 != base.c2 - base.c1:
            return E_VAL
        m = rng_mask(r, crit)
        mask = m if mask is None else _mand(mask, m)
    return mask, h, w


def _crit_error(crit):
    return crit.__class__ is Err


@fn('COUNTIF')
def f_countif(args):
    r = args[0]()
    crit = scal(args, 1)
    if crit.__class__ is Err:
        return crit
    if r.__class__ is not Rng:
        return E_VAL
    m = rng_mask(r, crit)
    return float(sum(m))


@fn('COUNTIFS')
def f_countifs(args):
    pairs = _pairs(args, 0)
    for r, c in pairs:
        if c.__class__ is Err:
            return c
    res = _cells_match(pairs)
    if res is None:
        return E_VAL
    if res.__class__ is Err:
        return res
    return float(sum(res[0]))


@fn('SUMIF')
def f_sumif(args):
    r = args[0]()
    crit = scal(args, 1)
    if crit.__class__ is Err:
        return crit
    sr = args[2]() if len(args) > 2 else r
    if r.__class__ is not Rng or sr.__class__ is not Rng:
        return E_VAL
    m = rng_mask(r, crit)
    sv = rng_values(sr)
    tot = 0.0
    for i, ok in enumerate(m):
        if i < len(sv) and ok:
            x = sv[i]
            if x.__class__ is float:
                tot += x
            elif x.__class__ is Err:
                return x
    return tot


@fn('SUMIFS')
def f_sumifs(args):
    sr = args[0]()
    pairs = _pairs(args, 1)
    for r, c in pairs:
        if c.__class__ is Err:
            return c
    if sr.__class__ is not Rng:
        return E_VAL
    res = _cells_match(pairs)
    if res is None:
        return E_VAL
    if res.__class__ is Err:
        return res
    mask = res[0]
    nums, isn, haserr = numinfo(sr)
    if len(nums) != len(mask):
        return E_VAL
    if haserr:
        sv = rng_values(sr)
        for m, x in zip(mask, sv):
            if m and x.__class__ is Err:
                return x
    return ksum(compress(nums, mask))


@fn('AVERAGEIF')
def f_averageif(args):
    r = args[0]()
    crit = scal(args, 1)
    if crit.__class__ is Err:
        return crit
    sr = args[2]() if len(args) > 2 else r
    if r.__class__ is not Rng or sr.__class__ is not Rng:
        return E_VAL
    m = rng_mask(r, crit)
    sv = rng_values(sr)
    tot = 0.0
    n = 0
    for i, ok in enumerate(m):
        if i < len(sv) and ok:
            x = sv[i]
            if x.__class__ is float:
                tot += x
                n += 1
            elif x.__class__ is Err:
                return x
    if n == 0:
        return E_DIV
    return tot / n


@fn('AVERAGEIFS')
def f_averageifs(args):
    sr = args[0]()
    pairs = _pairs(args, 1)
    for r, c in pairs:
        if c.__class__ is Err:
            return c
    if sr.__class__ is not Rng:
        return E_VAL
    res = _cells_match(pairs)
    if res is None:
        return E_VAL
    if res.__class__ is Err:
        return res
    mask = res[0]
    nums, isn, haserr = numinfo(sr)
    if len(nums) != len(mask):
        return E_VAL
    if haserr:
        sv = rng_values(sr)
        for m, x in zip(mask, sv):
            if m and x.__class__ is Err:
                return x
    n = sum(compress(isn, mask))
    if n == 0:
        return E_DIV
    return ksum(compress(nums, mask)) / n


# ---------- lookup ----------
def _lk_match(needle, v, wild):
    if needle.__class__ is str and wild:
        rx = get_wild(needle)
        return v.__class__ is str and rx.match(v) is not None
    return cmp_vals(v, needle) == 0 and v.__class__ is needle.__class__ or (
        needle.__class__ in (float, int) and v.__class__ is float and approx_eq(v, float(needle)))


def _exact_index_cached(needle, vals, ck):
    idx = IDXCACHE.get(ck)
    if idx is None:
        idx = {}
        for i, v in enumerate(vals):
            if v.__class__ is float:
                k = v
            elif v.__class__ is str:
                k = v.lower()
            else:
                continue
            if k not in idx:
                idx[k] = i
        IDXCACHE[ck] = idx
    if needle.__class__ is float:
        return idx.get(needle, -1)
    if needle.__class__ is str:
        if '*' in needle or '?' in needle or '~' in needle:
            return _exact_index(needle, vals)
        return idx.get(needle.lower(), -1)
    return _exact_index(needle, vals)


def _exact_index(needle, vals, wild=True):
    if needle.__class__ is str and wild and ('*' in needle or '?' in needle or '~' in needle):
        for i, v in enumerate(vals):
            if _lk_match(needle, v, True):
                return i
        return -1
    if needle.__class__ is float:
        for i, v in enumerate(vals):
            if v.__class__ is float and approx_eq(v, needle):
                return i
        return -1
    if needle.__class__ is str:
        nl = needle.lower()
        for i, v in enumerate(vals):
            if v.__class__ is str and v.lower() == nl:
                return i
        return -1
    if needle.__class__ is bool:
        for i, v in enumerate(vals):
            if v.__class__ is bool and v == needle:
                return i
        return -1
    return -1


def _approx_index_lin(needle, vals, asc=True):
    best = -1
    bv = None
    for i, v in enumerate(vals):
        if v is None or v.__class__ is Err or type_rank(v) != type_rank(needle):
            continue
        if v.__class__ is str and needle.__class__ is str:
            vl, nl = v.lower(), needle.lower()
        else:
            vl, nl = v, needle
        if v.__class__ is float:
            if asc:
                ok = vl < nl or approx_eq(vl, nl)
            else:
                ok = vl > nl or approx_eq(vl, nl)
        elif v.__class__ is str:
            ok = (vl <= nl) if asc else (vl >= nl)
        else:
            ok = (vl <= nl) if asc else (vl >= nl)
        if ok:
            if bv is None or (cmp_vals(v, bv) > 0 if asc else cmp_vals(v, bv) < 0):
                best = i
                bv = v
    return best


def _approx_index(needle, vals):
    # binary search: last element <= needle, types ordered number < text < bool
    nr = type_rank(needle)
    best = -1
    lo, hi = 0, len(vals) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        v = vals[mid]
        if v is None or v.__class__ is Err:
            lo = mid + 1
            continue
        rv = type_rank(v)
        if rv != nr:
            if rv < nr:
                lo = mid + 1
            else:
                hi = mid - 1
            continue
        c = cmp_vals(v, needle)
        if c <= 0:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    if best < 0:
        return _approx_index_lin(needle, vals, True)
    return best


def _lookup_args(args):
    return None


def _vlookup_common(args, horiz):
    needle = scal(args, 0)
    if needle.__class__ is Err:
        return needle
    tbl = args[1]()
    idx = scal(args, 2)
    if idx.__class__ is Err:
        return idx
    idx = tonum(idx)
    if idx.__class__ is Err:
        return idx
    approx = True
    if len(args) > 3:
        ap = scal(args, 3)
        if ap.__class__ is Err:
            return ap
        approx = tobool(ap)
        if approx.__class__ is Err:
            return approx
    if tbl.__class__ is not Rng:
        return E_VAL
    h = tbl.r2 - tbl.r1 + 1
    w = tbl.c2 - tbl.c1 + 1
    k = int(idx)
    if horiz:
        if k < 1 or k > h:
            return E_VAL
        keyvals = rng_values(Rng(tbl.sh, tbl.r1, tbl.c1, tbl.r1, tbl.c2))
    else:
        if k < 1 or k > w:
            return E_VAL
        keyvals = rng_values(Rng(tbl.sh, tbl.r1, tbl.c1, tbl.r2, tbl.c1))
    if approx:
        if needle is None:
            return E_NA
        i = _approx_index(needle, keyvals)
        if i < 0:
            return E_NA
    else:
        if horiz:
            i = _exact_index(needle, keyvals)
        else:
            i = _exact_index_cached(needle, keyvals, (tbl.sh, tbl.r1, tbl.c1, tbl.r2, tbl.c1))
        if i < 0:
            return E_NA
    if horiz:
        r = tbl.r1 + k - 1
        cc = tbl.c1 + i
        return Rng(tbl.sh, r, cc, r, cc)
    r = tbl.r1 + i
    cc = tbl.c1 + k - 1
    return Rng(tbl.sh, r, cc, r, cc)


@fn('VLOOKUP')
def f_vlookup(args):
    r = _vlookup_common(args, False)
    return deref(r) if r.__class__ is Rng else r


@fn('HLOOKUP')
def f_hlookup(args):
    r = _vlookup_common(args, True)
    return deref(r) if r.__class__ is Rng else r


@fn('MATCH')
def f_match(args):
    needle = scal(args, 0)
    if needle.__class__ is Err:
        return needle
    arr = args[1]()
    mt = 1.0
    if len(args) > 2:
        mt = tonum(scal(args, 2))
        if mt.__class__ is Err:
            return mt
    if arr.__class__ is not Rng:
        return E_VAL
    vals = rng_values(arr)
    if needle is None:
        needle = 0.0
    if mt == 0:
        i = _exact_index_cached(needle, vals, (arr.sh, arr.r1, arr.c1, arr.r2, arr.c2))
    elif mt > 0:
        i = _approx_index(needle, vals)
    else:
        i = _approx_index_lin(needle, vals, False)
    if False:
        # smallest value >= needle, descending
        i = -1
        lo, hi = 0, len(vals) - 1
        best = -1
        while lo <= hi:
            mid = (lo + hi) // 2
            c = cmp_vals(vals[mid], needle)
            if c.__class__ is Err:
                hi = mid - 1
                continue
            if c >= 0:
                best = mid
                lo = mid + 1
            else:
                hi = mid - 1
        i = best
    if i < 0:
        return E_NA
    return float(i + 1)


@fn('INDEX')
def f_index(args):
    arr = args[0]()
    if arr.__class__ is not Rng:
        return E_VAL
    r = scal(args, 1)
    c = scal(args, 2) if len(args) > 2 else None
    if r.__class__ is Err:
        return r
    h = arr.r2 - arr.r1 + 1
    w = arr.c2 - arr.c1 + 1
    ri = int(tonum(r)) if r is not None else 0
    ci = int(tonum(c)) if c is not None else 0
    if c is None and len(args) <= 2:
        if h == 1 and w > 1:
            ci, ri = ri, 1
        elif w == 1:
            ci = 1
    if ri == 0 and ci == 0 and h == 1 and w == 1:
        ri = ci = 1
    if ri < 0 or ci < 0:
        return E_VAL
    if ri == 0 or ci == 0:
        return E_VAL
    if ri > h or ci > w:
        return E_REF
    rr = arr.r1 + ri - 1
    cc = arr.c1 + ci - 1
    return Rng(arr.sh, rr, cc, rr, cc)


@fn('XLOOKUP')
def f_xlookup(args):
    needle = scal(args, 0)
    if needle.__class__ is Err:
        return needle
    la = args[1]()
    ra = args[2]()
    if la.__class__ is not Rng or ra.__class__ is not Rng:
        return E_VAL
    nf = args[3]() if len(args) > 3 else None
    mm = 0.0
    if len(args) > 4:
        mm = tonum(scal(args, 4))
        if mm.__class__ is Err:
            return mm
    sm = 1.0
    if len(args) > 5:
        sm = tonum(scal(args, 5))
        if sm.__class__ is Err:
            return sm
    lv = rng_values(la)
    n = len(lv)
    order = range(n) if sm >= 0 else range(n - 1, -1, -1)
    idx = -1
    if mm == 0 and sm == 1 and needle.__class__ in (float, str):
        idx = _exact_index_cached(needle, lv, (la.sh, la.r1, la.c1, la.r2, la.c2))
    elif mm == 0:
        for i in order:
            if _eqv(lv[i], needle):
                idx = i
                break
    elif mm == 2:
        rx = get_wild(tostr(needle))
        for i in order:
            v = lv[i]
            if v.__class__ is str and rx.match(v):
                idx = i
                break
            if v.__class__ is float and needle.__class__ is float and approx_eq(v, needle):
                idx = i
                break
    else:
        # -1: exact or next smaller; 1: exact or next larger
        best = None
        bi = -1
        for i in order:
            v = lv[i]
            if v is None or v.__class__ is Err:
                continue
            if _eqv(v, needle):
                bi = i
                best = None
                break
            c = cmp_vals(v, needle)
            if c.__class__ is Err or type_rank(v) != type_rank(needle):
                continue
            if mm == -1 and c < 0:
                if best is None or cmp_vals(v, best) > 0:
                    best = v
                    bi = i
            if mm == 1 and c > 0:
                if best is None or cmp_vals(v, best) < 0:
                    best = v
                    bi = i
        idx = bi
    if idx < 0:
        if nf is None:
            return E_NA
        return nf
    rv = rng_values(ra)
    if idx >= len(rv):
        return E_REF
    return Rng(ra.sh, ra.r1 + (idx // (ra.c2 - ra.c1 + 1)) if ra.r2 > ra.r1 else ra.r1, (ra.c1 + idx) if ra.r2 == ra.r1 else ra.c1, 0, 0) if False else _xl_ret(ra, idx)


def _xl_ret(ra, idx):
    w = ra.c2 - ra.c1 + 1
    h = ra.r2 - ra.r1 + 1
    if h == 1:
        r, c = ra.r1, ra.c1 + idx
    else:
        r, c = ra.r1 + idx, ra.c1
    return Rng(ra.sh, r, c, r, c)


def _eqv(a, b):
    if a is None:
        return False
    if a.__class__ is Err or b.__class__ is Err:
        return False
    if a.__class__ is str and b.__class__ is str:
        return a.lower() == b.lower()
    if type_rank(a) != type_rank(b):
        return False
    if a.__class__ is float:
        return approx_eq(a, b)
    return a == b


# ---------- date functions ----------
@fn('DATE')
def f_date(args):
    y = tonum(scal(args, 0))
    m = tonum(scal(args, 1))
    d = tonum(scal(args, 2))
    for x in (y, m, d):
        if x.__class__ is Err:
            return x
    y = int(y)
    m = int(m)
    d = int(d)
    if 0 <= y < 1900:
        y += 1900
    if y < 0 or y > 9999:
        return E_NUM
    y += (m - 1) // 12
    m = (m - 1) % 12 + 1
    try:
        base = date(y, m, 1)
    except ValueError:
        return E_NUM
    res = base + timedelta(days=d - 1)
    return serial_of(res)


def _sd(args, i):
    v = tonum(scal(args, i))
    if v.__class__ is Err:
        return v
    try:
        return to_date(v)
    except OverflowError:
        return E_NUM


@fn('YEAR')
def f_year(args):
    d = _sd(args, 0)
    return d if d.__class__ is Err else float(d.year)


@fn('MONTH')
def f_month(args):
    d = _sd(args, 0)
    return d if d.__class__ is Err else float(d.month)


@fn('DAY')
def f_day(args):
    d = _sd(args, 0)
    return d if d.__class__ is Err else float(d.day)


@fn('WEEKDAY')
def f_weekday(args):
    s = tonum(scal(args, 0))
    if s.__class__ is Err:
        return s
    t = 1
    if len(args) > 1:
        t = tonum(scal(args, 1))
        if t.__class__ is Err:
            return t
    n = int(math.floor(s))
    t = int(t)
    if t == 1:
        return float((n + 6) % 7 + 1)
    if t == 2:
        return float((n + 5) % 7 + 1)
    if t == 3:
        return float((n + 5) % 7)
    return E_NUM


@fn('DAYS')
def f_days(args):
    e = tonum(scal(args, 0))
    if e.__class__ is Err:
        return e
    s = tonum(scal(args, 1))
    if s.__class__ is Err:
        return s
    return float(e - s)


def _sd_trunc(args, i):
    v = tonum(scal(args, i))
    if v.__class__ is Err:
        return v
    try:
        return EPOCH + timedelta(days=int(v))
    except OverflowError:
        return E_NUM


@fn('EDATE')
def f_edate(args):
    s = _sd_trunc(args, 0)
    if s.__class__ is Err:
        return s
    m = tonum(scal(args, 1))
    if m.__class__ is Err:
        return m
    return serial_of(add_months(s, int(m)))


@fn('EOMONTH')
def f_eomonth(args):
    s = _sd_trunc(args, 0)
    if s.__class__ is Err:
        return s
    m = tonum(scal(args, 1))
    if m.__class__ is Err:
        return m
    d = add_months(s.replace(day=1), int(m))
    return serial_of(date(d.year, d.month, days_in_month(d.year, d.month)))


@fn('DATEDIF')
def f_datedif(args):
    s = tonum(scal(args, 0))
    if s.__class__ is Err:
        return s
    e = tonum(scal(args, 1))
    if e.__class__ is Err:
        return e
    unit = scal(args, 2)
    if unit.__class__ is Err:
        return unit
    unit = tostr(unit).upper()
    if s > e:
        return E_VAL
    d1 = to_date(s)
    d2 = to_date(e)
    if unit == 'Y':
        y = d2.year - d1.year
        if (d2.month, d2.day) < (d1.month, d1.day):
            y -= 1
        return float(y)
    if unit == 'M':
        m = (d2.year - d1.year) * 12 + (d2.month - d1.month)
        if d2.day < d1.day:
            m -= 1
        return float(m)
    if unit == 'D':
        return float((d2 - d1).days)
    if unit == 'MD':
        if d2.day >= d1.day:
            return float(d2.day - d1.day)
        pm_y, pm_m = (d2.year, d2.month - 1) if d2.month > 1 else (d2.year - 1, 12)
        return float(days_in_month(pm_y, pm_m) - d1.day + d2.day)
    if unit == 'YM':
        m = (d2.month - d1.month)
        if d2.day < d1.day:
            m -= 1
        return float(m % 12)
    if unit == 'YD':
        y = d2.year
        try:
            s2 = date(y, d1.month, d1.day)
        except ValueError:
            s2 = date(y, d1.month, 28)
        if s2 > d2:
            try:
                s2 = date(y - 1, d1.month, d1.day)
            except ValueError:
                s2 = date(y - 1, d1.month, 28)
        return float((d2 - s2).days)
    return E_VAL


# ---------- driver ----------
def load_book(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    relmap = {r.get("Id"): r.get("Target") for r in rels}
    ss = []
    if "xl/sharedStrings.xml" in names:
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall(M + "si"):
            t = si.find(M + "t")
            if t is not None:
                ss.append(t.text or "")
            else:
                ss.append("".join((r.findtext(M + "t") or "") for r in si.findall(M + "r")))
    sheets = []
    for s in wb.find(M + "sheets"):
        target = relmap[s.get(R_ID)]
        p = target.lstrip("/") if target.startswith("/") else "xl/" + target
        sheets.append((s.get("name"), p))
    raw = {}
    forms = []  # (sheetname, r, c, formula text)
    for name, p in sheets:
        root = ET.fromstring(z.read(p))
        cells = {}
        CELLS[name] = cells
        for c in root.iter(M + "c"):
            ref = c.get("r")
            m = re.fullmatch(r'([A-Z]+)(\d+)', ref)
            key = (int(m.group(2)), col2n(m.group(1)))
            t = c.get("t", "n")
            f = c.find(M + "f")
            v = c.find(M + "v")
            vt = v.text if v is not None else None
            if f is not None:
                forms.append((name, key, f.text or ""))
                continue
            if vt is None:
                if t == "inlineStr":
                    is_ = c.find(M + "is")
                    if is_ is not None:
                        cells[key] = "".join(x.text or "" for x in is_.iter(M + "t"))
                continue
            if t == "s":
                cells[key] = ss[int(vt)]
            elif t in ("str", "inlineStr"):
                cells[key] = vt
            elif t == "b":
                cells[key] = 1.0 if vt in ("1", "true") else 0.0
                BOOLS.setdefault(name, set()).add(key)
            elif t == "e":
                cells[key] = ERRMAP.get(vt, E_VAL)
            else:
                cells[key] = float(vt)
    return [s[0] for s in sheets], forms


def run(inp, outp):
    names, forms = load_book(inp)
    FCOF = lambda n, k: CELLS[n][k]
    for name, key, text in forms:
        try:
            fnc = Parser(tokenize(text), name).parse()
        except Exception:
            fnc = (lambda: E_NAME)
        CELLS[name][key] = FC(fnc)
    out = {}
    for name, key, text in forms:
        try:
            v = cellval(name, key[0], key[1])
        except Exception:
            v = E_VAL
        out.setdefault(name, {})
        ref = colname(key[1]) + str(key[0])
        if FCOF(name, key).bl and v.__class__ is float:
            e = {"t": "b", "v": v != 0}
        elif v.__class__ is float or v.__class__ is int:
            if math.isnan(v) or math.isinf(v):
                e = {"t": "e", "v": "#NUM!"}
            else:
                e = {"t": "n", "v": float(v)}
        elif v.__class__ is str:
            e = {"t": "s", "v": v}
        elif v.__class__ is Err:
            e = {"t": "e", "v": v.c}
        else:
            e = {"t": "n", "v": 0.0}
        out[name][ref] = e
    with open(outp, "w") as fh:
        json.dump(out, fh)


def colname(n):
    s = ''
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


def main(argv):
    import gc
    gc.disable()
    inp = outp = None
    i = 0
    while i < len(argv):
        if argv[i] == '--in':
            inp = argv[i + 1]
            i += 2
        elif argv[i] == '--out':
            outp = argv[i + 1]
            i += 2
        else:
            i += 1
    if inp is None or outp is None:
        print("usage: calc eval --in book.xlsx --out values.json", file=sys.stderr)
        return 2
    sys.setrecursionlimit(1000000)
    for mb in (512, 256, 64):
        try:
            threading.stack_size(mb * 1024 * 1024)
            break
        except Exception:
            pass
    res = [0]

    def target():
        try:
            run(inp, outp)
        except Exception as ex:
            import traceback
            traceback.print_exc()
            res[0] = 1
    t = threading.Thread(target=target)
    t.start()
    t.join()
    return res[0]


if __name__ == '__main__':
    args = sys.argv[1:]
    if args and args[0] == 'eval':
        args = args[1:]
    sys.exit(main(args))
