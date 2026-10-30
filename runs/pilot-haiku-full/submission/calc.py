#!/usr/bin/env python3
"""Spreadsheet formula engine: reads an XLSX workbook, evaluates all formulas, writes JSON values."""
import sys, os, re, json, math, zipfile, posixpath, threading, datetime, decimal
import xml.etree.ElementTree as ET
from decimal import Decimal

NS = '{http://schemas.openxmlformats.org/spreadsheetml/2006/main}'
RNS = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'
EPOCH = datetime.date(1899, 12, 30)
TD = datetime.timedelta
MAXF = 1.7976931348623157e308


class XErr(object):
    __slots__ = ('code',)

    def __init__(self, code):
        self.code = code


ERRS = {}
for _c in ['#DIV/0!', '#VALUE!', '#N/A', '#NUM!', '#REF!', '#NAME?', '#NULL!']:
    ERRS[_c] = XErr(_c)
DIV0 = ERRS['#DIV/0!']
VALUE = ERRS['#VALUE!']
NA = ERRS['#N/A']
NUM = ERRS['#NUM!']
REF = ERRS['#REF!']
NAME = ERRS['#NAME?']


class _Missing(object):
    pass


MISSING = _Missing()


class ParseErr(Exception):
    pass


class Sheet(object):
    __slots__ = ('name', 'cells')

    def __init__(self, name):
        self.name = name
        self.cells = {}


class FCell(object):
    __slots__ = ('text', 'fn', 'val', 'state')

    def __init__(self, text):
        self.text = text
        self.fn = None
        self.val = None
        self.state = 0


class Ref(object):
    __slots__ = ('sh', 'r1', 'c1', 'r2', 'c2', 'fr', 'fc')

    def __init__(self, sh, r1, c1, r2, c2, fr, fc):
        self.sh = sh
        self.r1 = r1
        self.c1 = c1
        self.r2 = r2
        self.c2 = c2
        self.fr = fr
        self.fc = fc


WB = {}


def getv(sh, r, c):
    v = sh.cells.get((r, c))
    if v.__class__ is FCell:
        if v.state == 2:
            return v.val
        return evalcell(v)
    return v


def evalcell(cell):
    if cell.state == 1:
        return VALUE
    cell.state = 1
    try:
        v = cell.fn()
        if v.__class__ is Ref:
            v = sc(v)
    except RecursionError:
        v = VALUE
    except Exception:
        if os.environ.get('CALC_DEBUG'):
            import traceback
            traceback.print_exc()
        v = VALUE
    if v is None:
        v = 0.0
    cell.val = v
    cell.state = 2
    return v


def sc(x):
    if x.__class__ is not Ref:
        return x
    r1, c1, r2, c2 = x.r1, x.c1, x.r2, x.c2
    if r1 == r2 and c1 == c2:
        return getv(x.sh, r1, c1)
    if c1 == c2 and r1 <= x.fr <= r2:
        return getv(x.sh, x.fr, c1)
    if r1 == r2 and c1 <= x.fc <= c2:
        return getv(x.sh, r1, x.fc)
    return VALUE


def rvals(x):
    sh = x.sh
    cells = sh.cells
    out = []
    ap = out.append
    for r in range(x.r1, x.r2 + 1):
        for c in range(x.c1, x.c2 + 1):
            v = cells.get((r, c))
            if v.__class__ is FCell:
                v = v.val if v.state == 2 else evalcell(v)
            ap(v)
    return out


RE_NUM = re.compile(r'[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?')
RE_THOU = re.compile(r'[+-]?\d{1,3}(?:,\d{3})+(?:\.\d*)?')


def parse_num_str(s):
    t = s.strip()
    tu = t.upper()
    if tu == 'TRUE':
        return 1.0
    if tu == 'FALSE':
        return 0.0
    pct = False
    if t.endswith('%'):
        pct = True
        t = t[:-1].rstrip()
    if RE_NUM.fullmatch(t):
        x = float(t)
    elif RE_THOU.fullmatch(t):
        x = float(t.replace(',', ''))
    else:
        return None
    if pct:
        x /= 100.0
    return x


def num_of(v):
    c = v.__class__
    if c is float:
        return v
    if v is None:
        return 0.0
    if c is bool:
        return 1.0 if v else 0.0
    if c is str:
        x = parse_num_str(v)
        return VALUE if x is None else x
    if c is XErr:
        return v
    if c is Ref:
        return num_of(sc(v))
    return VALUE


def num2str(x):
    if x == 0:
        return '0'
    if x.is_integer() and abs(x) < 1e15:
        return str(int(x))
    if 1e-15 <= abs(x) < 1e15:
        return format(Decimal('%.15g' % x), 'f')
    s = '%.15g' % x
    if 'e' in s:
        mant, ex = s.split('e')
        sign = ex[0]
        digs = ex[1:].lstrip('0').zfill(3)
        return mant + 'E' + sign + digs
    return s


def to_text(v):
    c = v.__class__
    if c is str:
        return v
    if c is float:
        return num2str(v)
    if v is None:
        return ''
    if c is bool:
        return '1' if v else '0'
    if c is XErr:
        return v
    if c is Ref:
        return to_text(sc(v))
    return ''


def aeq(a, b):
    if a == b:
        return True
    return abs(a - b) <= 2.0 ** -48 * max(abs(a), abs(b))


def cmp_vals(x, y):
    if x is None:
        x = '' if y.__class__ is str else 0.0
    if y is None:
        y = '' if x.__class__ is str else 0.0
    xs = x.__class__ is str
    ys = y.__class__ is str
    if xs:
        if ys:
            a = x.lower()
            b = y.lower()
            return -1 if a < b else (1 if a > b else 0)
        return 1
    if ys:
        return -1
    a = float(x)
    b = float(y)
    if a == b or aeq(a, b):
        return 0
    return -1 if a < b else 1


def to_bool(v):
    c = v.__class__
    if c is bool:
        return v
    if c is float:
        return v != 0
    if v is None:
        return False
    if c is XErr:
        return v
    if c is Ref:
        return to_bool(sc(v))
    return VALUE


def power(a, b):
    if a == 0:
        if b == 0:
            return 1.0
        if b < 0:
            return NUM
        return 0.0
    if a < 0 and b != math.floor(b):
        return NUM
    try:
        r = a ** b
    except (OverflowError, ZeroDivisionError):
        return NUM
    if r.__class__ is complex:
        return NUM
    if not (-MAXF < r < MAXF):
        return NUM
    return float(r)


def binop_num(op, x, y):
    if op == '/':
        if y.__class__ is XErr:
            return y
        b = num_of(y)
        if b.__class__ is XErr:
            return b
        if x.__class__ is XErr:
            if b == 0:
                return DIV0
            return x
        a = num_of(x)
        if a.__class__ is XErr:
            return a
        if b == 0:
            return DIV0
        return a / b
    if x.__class__ is XErr:
        return x
    if y.__class__ is XErr:
        return y
    a = num_of(x)
    if a.__class__ is XErr:
        return a
    b = num_of(y)
    if b.__class__ is XErr:
        return b
    if op == '+':
        r = a + b
    elif op == '-':
        r = a - b
    elif op == '*':
        r = a * b
    elif op == '/':
        if b == 0:
            return DIV0
        r = a / b
    else:
        return power(a, b)
    if not (-MAXF < r < MAXF):
        return NUM
    return r


def _add(x, y):
    if x.__class__ is float and y.__class__ is float:
        r = x + y
        if -MAXF < r < MAXF:
            return r
        return NUM
    return binop_num('+', x, y)


def _sub(x, y):
    if x.__class__ is float and y.__class__ is float:
        r = x - y
        if -MAXF < r < MAXF:
            return r
        return NUM
    return binop_num('-', x, y)


def _mul(x, y):
    if x.__class__ is float and y.__class__ is float:
        r = x * y
        if -MAXF < r < MAXF:
            return r
        return NUM
    return binop_num('*', x, y)


def _div(x, y):
    return binop_num('/', x, y)


def _pow(x, y):
    return binop_num('^', x, y)


def _concat(x, y):
    if x.__class__ is str and y.__class__ is str:
        return x + y
    if y.__class__ is XErr:
        return y
    if x.__class__ is XErr:
        return x
    a = to_text(x)
    if a.__class__ is XErr:
        return a
    b = to_text(y)
    if b.__class__ is XErr:
        return b
    return a + b


def _cmp(op, x, y):
    if y.__class__ is XErr:
        return y
    if x.__class__ is XErr:
        return x
    c = cmp_vals(x, y)
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


def _neg(x):
    a = num_of(x)
    if a.__class__ is XErr:
        return a
    return -a


def _pct(x):
    a = num_of(x)
    if a.__class__ is XErr:
        return a
    return a / 100.0


BINFN = {
    '+': _add, '-': _sub, '*': _mul, '/': _div, '^': _pow, '&': _concat,
    '=': lambda x, y: _cmp('=', x, y), '<>': lambda x, y: _cmp('<>', x, y),
    '<': lambda x, y: _cmp('<', x, y), '>': lambda x, y: _cmp('>', x, y),
    '<=': lambda x, y: _cmp('<=', x, y), '>=': lambda x, y: _cmp('>=', x, y),
}

_WILD_CACHE = {}


def wild_re(pat):
    rx = _WILD_CACHE.get(pat)
    if rx is not None:
        return rx
    out = []
    i = 0
    while i < len(pat):
        ch = pat[i]
        if ch == '~' and i + 1 < len(pat):
            out.append(re.escape(pat[i + 1]))
            i += 2
            continue
        if ch == '*':
            out.append('.*')
        elif ch == '?':
            out.append('.')
        else:
            out.append(re.escape(ch))
        i += 1
    rx = re.compile(''.join(out), re.S | re.I)
    _WILD_CACHE[pat] = rx
    return rx


def has_wild(s):
    return '*' in s or '?' in s or '~' in s


def match_exact(key, v):
    if key.__class__ is str:
        if v.__class__ is not str:
            return False
        if has_wild(key):
            return wild_re(key).fullmatch(v) is not None
        return key.lower() == v.lower()
    if v is None or v.__class__ is str or v.__class__ is XErr:
        return False
    if key is None:
        return v == 0.0 or v is False
    return cmp_vals(key, v) == 0


def crit_pred(crit):
    c = crit.__class__
    if c is XErr:
        return lambda v: False
    if c is float or c is bool:
        n = float(crit)
        return lambda v: v.__class__ is float and aeq(v, n)
    if crit is None:
        return lambda v: v is None or v == ''
    s = crit
    op = '='
    for cand in ('<=', '>=', '<>', '<', '>', '='):
        if s.startswith(cand):
            op = cand
            s = s[len(cand):]
            break
    n = parse_num_str(s) if s.strip() != '' else None
    if n is not None:
        if op == '=':
            return lambda v: v.__class__ is float and aeq(v, n)
        if op == '<>':
            return lambda v: not (v.__class__ is float and aeq(v, n))
        if op == '<':
            return lambda v: v.__class__ is float and v < n and not aeq(v, n)
        if op == '>':
            return lambda v: v.__class__ is float and v > n and not aeq(v, n)
        if op == '<=':
            return lambda v: v.__class__ is float and (v <= n or aeq(v, n))
        return lambda v: v.__class__ is float and (v >= n or aeq(v, n))
    if op in ('=', '<>'):
        if s == '':
            if op == '=':
                return lambda v: v is None or v == ''
            return lambda v: not (v is None or v == '')
        rx = wild_re(s)
        if op == '=':
            return lambda v: v.__class__ is str and rx.fullmatch(v) is not None
        return lambda v: not (v.__class__ is str and rx.fullmatch(v) is not None)
    sl = s.lower()
    if op == '<':
        return lambda v: v.__class__ is str and v.lower() < sl
    if op == '>':
        return lambda v: v.__class__ is str and v.lower() > sl
    if op == '<=':
        return lambda v: v.__class__ is str and v.lower() <= sl
    return lambda v: v.__class__ is str and v.lower() >= sl


def bsearch_le(arr, key):
    lo, hi = 0, len(arr) - 1
    best = -1
    while lo <= hi:
        mid = (lo + hi) // 2
        if cmp_vals(arr[mid], key) <= 0:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    return best


def bsearch_ge_desc(arr, key):
    lo, hi = 0, len(arr) - 1
    best = -1
    while lo <= hi:
        mid = (lo + hi) // 2
        if cmp_vals(arr[mid], key) >= 0:
            best = mid
            lo = mid + 1
        else:
            hi = mid - 1
    return best


_INDEX_CACHE = {}


def exact_index(sh, r1, c1, r2, c2, vals):
    key = (id(sh), r1, c1, r2, c2)
    d = _INDEX_CACHE.get(key)
    if d is not None:
        return d
    d = {}
    for i, v in enumerate(vals):
        k = None
        c = v.__class__
        if c is str:
            k = ('s', v.lower())
        elif c is float:
            k = ('n', v)
        elif c is bool:
            k = ('b', v)
        if k is not None and k not in d:
            d[k] = i
    _INDEX_CACHE[key] = d
    return d


def exact_find(vals, key, index=None):
    if key.__class__ is str and not has_wild(key):
        if index is not None:
            return index.get(('s', key.lower()), -1)
        kl = key.lower()
        for i, v in enumerate(vals):
            if v.__class__ is str and v.lower() == kl:
                return i
        return -1
    if key.__class__ is float and index is not None:
        return index.get(('n', key), -1)
    for i, v in enumerate(vals):
        if match_exact(key, v):
            return i
    return -1


def sc_arg(x):
    return sc(x) if x.__class__ is Ref else x


def nums_from(args):
    out = []
    ap = out.append
    for a in args:
        if a is MISSING:
            continue
        c = a.__class__
        if c is Ref:
            for v in rvals(a):
                vc = v.__class__
                if vc is float:
                    ap(v)
                elif vc is bool:
                    ap(1.0 if v else 0.0)
                elif vc is XErr:
                    return v
            continue
        if c is float:
            ap(a)
        elif c is bool:
            ap(1.0 if a else 0.0)
        elif a is None:
            continue
        elif c is str:
            return VALUE
        elif c is XErr:
            return a
    return out


def f_sum(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    return math.fsum(v)


def f_average(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    if not v:
        return DIV0
    return math.fsum(v) / len(v)


def f_min(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    return min(v) if v else 0.0


def f_max(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    return max(v) if v else 0.0


def f_count(*args):
    n = 0
    for a in args:
        if a is MISSING:
            continue
        if a.__class__ is Ref:
            for v in rvals(a):
                if v.__class__ is float or v.__class__ is bool:
                    n += 1
        else:
            c = a.__class__
            if c is float or c is bool:
                n += 1
            elif c is str and parse_num_str(a) is not None:
                n += 1
    return float(n)


def f_counta(*args):
    n = 0
    for a in args:
        if a is MISSING:
            continue
        if a.__class__ is Ref:
            for v in rvals(a):
                if v is not None:
                    n += 1
        elif a is not None:
            n += 1
    return float(n)


def f_countblank(rng=MISSING, *_):
    if rng.__class__ is not Ref:
        return 0.0
    return float(sum(1 for v in rvals(rng) if v is None or v == ''))


def f_product(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    if not v:
        return 0.0
    p = 1.0
    for x in v:
        p *= x
    if not (-MAXF < p < MAXF):
        return NUM
    return p


def f_median(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    if not v:
        return VALUE
    v = sorted(v)
    n = len(v)
    if n % 2:
        return v[n // 2]
    return (v[n // 2 - 1] + v[n // 2]) / 2.0


def _var(args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    n = len(v)
    if n < 2:
        return DIV0
    m = math.fsum(v) / n
    return math.fsum((x - m) ** 2 for x in v) / (n - 1)


def f_var(*args):
    return _var(args)


def f_stdev(*args):
    r = _var(args)
    if r.__class__ is XErr:
        return r
    return math.sqrt(r)


def f_sumsq(*args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    return math.fsum(x * x for x in v)


def f_large(rng=MISSING, k=MISSING, *_):
    v = nums_from([rng])
    if v.__class__ is XErr:
        return v
    kk = num_of(sc_arg(k))
    if kk.__class__ is XErr:
        return kk
    ki = int(math.trunc(kk))
    if ki < 1 or ki > len(v):
        return VALUE
    return sorted(v, reverse=True)[ki - 1]


def f_small(rng=MISSING, k=MISSING, *_):
    v = nums_from([rng])
    if v.__class__ is XErr:
        return v
    kk = num_of(sc_arg(k))
    if kk.__class__ is XErr:
        return kk
    ki = int(math.trunc(kk))
    if ki < 1 or ki > len(v):
        return VALUE
    return sorted(v)[ki - 1]


def f_abs(x=MISSING, *_):
    a = num_of(sc_arg(x))
    return a if a.__class__ is XErr else abs(a)


def f_int(x=MISSING, *_):
    a = num_of(sc_arg(x))
    return a if a.__class__ is XErr else float(math.floor(a))


def f_sign(x=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    return 1.0 if a > 0 else (-1.0 if a < 0 else 0.0)


def f_sqrt(x=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    if a < 0:
        return VALUE
    return math.sqrt(a)


def f_exp(x=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    try:
        r = math.exp(a)
    except OverflowError:
        return NUM
    return r if r < MAXF else NUM


def f_ln(x=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    if a <= 0:
        return VALUE
    return math.log(a)


def f_log10(x=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    if a <= 0:
        return VALUE
    return math.log10(a)


def f_log(x=MISSING, b=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    base = 10.0
    if b is not MISSING:
        base = num_of(sc_arg(b))
        if base.__class__ is XErr:
            return base
    if a <= 0 or base <= 0:
        return VALUE
    if base == 1:
        return DIV0
    return math.log(a) / math.log(base)


def f_power(x=MISSING, y=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    b = num_of(sc_arg(y))
    if b.__class__ is XErr:
        return b
    return power(a, b)


def f_pi(*_):
    return math.pi


def f_mod(n=MISSING, d=MISSING, *_):
    a = num_of(sc_arg(n))
    if a.__class__ is XErr:
        return a
    b = num_of(sc_arg(d))
    if b.__class__ is XErr:
        return b
    if b == 0:
        return DIV0
    return a - b * math.floor(a / b)


def _round_dec(x, n, mode):
    if x != x or x in (float('inf'), float('-inf')):
        return NUM
    if abs(x) >= 1e15:
        return x
    try:
        d = Decimal('%.15g' % x)
        q = Decimal(1).scaleb(-n)
        with decimal.localcontext() as ctx:
            ctx.prec = 60
            r = d.quantize(q, rounding=mode)
        return float(r)
    except Exception:
        return x


def f_round(x=MISSING, n=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    k = num_of(sc_arg(n))
    if k.__class__ is XErr:
        return k
    return _round_dec(a, int(math.trunc(k)), decimal.ROUND_HALF_UP)


def f_rounddown(x=MISSING, n=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    k = num_of(sc_arg(n))
    if k.__class__ is XErr:
        return k
    return _round_dec(a, int(math.trunc(k)), decimal.ROUND_DOWN)


def f_roundup(x=MISSING, n=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    k = num_of(sc_arg(n))
    if k.__class__ is XErr:
        return k
    return _round_dec(a, int(math.trunc(k)), decimal.ROUND_UP)


def f_trunc(x=MISSING, n=MISSING, *_):
    a = num_of(sc_arg(x))
    if a.__class__ is XErr:
        return a
    k = 0
    if n is not MISSING:
        kk = num_of(sc_arg(n))
        if kk.__class__ is XErr:
            return kk
        k = int(math.trunc(kk))
    return _round_dec(a, k, decimal.ROUND_DOWN)


def f_if(c=MISSING, a=MISSING, b=MISSING, *_):
    cv = to_bool(sc_arg(c))
    if cv.__class__ is XErr:
        return cv
    if cv:
        return sc_arg(a) if a is not MISSING else 0.0
    return sc_arg(b) if b is not MISSING else False


def f_iferror(v=MISSING, alt=MISSING, *_):
    x = sc_arg(v)
    if x.__class__ is XErr:
        return sc_arg(alt) if alt is not MISSING else 0.0
    return x


def f_ifna(v=MISSING, alt=MISSING, *_):
    x = sc_arg(v)
    if x is NA:
        return sc_arg(alt) if alt is not MISSING else 0.0
    return x


def f_ifs(*args):
    for i in range(0, len(args) - 1, 2):
        c = to_bool(sc_arg(args[i]))
        if c.__class__ is XErr:
            return c
        if c:
            return sc_arg(args[i + 1])
    return NA


def f_switch(expr=MISSING, *args):
    x = sc_arg(expr)
    if x.__class__ is XErr:
        return x
    n = len(args)
    for i in range(0, n - 1, 2):
        v = sc_arg(args[i])
        if v.__class__ is XErr:
            continue
        if cmp_vals(x, v) == 0 and (x.__class__ is str) == (v.__class__ is str):
            return sc_arg(args[i + 1])
    if n % 2 == 1:
        return sc_arg(args[-1])
    return NA


def f_choose(idx=MISSING, *vals):
    i = num_of(sc_arg(idx))
    if i.__class__ is XErr:
        return i
    ii = int(math.trunc(i))
    if ii < 1 or ii > len(vals):
        return VALUE
    v = vals[ii - 1]
    return sc_arg(v) if v is not MISSING else 0.0


def _logical_collect(args):
    vals = []
    for a in args:
        if a is MISSING:
            continue
        if a.__class__ is Ref:
            for v in rvals(a):
                c = v.__class__
                if c is XErr:
                    return v
                if c is bool:
                    vals.append(v)
                elif c is float:
                    vals.append(v != 0)
            continue
        c = a.__class__
        if c is XErr:
            return a
        if c is bool:
            vals.append(a)
        elif c is float:
            vals.append(a != 0)
        elif a is None:
            continue
        else:
            return VALUE
    return vals


def f_and(*args):
    vals = _logical_collect(args)
    if vals.__class__ is XErr:
        return vals
    if not vals:
        return VALUE
    return all(vals)


def f_or(*args):
    vals = _logical_collect(args)
    if vals.__class__ is XErr:
        return vals
    if not vals:
        return VALUE
    return any(vals)


def f_xor(*args):
    vals = _logical_collect(args)
    if vals.__class__ is XErr:
        return vals
    if not vals:
        return VALUE
    return sum(1 for v in vals if v) % 2 == 1


def f_not(x=MISSING, *_):
    v = to_bool(sc_arg(x))
    if v.__class__ is XErr:
        return v
    return not v


def f_true(*_):
    return True


def f_false(*_):
    return False


def f_isblank(x=MISSING, *_):
    if x.__class__ is Ref:
        return sc(x) is None
    return False


def f_iserror(x=MISSING, *_):
    return sc_arg(x).__class__ is XErr


def f_isna(x=MISSING, *_):
    return sc_arg(x) is NA


def f_iserr(x=MISSING, *_):
    v = sc_arg(x)
    return v.__class__ is XErr and v is not NA


def f_isnumber(x=MISSING, *_):
    v = sc_arg(x)
    return v.__class__ is float or v.__class__ is bool


def f_istext(x=MISSING, *_):
    return sc_arg(x).__class__ is str


def f_islogical(x=MISSING, *_):
    return sc_arg(x).__class__ is bool


def f_n(x=MISSING, *_):
    v = sc_arg(x)
    c = v.__class__
    if c is float:
        return v
    if c is bool:
        return 1.0 if v else 0.0
    if c is XErr:
        return v
    return 0.0


def f_t(x=MISSING, *_):
    v = sc_arg(x)
    if v.__class__ is XErr:
        return v
    if v.__class__ is str:
        return v
    return ''


def _text1(x):
    return to_text(sc_arg(x))


def f_len(x=MISSING, *_):
    t = _text1(x)
    if t.__class__ is XErr:
        return t
    return float(len(t))


def f_left(x=MISSING, n=MISSING, *_):
    t = _text1(x)
    if t.__class__ is XErr:
        return t
    k = 1.0
    if n is not MISSING:
        k = num_of(sc_arg(n))
        if k.__class__ is XErr:
            return k
    if k < 0:
        return VALUE
    return t[:int(math.trunc(k))]


def f_right(x=MISSING, n=MISSING, *_):
    t = _text1(x)
    if t.__class__ is XErr:
        return t
    k = 1.0
    if n is not MISSING:
        k = num_of(sc_arg(n))
        if k.__class__ is XErr:
            return k
    if k < 0:
        return VALUE
    k = int(math.trunc(k))
    if k == 0:
        return ''
    return t[-k:]


def f_mid(x=MISSING, s=MISSING, n=MISSING, *_):
    t = _text1(x)
    if t.__class__ is XErr:
        return t
    sv = num_of(sc_arg(s))
    if sv.__class__ is XErr:
        return sv
    nv = num_of(sc_arg(n))
    if nv.__class__ is XErr:
        return nv
    si = int(math.trunc(sv))
    ni = int(math.trunc(nv))
    if si < 1 or ni < 0:
        return VALUE
    return t[si - 1:si - 1 + ni]


def f_find(needle=MISSING, hay=MISSING, start=MISSING, *_):
    nd = _text1(needle)
    if nd.__class__ is XErr:
        return nd
    hs = _text1(hay)
    if hs.__class__ is XErr:
        return hs
    st = 1
    if start is not MISSING:
        sv = num_of(sc_arg(start))
        if sv.__class__ is XErr:
            return sv
        st = int(math.trunc(sv))
    if st < 1 or st > len(hs) + 1:
        return VALUE
    i = hs.find(nd, st - 1)
    if i < 0:
        return VALUE
    return float(i + 1)


def f_search(needle=MISSING, hay=MISSING, start=MISSING, *_):
    nd = _text1(needle)
    if nd.__class__ is XErr:
        return nd
    hs = _text1(hay)
    if hs.__class__ is XErr:
        return hs
    st = 1
    if start is not MISSING:
        sv = num_of(sc_arg(start))
        if sv.__class__ is XErr:
            return sv
        st = int(math.trunc(sv))
    if st < 1 or st > len(hs) + 1:
        return VALUE
    if nd == '':
        return VALUE
    m = wild_re(nd).search(hs, st - 1)
    if m is None:
        return VALUE
    return float(m.start() + 1)


def f_replace(old=MISSING, start=MISSING, num=MISSING, new=MISSING, *_):
    o = _text1(old)
    if o.__class__ is XErr:
        return o
    sv = num_of(sc_arg(start))
    if sv.__class__ is XErr:
        return sv
    nv = num_of(sc_arg(num))
    if nv.__class__ is XErr:
        return nv
    w = _text1(new)
    if w.__class__ is XErr:
        return w
    si = int(math.trunc(sv))
    ni = int(math.trunc(nv))
    if si < 1 or ni < 0:
        return VALUE
    return o[:si - 1] + w + o[si - 1 + ni:]


def f_substitute(text=MISSING, old=MISSING, new=MISSING, inst=MISSING, *_):
    t = _text1(text)
    if t.__class__ is XErr:
        return t
    o = _text1(old)
    if o.__class__ is XErr:
        return o
    w = _text1(new)
    if w.__class__ is XErr:
        return w
    if o == '':
        return t
    if inst is MISSING:
        return t.replace(o, w)
    iv = num_of(sc_arg(inst))
    if iv.__class__ is XErr:
        return iv
    k = int(math.trunc(iv))
    if k < 1:
        return VALUE
    parts = t.split(o)
    if k >= len(parts):
        return t
    return o.join(parts[:k]) + w + o.join(parts[k:])


def f_rept(text=MISSING, n=MISSING, *_):
    t = _text1(text)
    if t.__class__ is XErr:
        return t
    k = num_of(sc_arg(n))
    if k.__class__ is XErr:
        return k
    ki = int(math.trunc(k))
    if ki < 0:
        return VALUE
    return t * ki


def f_trim(text=MISSING, *_):
    t = _text1(text)
    if t.__class__ is XErr:
        return t
    return re.sub(' +', ' ', t).strip(' ')


def f_upper(text=MISSING, *_):
    t = _text1(text)
    return t if t.__class__ is XErr else t.upper()


def f_lower(text=MISSING, *_):
    t = _text1(text)
    return t if t.__class__ is XErr else t.lower()


def f_proper(text=MISSING, *_):
    t = _text1(text)
    if t.__class__ is XErr:
        return t
    out = []
    prev_alpha = False
    for ch in t:
        if ch.isalpha():
            out.append(ch.lower() if prev_alpha else ch.upper())
            prev_alpha = True
        else:
            out.append(ch)
            prev_alpha = False
    return ''.join(out)


def f_exact(a=MISSING, b=MISSING, *_):
    x = _text1(a)
    if x.__class__ is XErr:
        return x
    y = _text1(b)
    if y.__class__ is XErr:
        return y
    return x == y


def f_concatenate(*args):
    out = []
    for a in args:
        if a is MISSING:
            continue
        t = to_text(sc_arg(a))
        if t.__class__ is XErr:
            return t
        out.append(t)
    return ''.join(out)


def f_concat(*args):
    out = []
    for a in args:
        if a is MISSING:
            continue
        vals = rvals(a) if a.__class__ is Ref else [a]
        for v in vals:
            t = ('1' if v else '0') if (v.__class__ is bool and a.__class__ is Ref) else to_text(v)
            if t.__class__ is XErr:
                return t
            out.append(t)
    return ''.join(out)


def f_textjoin(delim=MISSING, ign=MISSING, *args):
    d = to_text(sc_arg(delim))
    if d.__class__ is XErr:
        return d
    ig = to_bool(sc_arg(ign))
    if ig.__class__ is XErr:
        return ig
    parts = []
    for a in args:
        if a is MISSING:
            continue
        vals = rvals(a) if a.__class__ is Ref else [a]
        for v in vals:
            if v.__class__ is XErr:
                return v
            if ig and (v is None or v == ''):
                continue
            t = ('1' if v else '0') if (v.__class__ is bool and a.__class__ is Ref) else to_text(v)
            if t.__class__ is XErr:
                return t
            parts.append(t)
    return d.join(parts)


def parse_date_str(s):
    m = re.fullmatch(r'\s*(\d{4})-(\d{1,2})-(\d{1,2})\s*', s)
    if not m:
        return None
    try:
        d = datetime.date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
    except ValueError:
        return None
    return float((d - EPOCH).days)


def f_value(x=MISSING, *_):
    v = sc_arg(x)
    c = v.__class__
    if c is float:
        return v
    if c is XErr:
        return v
    if v is None:
        return 0.0
    if c is str:
        r = parse_num_str(v)
        if r is not None:
            return r
        d = parse_date_str(v)
        if d is not None:
            return d
        return VALUE
    return VALUE


def _serial_date_any(s):
    n = int(math.floor(s))
    try:
        return EPOCH + TD(days=n)
    except OverflowError:
        return None


def _serial_date(s):
    n = int(math.floor(s))
    if n < 0:
        return None
    try:
        return EPOCH + TD(days=n)
    except OverflowError:
        return None


def _date_serial(d):
    return float((d - EPOCH).days)


def _dim(y, m):
    if m == 12:
        return 31
    return (datetime.date(y, m + 1, 1) - datetime.date(y, m, 1)).days


def f_date(y=MISSING, m=MISSING, d=MISSING, *_):
    ys = num_of(sc_arg(y))
    if ys.__class__ is XErr:
        return ys
    ms = num_of(sc_arg(m))
    if ms.__class__ is XErr:
        return ms
    ds = num_of(sc_arg(d))
    if ds.__class__ is XErr:
        return ds
    yi = int(math.trunc(ys))
    mi = int(math.trunc(ms))
    di = int(math.trunc(ds))
    if 0 <= yi < 100:
        yi += 1900
    if yi < 1 or yi > 9999:
        return NUM
    yr = yi + (mi - 1) // 12
    mo = (mi - 1) % 12 + 1
    if yr > 9999:
        return NUM
    try:
        base = datetime.date(yr, mo, 1)
        res = base + TD(days=di - 1)
    except (ValueError, OverflowError):
        return NUM
    return _date_serial(res)


def _date1(x):
    v = num_of(sc_arg(x))
    if v.__class__ is XErr:
        return v
    d = _serial_date(v)
    if d is None:
        return NUM
    return d


def f_year(x=MISSING, *_):
    d = _date1(x)
    return d if d.__class__ is XErr else float(d.year)


def f_month(x=MISSING, *_):
    d = _date1(x)
    return d if d.__class__ is XErr else float(d.month)


def f_day(x=MISSING, *_):
    d = _date1(x)
    return d if d.__class__ is XErr else float(d.day)


def f_weekday(x=MISSING, t=MISSING, *_):
    d = _date1(x)
    if d.__class__ is XErr:
        return d
    typ = 1
    if t is not MISSING:
        tv = num_of(sc_arg(t))
        if tv.__class__ is XErr:
            return tv
        typ = int(math.trunc(tv))
    wd = d.weekday()
    if typ == 1:
        return float((wd + 1) % 7 + 1)
    if typ == 2 or typ == 11:
        return float(wd + 1)
    if typ == 3:
        return float(wd)
    if 12 <= typ <= 17:
        start = (typ - 11) % 7
        return float((wd - start) % 7 + 1)
    return VALUE


def f_days(end=MISSING, start=MISSING, *_):
    e = num_of(sc_arg(end))
    if e.__class__ is XErr:
        return e
    s = num_of(sc_arg(start))
    if s.__class__ is XErr:
        return s
    return float(e - s)


def f_edate(start=MISSING, months=MISSING, *_):
    v = num_of(sc_arg(start))
    if v.__class__ is XErr:
        return v
    mm = num_of(sc_arg(months))
    if mm.__class__ is XErr:
        return mm
    d = _serial_date_any(v)
    if d is None:
        return NUM
    tot = d.year * 12 + (d.month - 1) + int(math.trunc(mm))
    y = tot // 12
    mo = tot % 12 + 1
    if y < 0 or y > 9999:
        return NUM
    day = min(d.day, _dim(y, mo))
    return _date_serial(datetime.date(y, mo, day))


def f_eomonth(start=MISSING, months=MISSING, *_):
    v = num_of(sc_arg(start))
    if v.__class__ is XErr:
        return v
    mm = num_of(sc_arg(months))
    if mm.__class__ is XErr:
        return mm
    d = _serial_date_any(v)
    if d is None:
        return NUM
    tot = d.year * 12 + (d.month - 1) + int(math.trunc(mm))
    y = tot // 12
    mo = tot % 12 + 1
    if y < 0 or y > 9999:
        return NUM
    return _date_serial(datetime.date(y, mo, _dim(y, mo)))


def f_datedif(start=MISSING, end=MISSING, unit=MISSING, *_):
    s = num_of(sc_arg(start))
    if s.__class__ is XErr:
        return s
    e = num_of(sc_arg(end))
    if e.__class__ is XErr:
        return e
    u = _text1(unit)
    if u.__class__ is XErr:
        return u
    si = int(math.trunc(s))
    ei = int(math.trunc(e))
    if si < 0 or ei < 0 or si > ei:
        return VALUE
    d1 = _serial_date(si)
    d2 = _serial_date(ei)
    u = u.upper()
    if u == 'Y':
        return float(d2.year - d1.year - (1 if (d2.month, d2.day) < (d1.month, d1.day) else 0))
    if u == 'M':
        return float((d2.year - d1.year) * 12 + (d2.month - d1.month) - (1 if d2.day < d1.day else 0))
    if u == 'D':
        return float(ei - si)
    if u == 'MD':
        if d2.day >= d1.day:
            return float(d2.day - d1.day)
        pm = d2.month - 1 if d2.month > 1 else 12
        py = d2.year if d2.month > 1 else d2.year - 1
        return float(_dim(py, pm) + d2.day - d1.day)
    if u == 'YM':
        return float(((d2.month - d1.month) - (1 if d2.day < d1.day else 0)) % 12)
    if u == 'YD':
        y = d2.year
        if (d1.month, d1.day) > (d2.month, d2.day):
            y -= 1
        try:
            anchor = datetime.date(y, d1.month, d1.day)
        except ValueError:
            anchor = datetime.date(y, d1.month, 28)
        return float((d2 - anchor).days)
    return VALUE


def f_vlookup(v=MISSING, t=MISSING, idx=MISSING, approx=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if t.__class__ is not Ref:
        return VALUE
    ci = num_of(sc_arg(idx))
    if ci.__class__ is XErr:
        return ci
    ci = int(math.trunc(ci))
    ncols = t.c2 - t.c1 + 1
    if ci < 1:
        return VALUE
    if ci > ncols:
        return VALUE
    ap = True
    if approx is not MISSING:
        ap = to_bool(sc_arg(approx))
        if ap.__class__ is XErr:
            return ap
    sh = t.sh
    keys = [getv(sh, r, t.c1) for r in range(t.r1, t.r2 + 1)]
    if not ap:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(sh, t.r1, t.c1, t.r2, t.c2, keys)
        i = exact_find(keys, key, idxd)
    else:
        i = bsearch_le(keys, key)
    if i < 0:
        return NA
    return getv(sh, t.r1 + i, t.c1 + ci - 1)


def f_hlookup(v=MISSING, t=MISSING, idx=MISSING, approx=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if t.__class__ is not Ref:
        return VALUE
    ri = num_of(sc_arg(idx))
    if ri.__class__ is XErr:
        return ri
    ri = int(math.trunc(ri))
    nrows = t.r2 - t.r1 + 1
    if ri < 1:
        return VALUE
    if ri > nrows:
        return VALUE
    ap = True
    if approx is not MISSING:
        ap = to_bool(sc_arg(approx))
        if ap.__class__ is XErr:
            return ap
    sh = t.sh
    keys = [getv(sh, t.r1, c) for c in range(t.c1, t.c2 + 1)]
    if not ap:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(sh, t.r1, t.c1, t.r2, t.c2, keys)
        i = exact_find(keys, key, idxd)
    else:
        i = bsearch_le(keys, key)
    if i < 0:
        return NA
    return getv(sh, t.r1 + ri - 1, t.c1 + i)


def f_match(v=MISSING, rng=MISSING, typ=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if rng.__class__ is not Ref:
        return NA
    mt = 1
    if typ is not MISSING:
        mv = num_of(sc_arg(typ))
        if mv.__class__ is XErr:
            return mv
        mt = int(math.trunc(mv))
    vals = rvals(rng)
    if mt == 0:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(rng.sh, rng.r1, rng.c1, rng.r2, rng.c2, vals)
        i = exact_find(vals, key, idxd)
    elif mt > 0:
        i = bsearch_le(vals, key)
    else:
        i = bsearch_ge_desc(vals, key)
    if i < 0:
        return NA
    return float(i + 1)


def f_index(rng=MISSING, r=MISSING, c=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    rows = rng.r2 - rng.r1 + 1
    cols = rng.c2 - rng.c1 + 1
    ri = None
    ci = None
    if r is not MISSING:
        rv = num_of(sc_arg(r))
        if rv.__class__ is XErr:
            return rv
        ri = int(math.trunc(rv))
    if c is not MISSING:
        cv = num_of(sc_arg(c))
        if cv.__class__ is XErr:
            return cv
        ci = int(math.trunc(cv))
    if ci is None and ri is not None and rows == 1 and cols > 1:
        ci = ri
        ri = 1
    if ri is None:
        ri = 1
    if ci is None:
        ci = 1
    if ri < 0 or ci < 0:
        return VALUE
    if ri > rows or ci > cols:
        return REF
    if ri < 1 or ci < 1:
        return VALUE
    return getv(rng.sh, rng.r1 + ri - 1, rng.c1 + ci - 1)


def f_xlookup(v=MISSING, look=MISSING, ret=MISSING, nf=MISSING, mm=MISSING, sm=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if look.__class__ is not Ref or ret.__class__ is not Ref:
        return VALUE
    match_mode = 0
    if mm is not MISSING:
        x = num_of(sc_arg(mm))
        if x.__class__ is XErr:
            return x
        match_mode = int(math.trunc(x))
    search_mode = 1
    if sm is not MISSING:
        x = num_of(sc_arg(sm))
        if x.__class__ is XErr:
            return x
        search_mode = int(math.trunc(x))
    vals = rvals(look)
    rvs = rvals(ret)
    n = len(vals)
    if len(rvs) != n:
        return VALUE
    order = range(n) if search_mode >= 0 else range(n - 1, -1, -1)
    found = -1
    if match_mode == 0 or match_mode == 2:
        if (key.__class__ is not str or not has_wild(key)) and match_mode == 0 and search_mode == 1:
            idxd = exact_index(look.sh, look.r1, look.c1, look.r2, look.c2, vals)
            found = exact_find(vals, key, idxd)
        else:
            for i in order:
                if match_exact(key, vals[i]):
                    found = i
                    break
    else:
        for i in order:
            if match_exact(key, vals[i]):
                found = i
                break
        if found < 0 and key.__class__ is float:
            best = -1
            bestv = None
            for i in order:
                x = vals[i]
                if x.__class__ is not float:
                    continue
                if match_mode == -1 and x < key:
                    if bestv is None or x > bestv:
                        bestv = x
                        best = i
                elif match_mode == 1 and x > key:
                    if bestv is None or x < bestv:
                        bestv = x
                        best = i
            found = best
    if found < 0:
        return sc_arg(nf) if nf is not MISSING else NA
    return rvs[found]


def f_countif(rng=MISSING, crit=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    pred = crit_pred(c)
    return float(sum(1 for v in rvals(rng) if pred(v)))


def _pairs(args):
    out = []
    for i in range(0, len(args) - 1, 2):
        rng = args[i]
        if rng.__class__ is not Ref:
            return None
        cr = sc_arg(args[i + 1])
        out.append((rng, cr))
    return out


def _masks(pairs):
    n = None
    masks = []
    for rng, cr in pairs:
        if cr.__class__ is XErr:
            return cr
        size = (rng.r2 - rng.r1 + 1) * (rng.c2 - rng.c1 + 1)
        if n is None:
            n = size
        elif n != size:
            return VALUE
        pred = crit_pred(cr)
        masks.append([pred(v) for v in rvals(rng)])
    return masks, (n or 0)


def _combine(masks, n):
    if not masks:
        return [True] * n
    res = masks[0]
    for m in masks[1:]:
        res = [a and b for a, b in zip(res, m)]
    return res


def f_countifs(*args):
    pairs = _pairs(args)
    if pairs is None:
        return VALUE
    mk = _masks(pairs)
    if mk.__class__ is XErr:
        return mk
    masks, n = mk
    return float(sum(1 for b in _combine(masks, n) if b))


def _sumrange_vals(sumr, rng):
    h = rng.r2 - rng.r1 + 1
    w = rng.c2 - rng.c1 + 1
    sub = Ref(sumr.sh, sumr.r1, sumr.c1, sumr.r1 + h - 1, sumr.c1 + w - 1, sumr.fr, sumr.fc)
    return rvals(sub)


def f_sumif(rng=MISSING, crit=MISSING, sumr=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    pred = crit_pred(c)
    vals = rvals(rng)
    if sumr.__class__ is not Ref:
        sv = vals
    else:
        sv = _sumrange_vals(sumr, rng)
    tot = []
    for v, s in zip(vals, sv):
        if pred(v):
            if s.__class__ is XErr:
                return s
            if s.__class__ is float:
                tot.append(s)
    return math.fsum(tot)


def f_averageif(rng=MISSING, crit=MISSING, avg=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    pred = crit_pred(c)
    vals = rvals(rng)
    if avg.__class__ is not Ref:
        sv = vals
    else:
        sv = _sumrange_vals(avg, rng)
    tot = []
    for v, s in zip(vals, sv):
        if pred(v):
            if s.__class__ is XErr:
                return s
            if s.__class__ is float:
                tot.append(s)
    if not tot:
        return DIV0
    return math.fsum(tot) / len(tot)


def f_sumifs(sumr=MISSING, *args):
    if sumr.__class__ is not Ref:
        return VALUE
    pairs = _pairs(args)
    if pairs is None:
        return VALUE
    mk = _masks(pairs)
    if mk.__class__ is XErr:
        return mk
    masks, n = mk
    sv = rvals(sumr)
    if len(sv) != n:
        return VALUE
    sel = _combine(masks, n)
    tot = []
    for s, ok in zip(sv, sel):
        if ok:
            if s.__class__ is XErr:
                return s
            if s.__class__ is float:
                tot.append(s)
    return math.fsum(tot)


def f_averageifs(avg=MISSING, *args):
    if avg.__class__ is not Ref:
        return VALUE
    pairs = _pairs(args)
    if pairs is None:
        return VALUE
    mk = _masks(pairs)
    if mk.__class__ is XErr:
        return mk
    masks, n = mk
    sv = rvals(avg)
    if len(sv) != n:
        return VALUE
    sel = _combine(masks, n)
    tot = []
    for s, ok in zip(sv, sel):
        if ok:
            if s.__class__ is XErr:
                return s
            if s.__class__ is float:
                tot.append(s)
    if not tot:
        return DIV0
    return math.fsum(tot) / len(tot)


def f_sumproduct(*args):
    arrs = []
    shape = None
    for a in args:
        if a.__class__ is Ref:
            sh_ = (a.r2 - a.r1 + 1, a.c2 - a.c1 + 1)
            if shape is None:
                shape = sh_
            elif shape != sh_:
                return VALUE
            arrs.append(rvals(a))
        else:
            if a.__class__ is XErr:
                return a
            if shape is None:
                shape = (1, 1)
            elif shape != (1, 1):
                return VALUE
            arrs.append([a])
    if not arrs:
        return VALUE
    n = len(arrs[0])
    tot = 0.0
    for i in range(n):
        p = 1.0
        bad = False
        for arr in arrs:
            v = arr[i]
            if v.__class__ is XErr:
                return v
            if v.__class__ is bool:
                v = 1.0 if v else 0.0
            if v.__class__ is not float:
                bad = True
            else:
                p *= v
        if not bad:
            tot += p
    return tot


def split_sections(fmt):
    secs = []
    cur = []
    inq = False
    i = 0
    while i < len(fmt):
        ch = fmt[i]
        if ch == '"':
            inq = not inq
            cur.append(ch)
        elif ch == '\\' and i + 1 < len(fmt):
            cur.append(fmt[i:i + 2])
            i += 2
            continue
        elif ch == ';' and not inq:
            secs.append(''.join(cur))
            cur = []
        else:
            cur.append(ch)
        i += 1
    secs.append(''.join(cur))
    return secs


DATE_TOK = re.compile(r'"[^"]*"|\\.|\[[^\]]*\]|yyyy|yy|mmmm|mmm|mm|m|dddd|ddd|dd|d|hh|h|ss|s|AM/PM|am/pm|A/P|a/p|.', re.I)


def is_date_fmt(sec):
    stripped = re.sub(r'"[^"]*"|\\.', '', sec)
    stripped = re.sub(r'\[[^\]]*\]', '', stripped)
    return re.search(r'[ymdhs]', stripped, re.I) is not None


MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December']
DAYS_ = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']


def fmt_date(x, sec):
    d = _serial_date(x)
    if d is None:
        return None
    frac = x - math.floor(x)
    secs = int(round(frac * 86400))
    if secs >= 86400:
        secs = 0
    hh = secs // 3600
    mi = (secs % 3600) // 60
    ss = secs % 60
    toks = DATE_TOK.findall(sec)
    has_ampm = any(t.upper() in ('AM/PM', 'A/P') for t in toks)
    out = []
    for i, t in enumerate(toks):
        tl = t.lower()
        if t.startswith('"'):
            out.append(t[1:-1])
        elif t.startswith('\\'):
            out.append(t[1:])
        elif t.startswith('['):
            continue
        elif tl == 'yyyy':
            out.append('%04d' % d.year)
        elif tl == 'yy':
            out.append('%02d' % (d.year % 100))
        elif tl in ('m', 'mm'):
            prev = None
            for j in range(i - 1, -1, -1):
                if not toks[j].startswith('"'):
                    prev = toks[j].lower()
                    break
            nxt = None
            for j in range(i + 1, len(toks)):
                if not toks[j].startswith('"'):
                    nxt = toks[j].lower()
                    break
            if prev in ('h', 'hh') or nxt in ('s', 'ss'):
                out.append('%02d' % mi if tl == 'mm' else str(mi))
            else:
                out.append('%02d' % d.month if tl == 'mm' else str(d.month))
        elif tl == 'mmm':
            out.append(MONTHS[d.month - 1][:3])
        elif tl == 'mmmm':
            out.append(MONTHS[d.month - 1])
        elif tl == 'dddd':
            out.append(DAYS_[d.weekday()])
        elif tl == 'ddd':
            out.append(DAYS_[d.weekday()][:3])
        elif tl == 'dd':
            out.append('%02d' % d.day)
        elif tl == 'd':
            out.append(str(d.day))
        elif tl in ('hh', 'h'):
            h = hh
            if has_ampm:
                h = hh % 12
                if h == 0:
                    h = 12
            out.append('%02d' % h if tl == 'hh' else str(h))
        elif tl in ('ss', 's'):
            out.append('%02d' % ss if tl == 'ss' else str(ss))
        elif tl == 'am/pm':
            out.append('AM' if hh < 12 else 'PM')
        elif tl == 'a/p':
            out.append('A' if hh < 12 else 'P')
        else:
            out.append(t)
    return ''.join(out)


NUMRUN = re.compile(r'[#0?][#0?,]*(?:\.[#0?]*)?(?:[Ee][+-][0#?]+)?|\.[#0?]+(?:[Ee][+-][0#?]+)?')


def _fixed_round(x, decimals):
    try:
        d = Decimal('%.15g' % float(x))
        q = Decimal(1).scaleb(-decimals)
        r = d.quantize(q, rounding=decimal.ROUND_HALF_UP)
        return '{:f}'.format(r)
    except Exception:
        return '%.*f' % (decimals, x)


def fmt_number(x, sec, allow_sign=True):
    stripped_lit = re.sub(r'"[^"]*"', lambda m: m.group(0)[1:-1].replace('%', '\x01'), sec)
    pct = stripped_lit.count('%')
    if pct:
        x = x * (100.0 ** pct)
    m = NUMRUN.search(stripped_lit)
    if m is None:
        return re.sub(r'\\(.)', r'\1', stripped_lit).replace('\x01', '%')
    pre = stripped_lit[:m.start()]
    post = stripped_lit[m.end():]
    core = m.group(0)
    pre = re.sub(r'\\(.)', r'\1', pre).replace('\x01', '%')
    post = re.sub(r'\\(.)', r'\1', post).replace('\x01', '%')
    neg = x < 0 and allow_sign
    ax = abs(x)
    if 'E' in core or 'e' in core:
        mant_part, ex_part = re.split(r'[Ee]', core, maxsplit=1)
        ip, _, fp = mant_part.partition('.')
        decimals = sum(1 for ch in fp if ch in '0#?')
        expd = sum(1 for ch in ex_part if ch == '0')
        esign = ex_part[0]
        if ax == 0:
            e = 0
            mv = 0.0
        else:
            e = int(math.floor(math.log10(ax)))
            mv = ax / (10.0 ** e)
        ms = _fixed_round(mv, decimals)
        if float(ms) >= 10.0:
            e += 1
            mv = ax / (10.0 ** e)
            ms = _fixed_round(mv, decimals)
        numstr = ms + ('E' + ('-' if e < 0 else ('+' if esign == '+' else '')) + str(abs(e)).zfill(expd))
        return pre + ('-' if neg else '') + numstr + post
    ip, _, fp = core.partition('.')
    grouping = ',' in ip
    min_int = sum(1 for ch in ip if ch == '0')
    decimals = sum(1 for ch in fp if ch in '0#?')
    mand = sum(1 for ch in fp if ch == '0')
    s = _fixed_round(ax, decimals)
    if '.' in s:
        si, sf = s.split('.')
    else:
        si, sf = s, ''
    si = si.lstrip('0')
    if len(si) < min_int:
        si = si.zfill(min_int)
    if grouping and si:
        groups = []
        while len(si) > 3:
            groups.insert(0, si[-3:])
            si = si[:-3]
        groups.insert(0, si)
        si = ','.join(groups)
    while len(sf) > mand and sf.endswith('0'):
        sf = sf[:-1]
    numstr = si + ('.' + sf if sf else '')
    return pre + ('-' if neg else '') + numstr + post


def fmt_text(x, fmt):
    if fmt.strip().lower() == 'general':
        return num2str(x)
    secs = split_sections(fmt)
    if x < 0 and len(secs) >= 2:
        sec = secs[1]
        if is_date_fmt(sec):
            return fmt_date(-x, sec) or ''
        return fmt_number(-x, sec, allow_sign=False)
    if x == 0 and len(secs) >= 3:
        return fmt_number(0.0, secs[2])
    sec = secs[0]
    if is_date_fmt(sec):
        r = fmt_date(x, sec)
        return r if r is not None else NUM
    if '@' in sec:
        return sec.replace('@', num2str(x))
    return fmt_number(x, sec)


def f_text(v=MISSING, f=MISSING, *_):
    x = sc_arg(v)
    if x.__class__ is XErr:
        return x
    fm = to_text(sc_arg(f))
    if fm.__class__ is XErr:
        return fm
    if x.__class__ is str:
        n = parse_num_str(x)
        if n is None:
            return x
        x = n
    if x is None:
        x = 0.0
    if x.__class__ is bool:
        x = 1.0 if x else 0.0
    return fmt_text(float(x), fm)


FUNCS = {
    'SUM': f_sum, 'AVERAGE': f_average, 'MIN': f_min, 'MAX': f_max, 'COUNT': f_count,
    'COUNTA': f_counta, 'COUNTBLANK': f_countblank, 'PRODUCT': f_product, 'MEDIAN': f_median,
    'VAR': f_var, 'STDEV': f_stdev, 'SUMSQ': f_sumsq, 'LARGE': f_large, 'SMALL': f_small,
    'ABS': f_abs, 'INT': f_int, 'SIGN': f_sign, 'SQRT': f_sqrt, 'EXP': f_exp, 'LN': f_ln,
    'LOG10': f_log10, 'LOG': f_log, 'POWER': f_power, 'PI': f_pi, 'MOD': f_mod,
    'ROUND': f_round, 'ROUNDDOWN': f_rounddown, 'ROUNDUP': f_roundup, 'TRUNC': f_trunc,
    'IF': f_if, 'IFERROR': f_iferror, 'IFNA': f_ifna, 'IFS': f_ifs, 'SWITCH': f_switch,
    'CHOOSE': f_choose, 'AND': f_and, 'OR': f_or, 'XOR': f_xor, 'NOT': f_not,
    'TRUE': f_true, 'FALSE': f_false,
    'ISBLANK': f_isblank, 'ISERROR': f_iserror, 'ISNA': f_isna, 'ISERR': f_iserr,
    'ISNUMBER': f_isnumber, 'ISTEXT': f_istext, 'ISLOGICAL': f_islogical, 'N': f_n, 'T': f_t,
    'LEN': f_len, 'LEFT': f_left, 'RIGHT': f_right, 'MID': f_mid, 'FIND': f_find,
    'SEARCH': f_search, 'REPLACE': f_replace, 'SUBSTITUTE': f_substitute, 'REPT': f_rept,
    'TRIM': f_trim, 'UPPER': f_upper, 'LOWER': f_lower, 'PROPER': f_proper, 'EXACT': f_exact,
    'CONCATENATE': f_concatenate, 'CONCAT': f_concat, 'TEXTJOIN': f_textjoin, 'VALUE': f_value,
    'DATE': f_date, 'YEAR': f_year, 'MONTH': f_month, 'DAY': f_day, 'WEEKDAY': f_weekday,
    'DAYS': f_days, 'EDATE': f_edate, 'EOMONTH': f_eomonth, 'DATEDIF': f_datedif,
    'VLOOKUP': f_vlookup, 'HLOOKUP': f_hlookup, 'MATCH': f_match, 'INDEX': f_index,
    'XLOOKUP': f_xlookup, 'COUNTIF': f_countif, 'COUNTIFS': f_countifs, 'SUMIF': f_sumif,
    'SUMIFS': f_sumifs, 'AVERAGEIF': f_averageif, 'AVERAGEIFS': f_averageifs,
    'SUMPRODUCT': f_sumproduct, 'TEXT': f_text,
}


TOKRE = re.compile(r'''
 (?P<ws>\s+)
|(?P<str>"(?:[^"]|"")*")
|(?P<err>\#(?:DIV/0!|VALUE!|N/A|NUM!|REF!|NAME\?|NULL!))
|(?P<func>[A-Za-z_][A-Za-z0-9_.]*\()
|(?P<ref>(?:(?:'(?:[^']|'')+'|[A-Za-z_][A-Za-z0-9_.]*)!)?\$?[A-Za-z]{1,3}\$?[0-9]+(?::\$?[A-Za-z]{1,3}\$?[0-9]+)?)(?![A-Za-z0-9_(])
|(?P<num>(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[Ee][+-]?[0-9]+)?)
|(?P<id>[A-Za-z_][A-Za-z0-9_.]*)
|(?P<op><>|<=|>=|[-+*/^&=<>%(),;:])
''', re.X)

CELLRE = re.compile(r'([A-Za-z]+)([0-9]+)')


def col_num(letters):
    n = 0
    for ch in letters.upper():
        n = n * 26 + (ord(ch) - 64)
    return n


def col_letters(n):
    s = ''
    while n > 0:
        n, rem = divmod(n - 1, 26)
        s = chr(65 + rem) + s
    return s


def cell_pos(text):
    m = CELLRE.fullmatch(text)
    if not m:
        raise ParseErr(text)
    return int(m.group(2)), col_num(m.group(1))


def parse_ref_text(text):
    sheet = None
    if '!' in text:
        i = text.rfind('!')
        sheet = text[:i]
        text = text[i + 1:]
        if sheet.startswith("'"):
            sheet = sheet[1:-1].replace("''", "'")
    parts = text.replace('$', '').split(':')
    r1, c1 = cell_pos(parts[0])
    if len(parts) == 2:
        r2, c2 = cell_pos(parts[1])
    else:
        r2, c2 = r1, c1
    return ('ref', sheet, min(r1, r2), min(c1, c2), max(r1, r2), max(c1, c2))


class Parser(object):
    def __init__(self, toks):
        self.t = toks
        self.i = 0

    def peek(self):
        if self.i < len(self.t):
            return self.t[self.i]
        return (None, None)

    def isop(self, s):
        k, t = self.peek()
        return k == 'op' and t == s

    def expr(self):
        left = self.concat()
        while True:
            k, t = self.peek()
            if k == 'op' and t in ('=', '<>', '<', '>', '<=', '>='):
                self.i += 1
                right = self.concat()
                left = ('bin', t, left, right)
            else:
                return left

    def concat(self):
        left = self.add()
        while self.isop('&'):
            self.i += 1
            right = self.add()
            left = ('bin', '&', left, right)
        return left

    def add(self):
        left = self.mul()
        while True:
            k, t = self.peek()
            if k == 'op' and t in ('+', '-'):
                self.i += 1
                right = self.mul()
                left = ('bin', t, left, right)
            else:
                return left

    def mul(self):
        left = self.pow()
        while True:
            k, t = self.peek()
            if k == 'op' and t in ('*', '/'):
                self.i += 1
                right = self.pow()
                left = ('bin', t, left, right)
            else:
                return left

    def pow(self):
        left = self.unary()
        while self.isop('^'):
            self.i += 1
            right = self.unary()
            left = ('bin', '^', left, right)
        return left

    def unary(self):
        if self.isop('-'):
            self.i += 1
            return ('neg', self.unary())
        if self.isop('+'):
            self.i += 1
            return self.unary()
        return self.pct()

    def pct(self):
        x = self.primary()
        while self.isop('%'):
            self.i += 1
            x = ('pct', x)
        return x

    def primary(self):
        k, t = self.peek()
        if k == 'num':
            self.i += 1
            return ('num', float(t))
        if k == 'str':
            self.i += 1
            return ('str', t[1:-1].replace('""', '"'))
        if k == 'err':
            self.i += 1
            return ('err', ERRS[t])
        if k == 'ref':
            self.i += 1
            return parse_ref_text(t)
        if k == 'func':
            self.i += 1
            name = t[:-1]
            if name.upper().startswith('_XLFN.'):
                name = name[6:]
            name = name.upper()
            args = []
            if self.isop(')'):
                self.i += 1
                return ('func', name, args)
            while True:
                if self.isop(',') or self.isop(')'):
                    args.append(('missing',))
                else:
                    args.append(self.expr())
                if self.isop(','):
                    self.i += 1
                    continue
                if self.isop(')'):
                    self.i += 1
                    break
                raise ParseErr('args')
            return ('func', name, args)
        if k == 'id':
            self.i += 1
            u = t.upper()
            if u == 'TRUE':
                return ('bool', True)
            if u == 'FALSE':
                return ('bool', False)
            return ('name', t)
        if k == 'op' and t == '(':
            self.i += 1
            e = self.expr()
            if not self.isop(')'):
                raise ParseErr('paren')
            self.i += 1
            return e
        raise ParseErr('primary')


def tokenize(s):
    toks = []
    pos = 0
    n = len(s)
    while pos < n:
        m = TOKRE.match(s, pos)
        if m is None:
            raise ParseErr(s)
        pos = m.end()
        k = m.lastgroup
        if k == 'ws':
            continue
        toks.append((k, m.group(k)))
    return toks


def parse_formula(text):
    toks = tokenize(text)
    p = Parser(toks)
    e = p.expr()
    if p.i != len(toks):
        raise ParseErr(text)
    return e


def resolve_sheet(name, sh):
    if name is None:
        return sh
    t = WB.get(name)
    if t is None:
        for k, v in WB.items():
            if k.lower() == name.lower():
                return v
    return t


def comp(node, sh, fr, fc, ref=False):
    k = node[0]
    if k == 'num' or k == 'str' or k == 'bool' or k == 'err':
        v = node[1]
        return lambda: v
    if k == 'missing':
        return lambda: MISSING
    if k == 'name':
        return lambda: NAME
    if k == 'ref':
        _, sname, r1, c1, r2, c2 = node
        tsh = resolve_sheet(sname, sh)
        if tsh is None:
            return lambda: REF
        if ref:
            return lambda: Ref(tsh, r1, c1, r2, c2, fr, fc)
        if r1 == r2 and c1 == c2:
            return lambda: getv(tsh, r1, c1)
        if c1 == c2 and r1 <= fr <= r2:
            return lambda: getv(tsh, fr, c1)
        if r1 == r2 and c1 <= fc <= c2:
            return lambda: getv(tsh, r1, fc)
        return lambda: VALUE
    if k == 'neg':
        a = comp(node[1], sh, fr, fc)
        return lambda: _neg(a())
    if k == 'pct':
        a = comp(node[1], sh, fr, fc)
        return lambda: _pct(a())
    if k == 'bin':
        fn = BINFN[node[1]]
        a = comp(node[2], sh, fr, fc)
        b = comp(node[3], sh, fr, fc)
        return lambda: fn(a(), b())
    if k == 'func':
        name = node[1]
        impl = FUNCS.get(name)
        if impl is None:
            return lambda: NAME
        argf = [comp(x, sh, fr, fc, True) for x in node[2]]
        n = len(argf)
        if n == 0:
            return lambda: impl()
        if n == 1:
            a0 = argf[0]
            return lambda: impl(a0())
        if n == 2:
            a0, a1 = argf
            return lambda: impl(a0(), a1())
        if n == 3:
            a0, a1, a2 = argf
            return lambda: impl(a0(), a1(), a2())
        return lambda: impl(*[f() for f in argf])
    return lambda: NAME


def load_workbook(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    wbx = ET.fromstring(z.read('xl/workbook.xml'))
    rels = ET.fromstring(z.read('xl/_rels/workbook.xml.rels'))
    relmap = {}
    for r in rels:
        relmap[r.get('Id')] = r.get('Target')
    sst = []
    if 'xl/sharedStrings.xml' in names:
        root = ET.fromstring(z.read('xl/sharedStrings.xml'))
        for si in root.iter(NS + 'si'):
            sst.append(''.join((t.text or '') for t in si.iter(NS + 't')))
    sheets = []
    for s in wbx.find(NS + 'sheets'):
        name = s.get('name')
        target = relmap[s.get(RNS + 'id')]
        if target.startswith('/'):
            p = target.lstrip('/')
        else:
            p = posixpath.normpath(posixpath.join('xl', target))
        root = ET.fromstring(z.read(p))
        sh = Sheet(name)
        shared = {}
        for c in root.iter(NS + 'c'):
            ref = c.get('r')
            t = c.get('t', 'n')
            m = CELLRE.fullmatch(ref)
            col = col_num(m.group(1))
            row = int(m.group(2))
            f = c.find(NS + 'f')
            v = c.find(NS + 'v')
            if f is not None:
                ftext = f.text
                if f.get('t') == 'shared':
                    si = f.get('si')
                    if ftext:
                        shared[si] = (ftext, row, col)
                    elif si in shared:
                        mt, mr, mc = shared[si]
                        ftext = shift_formula(mt, row - mr, col - mc)
                sh.cells[(row, col)] = FCell(ftext or '')
                continue
            if v is None or v.text is None:
                if t == 'inlineStr':
                    is_ = c.find(NS + 'is')
                    if is_ is not None:
                        sh.cells[(row, col)] = ''.join((x.text or '') for x in is_.iter(NS + 't'))
                continue
            vt = v.text
            if t == 's':
                sh.cells[(row, col)] = sst[int(vt)]
            elif t == 'inlineStr':
                sh.cells[(row, col)] = ''.join((x.text or '') for x in c.iter(NS + 't'))
            elif t == 'str':
                sh.cells[(row, col)] = vt
            elif t == 'b':
                sh.cells[(row, col)] = (vt.strip() == '1')
            elif t == 'e':
                sh.cells[(row, col)] = ERRS.get(vt, VALUE)
            else:
                sh.cells[(row, col)] = float(vt)
        sheets.append(sh)
        WB[name] = sh
    return sheets


def shift_formula(text, dr, dc):
    out = []
    for m in TOKRE.finditer(text):
        k = m.lastgroup
        t = m.group(k)
        if k == 'ref':
            pre = ''
            body = t
            if '!' in t:
                i = t.rfind('!')
                pre = t[:i + 1]
                body = t[i + 1:]
            segs = []
            for seg in body.split(':'):
                mm = re.fullmatch(r'(\$?)([A-Za-z]+)(\$?)([0-9]+)', seg)
                if not mm:
                    segs.append(seg)
                    continue
                ca, cl, ra, rn = mm.groups()
                cn = col_num(cl)
                rr = int(rn)
                if not ca:
                    cn += dc
                if not ra:
                    rr += dr
                segs.append('%s%s%s%d' % (ca, col_letters(cn), ra, rr))
            t = pre + ':'.join(segs)
        out.append(t)
    return ''.join(out)


def compile_sheets(sheets):
    for sh in sheets:
        for key, cell in list(sh.cells.items()):
            if cell.__class__ is not FCell:
                continue
            r, c = key
            try:
                ast = parse_formula(cell.text)
                cell.fn = comp(ast, sh, r, c, False)
            except Exception:
                cell.fn = (lambda: NAME)


def run(inp, outp):
    sheets = load_workbook(inp)
    compile_sheets(sheets)
    result = {}
    for sh in sheets:
        res = {}
        fcells = [(k, v) for k, v in sh.cells.items() if v.__class__ is FCell]
        fcells.sort(key=lambda kv: (kv[0][0], kv[0][1]))
        for (r, c), cell in fcells:
            getv(sh, r, c)
        for (r, c), cell in fcells:
            v = cell.val
            ref = '%s%d' % (col_letters(c), r)
            if v.__class__ is bool:
                res[ref] = {'t': 'b', 'v': v}
            elif v.__class__ is float:
                if v != v or v in (float('inf'), float('-inf')):
                    res[ref] = {'t': 'e', 'v': '#NUM!'}
                else:
                    res[ref] = {'t': 'n', 'v': v}
            elif v.__class__ is str:
                res[ref] = {'t': 's', 'v': v}
            elif v.__class__ is XErr:
                res[ref] = {'t': 'e', 'v': v.code}
            else:
                res[ref] = {'t': 'n', 'v': 0.0}
        result[sh.name] = res
    with open(outp, 'w') as fh:
        json.dump(result, fh, separators=(',', ':'))


def main(argv):
    inp = outp = None
    i = 0
    while i < len(argv):
        if argv[i] == '--in' and i + 1 < len(argv):
            inp = argv[i + 1]
            i += 2
        elif argv[i] == '--out' and i + 1 < len(argv):
            outp = argv[i + 1]
            i += 2
        else:
            i += 1
    if inp is None or outp is None:
        sys.stderr.write('usage: calc eval --in book.xlsx --out values.json\n')
        return 2
    err = []

    def target():
        try:
            run(inp, outp)
        except BaseException as e:
            import traceback
            traceback.print_exc()
            err.append(e)

    sys.setrecursionlimit(1000000)
    try:
        threading.stack_size(512 * 1024 * 1024)
        t = threading.Thread(target=target)
        t.start()
        t.join()
    except (RuntimeError, ValueError, MemoryError):
        sys.setrecursionlimit(20000)
        target()
    return 1 if err else 0



# ===== performance overrides (cached columns, byte masks, Pratt parser) =====
import itertools as _it

_old_sumif = f_sumif
_old_sumifs = f_sumifs
_old_averageifs = f_averageifs
_old_averageif = f_averageif

_RV_CACHE = {}


def rvals_c(x):
    k = (id(x.sh), x.r1, x.c1, x.r2, x.c2)
    v = _RV_CACHE.get(k)
    if v is None:
        v = rvals(x)
        _RV_CACHE[k] = v
    return v


def _rk(x):
    return (id(x.sh), x.r1, x.c1, x.r2, x.c2)


_MASK_CACHE = {}
_NUM_CACHE = {}


def mask_for(rng, crit):
    k = (_rk(rng), crit.__class__, crit)
    m = _MASK_CACHE.get(k)
    if m is None:
        pred = crit_pred(crit)
        m = bytes([1 if pred(v) else 0 for v in rvals_c(rng)])
        _MASK_CACHE[k] = m
    return m


def numinfo(rng):
    k = _rk(rng)
    r = _NUM_CACHE.get(k)
    if r is None:
        vals = rvals_c(rng)
        has_err = False
        nums = []
        isn = bytearray(len(vals))
        for i, x in enumerate(vals):
            c = x.__class__
            if c is float:
                nums.append(x)
                isn[i] = 1
            elif c is bool:
                nums.append(1.0 if x else 0.0)
                isn[i] = 1
            else:
                nums.append(0.0)
                if c is XErr:
                    has_err = True
        r = (nums, bytes(isn), has_err)
        _NUM_CACHE[k] = r
    return r


def _and_masks(masks, n):
    acc = None
    for m in masks:
        x = int.from_bytes(m, 'little')
        acc = x if acc is None else (acc & x)
    if acc is None:
        return bytes([1]) * n
    return acc.to_bytes(n, 'little')


def _size(x):
    return (x.r2 - x.r1 + 1) * (x.c2 - x.c1 + 1)


def _sumrange_ref(sumr, rng):
    h = rng.r2 - rng.r1 + 1
    w = rng.c2 - rng.c1 + 1
    return Ref(sumr.sh, sumr.r1, sumr.c1, sumr.r1 + h - 1, sumr.c1 + w - 1, sumr.fr, sumr.fc)


def f_countif(rng=MISSING, crit=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    return float(mask_for(rng, c).count(1))


def f_countifs(*args):
    pairs = _pairs(args)
    if pairs is None or not pairs:
        return VALUE
    n = None
    masks = []
    for rng, cr in pairs:
        if cr.__class__ is XErr:
            return cr
        size = _size(rng)
        if n is None:
            n = size
        elif n != size:
            return VALUE
        masks.append(mask_for(rng, cr))
    return float(_and_masks(masks, n).count(1))


def f_sumif(rng=MISSING, crit=MISSING, sumr=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    sr = rng if sumr.__class__ is not Ref else _sumrange_ref(sumr, rng)
    nums, isn, has_err = numinfo(sr)
    if has_err:
        return _old_sumif(rng, crit, sumr)
    m = _and_masks([mask_for(rng, c), isn], _size(rng))
    return math.fsum(_it.compress(nums, m))


def f_averageif(rng=MISSING, crit=MISSING, avg=MISSING, *_):
    if rng.__class__ is not Ref:
        return VALUE
    c = sc_arg(crit)
    if c.__class__ is XErr:
        return c
    sr = rng if avg.__class__ is not Ref else _sumrange_ref(avg, rng)
    nums, isn, has_err = numinfo(sr)
    if has_err:
        return _old_averageif(rng, crit, avg)
    m = _and_masks([mask_for(rng, c), isn], _size(rng))
    cnt = m.count(1)
    if cnt == 0:
        return DIV0
    return math.fsum(_it.compress(nums, m)) / cnt


def f_sumifs(sumr=MISSING, *args):
    if sumr.__class__ is not Ref:
        return VALUE
    pairs = _pairs(args)
    if pairs is None or not pairs:
        return VALUE
    n = _size(sumr)
    masks = []
    for rng, cr in pairs:
        if cr.__class__ is XErr:
            return cr
        if _size(rng) != n:
            return VALUE
        masks.append(mask_for(rng, cr))
    nums, isn, has_err = numinfo(sumr)
    if has_err:
        return _old_sumifs(sumr, *args)
    masks.append(isn)
    m = _and_masks(masks, n)
    return math.fsum(_it.compress(nums, m))


def f_averageifs(avg=MISSING, *args):
    if avg.__class__ is not Ref:
        return VALUE
    pairs = _pairs(args)
    if pairs is None or not pairs:
        return VALUE
    n = _size(avg)
    masks = []
    for rng, cr in pairs:
        if cr.__class__ is XErr:
            return cr
        if _size(rng) != n:
            return VALUE
        masks.append(mask_for(rng, cr))
    nums, isn, has_err = numinfo(avg)
    if has_err:
        return _old_averageifs(avg, *args)
    masks.append(isn)
    m = _and_masks(masks, n)
    cnt = m.count(1)
    if cnt == 0:
        return DIV0
    return math.fsum(_it.compress(nums, m)) / cnt


def f_vlookup(v=MISSING, t=MISSING, idx=MISSING, approx=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if t.__class__ is not Ref:
        return VALUE
    ci = num_of(sc_arg(idx))
    if ci.__class__ is XErr:
        return ci
    ci = int(math.trunc(ci))
    ncols = t.c2 - t.c1 + 1
    if ci < 1:
        return VALUE
    if ci > ncols:
        return VALUE
    ap = True
    if approx is not MISSING:
        ap = to_bool(sc_arg(approx))
        if ap.__class__ is XErr:
            return ap
    sh = t.sh
    keys = rvals_c(Ref(sh, t.r1, t.c1, t.r2, t.c1, 0, 0))
    if not ap:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(sh, t.r1, t.c1, t.r2, t.c1, keys)
        i = exact_find(keys, key, idxd)
    else:
        i = bsearch_le(keys, key)
    if i < 0:
        return NA
    return getv(sh, t.r1 + i, t.c1 + ci - 1)


def f_hlookup(v=MISSING, t=MISSING, idx=MISSING, approx=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if t.__class__ is not Ref:
        return VALUE
    ri = num_of(sc_arg(idx))
    if ri.__class__ is XErr:
        return ri
    ri = int(math.trunc(ri))
    nrows = t.r2 - t.r1 + 1
    if ri < 1:
        return VALUE
    if ri > nrows:
        return VALUE
    ap = True
    if approx is not MISSING:
        ap = to_bool(sc_arg(approx))
        if ap.__class__ is XErr:
            return ap
    sh = t.sh
    keys = rvals_c(Ref(sh, t.r1, t.c1, t.r1, t.c2, 0, 0))
    if not ap:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(sh, t.r1, t.c1, t.r1, t.c2, keys)
        i = exact_find(keys, key, idxd)
    else:
        i = bsearch_le(keys, key)
    if i < 0:
        return NA
    return getv(sh, t.r1 + ri - 1, t.c1 + i)


def f_match(v=MISSING, rng=MISSING, typ=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if rng.__class__ is not Ref:
        return NA
    mt = 1
    if typ is not MISSING:
        mv = num_of(sc_arg(typ))
        if mv.__class__ is XErr:
            return mv
        mt = int(math.trunc(mv))
    if mt not in (-1, 0, 1):
        return VALUE
    vals = rvals_c(rng)
    if mt == 0:
        idxd = None
        if key.__class__ is not str or not has_wild(key):
            idxd = exact_index(rng.sh, rng.r1, rng.c1, rng.r2, rng.c2, vals)
        i = exact_find(vals, key, idxd)
    elif mt > 0:
        i = bsearch_le(vals, key)
    else:
        i = bsearch_ge_desc(vals, key)
    if i < 0:
        return NA
    return float(i + 1)


def f_xlookup(v=MISSING, look=MISSING, ret=MISSING, nf=MISSING, mm=MISSING, sm=MISSING, *_):
    key = sc_arg(v)
    if key.__class__ is XErr:
        return key
    if look.__class__ is not Ref or ret.__class__ is not Ref:
        return VALUE
    match_mode = 0
    if mm is not MISSING:
        x = num_of(sc_arg(mm))
        if x.__class__ is XErr:
            return x
        match_mode = int(math.trunc(x))
    search_mode = 1
    if sm is not MISSING:
        x = num_of(sc_arg(sm))
        if x.__class__ is XErr:
            return x
        search_mode = int(math.trunc(x))
    vals = rvals_c(look)
    rvs = rvals_c(ret)
    n = len(vals)
    if len(rvs) != n:
        return VALUE
    order = range(n) if search_mode >= 0 else range(n - 1, -1, -1)
    found = -1
    if match_mode == 0 or match_mode == 2:
        if (key.__class__ is not str or not has_wild(key)) and match_mode == 0 and search_mode == 1:
            idxd = exact_index(look.sh, look.r1, look.c1, look.r2, look.c2, vals)
            found = exact_find(vals, key, idxd)
        else:
            for i in order:
                if match_exact(key, vals[i]):
                    found = i
                    break
    else:
        for i in order:
            if match_exact(key, vals[i]):
                found = i
                break
        if found < 0 and key.__class__ is float:
            best = -1
            bestv = None
            for i in order:
                x = vals[i]
                if x.__class__ is not float:
                    continue
                if match_mode == -1 and x < key:
                    if bestv is None or x > bestv:
                        bestv = x
                        best = i
                elif match_mode == 1 and x > key:
                    if bestv is None or x < bestv:
                        bestv = x
                        best = i
            found = best
    if found < 0:
        return sc_arg(nf) if nf is not MISSING else NA
    return rvs[found]


class Parser3(Parser):
    def expr(self):
        return self.parse(1)

    def parse(self, minp):
        left = self.unary3()
        while True:
            k, tx = self.peek()
            if k != 'op':
                break
            p = _BINPREC.get(tx)
            if p is None or p < minp:
                break
            self.i += 1
            right = self.parse(p + 1)
            left = ('bin', tx, left, right)
        return left

    def unary3(self):
        if self.isop('-'):
            self.i += 1
            return ('neg', self.unary3())
        if self.isop('+'):
            self.i += 1
            return self.unary3()
        return self.pct()


_BINPREC = {'=': 1, '<>': 1, '<': 1, '>': 1, '<=': 1, '>=': 1, '&': 2,
            '+': 3, '-': 3, '*': 4, '/': 4, '^': 5}


def parse_formula(text):
    toks = tokenize(text)
    p = Parser3(toks)
    e = p.expr()
    if p.i != len(toks):
        raise ParseErr(text)
    return e


FUNCS.update({
    'COUNTIF': f_countif, 'COUNTIFS': f_countifs, 'SUMIF': f_sumif, 'AVERAGEIF': f_averageif,
    'SUMIFS': f_sumifs, 'AVERAGEIFS': f_averageifs, 'VLOOKUP': f_vlookup, 'HLOOKUP': f_hlookup,
    'MATCH': f_match, 'XLOOKUP': f_xlookup,
})



# ===== error-semantics overrides and _xlfn prefix rules =====
_NEEDPFX = {'IFNA', 'IFS', 'SWITCH', 'XLOOKUP', 'XOR', 'CONCAT', 'TEXTJOIN', 'DAYS'}
_old_primary = Parser.primary


def _primary3(self):
    k, t = self.peek()
    if k == 'func':
        raw = t[:-1]
        if raw.upper().startswith('_XLFN.'):
            if raw[6:].upper() not in _NEEDPFX:
                node = _old_primary(self)
                return ('func', '#NAME', node[2])
        elif raw.upper() in _NEEDPFX:
            node = _old_primary(self)
            return ('func', '#NAME', node[2])
    return _old_primary(self)


Parser3.primary = _primary3



# ===== VALUE with dates and booleans; text date parsing =====
_DATE_FORMATS = ['%d-%b-%y', '%d-%b-%Y', '%Y-%m-%d', '%m/%d/%Y', '%m/%d/%y', '%d-%B-%Y', '%d %b %Y', '%b %d, %Y', '%B %d, %Y']


def _parse_text_date(s):
    t = s.strip()
    for f in _DATE_FORMATS:
        try:
            d = datetime.datetime.strptime(t, f).date()
            return float((d - EPOCH).days)
        except ValueError:
            pass
    return None


def f_value(x=MISSING, *_):
    v = sc_arg(x)
    c = v.__class__
    if c is float:
        return v
    if c is bool:
        return 1.0 if v else 0.0
    if c is XErr:
        return v
    if v is None:
        return 0.0
    if c is str:
        r = parse_num_str(v)
        if r is not None:
            return r
        d = _parse_text_date(v)
        if d is not None:
            return d
        return VALUE
    return VALUE


FUNCS['VALUE'] = f_value



# ===== LibreOffice-style variance (sum / sum of squares) =====
def _var_lo(args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    n = len(v)
    if n < 2:
        return DIV0
    s = 0.0
    s2 = 0.0
    for x in v:
        s += x
        s2 += x * x
    return (s2 - s * s / n) / (n - 1)


def f_var_lo(*args):
    return _var_lo(args)


def f_stdev_lo(*args):
    r = _var_lo(args)
    if r.__class__ is XErr:
        return r
    return math.sqrt(r) if r >= 0 else 0.0


FUNCS['VAR'] = f_var_lo
FUNCS['STDEV'] = f_stdev_lo



# ===== error precedence (priority) variant =====
_PRI = {'#VALUE!': 5, '#DIV/0!': 4, '#N/A': 3, '#NUM!': 2, '#REF!': 1, '#NAME?': 0, '#NULL!': 0}


def _pick_err(x, y):
    if x.__class__ is XErr and y.__class__ is XErr:
        return x if _PRI.get(x.code, 0) >= _PRI.get(y.code, 0) else y
    return x if x.__class__ is XErr else y


def binop_num(op, x, y):
    a = num_of(x)
    b = num_of(y)
    ae = a.__class__ is XErr
    be = b.__class__ is XErr
    if ae or be:
        if op == '/' and ae and not be and b == 0:
            return _pick_err(a, DIV0)
        if ae and be:
            return _pick_err(a, b)
        return a if ae else b
    if op == '+':
        r = a + b
    elif op == '-':
        r = a - b
    elif op == '*':
        r = a * b
    elif op == '/':
        if b == 0:
            return DIV0
        r = a / b
    else:
        return power(a, b)
    if not (-MAXF < r < MAXF):
        return NUM
    return r


def _cmp(op, x, y):
    if x.__class__ is XErr or y.__class__ is XErr:
        if x.__class__ is XErr and y.__class__ is XErr:
            return _pick_err(x, y)
        return x if x.__class__ is XErr else y
    c = cmp_vals(x, y)
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


def _concat(x, y):
    if x.__class__ is str and y.__class__ is str:
        return x + y
    if x.__class__ is XErr or y.__class__ is XErr:
        return _pick_err(x, y) if (x.__class__ is XErr and y.__class__ is XErr) else (x if x.__class__ is XErr else y)
    a = to_text(x)
    b = to_text(y)
    if a.__class__ is XErr or b.__class__ is XErr:
        return _pick_err(a, b) if (a.__class__ is XErr and b.__class__ is XErr) else (a if a.__class__ is XErr else b)
    return a + b


BINFN.update({
    '&': _concat, '=': lambda x, y: _cmp('=', x, y), '<>': lambda x, y: _cmp('<>', x, y),
    '<': lambda x, y: _cmp('<', x, y), '>': lambda x, y: _cmp('>', x, y),
    '<=': lambda x, y: _cmp('<=', x, y), '>=': lambda x, y: _cmp('>=', x, y),
})



# ===== division keeps divisor-first rules; other operators use priority =====
_binop_prio = binop_num


def binop_num(op, x, y):
    if op == '/':
        if y.__class__ is XErr:
            return y
        b = num_of(y)
        if b.__class__ is XErr:
            return b
        if x.__class__ is XErr:
            return DIV0 if b == 0 else x
        a = num_of(x)
        if a.__class__ is XErr:
            return a
        if b == 0:
            return DIV0
        r = a / b
        if not (-MAXF < r < MAXF):
            return NUM
        return r
    return _binop_prio(op, x, y)



# ===== additive/multiplicative operators: left operand error first =====
_binop_prev = binop_num


def binop_num(op, x, y):
    if op in ('+', '-', '*'):
        if x.__class__ is XErr:
            return x
        if y.__class__ is XErr:
            return y
        a = num_of(x)
        if a.__class__ is XErr:
            return a
        b = num_of(y)
        if b.__class__ is XErr:
            return b
        if op == '+':
            r = a + b
        elif op == '-':
            r = a - b
        else:
            r = a * b
        if not (-MAXF < r < MAXF):
            return NUM
        return r
    return _binop_prev(op, x, y)



# ===== MIN/MAX: boolean-only inputs give a boolean result =====
def _minmax_collect(args):
    nums = []
    bools = []
    for a in args:
        if a is MISSING:
            continue
        if a.__class__ is Ref:
            for v in rvals(a):
                c = v.__class__
                if c is float:
                    nums.append(v)
                elif c is bool:
                    bools.append(v)
                elif c is XErr:
                    return v
            continue
        c = a.__class__
        if c is float:
            nums.append(a)
        elif c is bool:
            bools.append(a)
        elif c is XErr:
            return a
        elif c is str:
            return VALUE
    return nums, bools


def f_min(*args):
    r = _minmax_collect(args)
    if r.__class__ is XErr:
        return r
    nums, bools = r
    if not nums:
        if bools:
            return all(bools)
        return 0.0
    return min(nums + [1.0 if b else 0.0 for b in bools])


def f_max(*args):
    r = _minmax_collect(args)
    if r.__class__ is XErr:
        return r
    nums, bools = r
    if not nums:
        if bools:
            return any(bools)
        return 0.0
    return max(nums + [1.0 if b else 0.0 for b in bools])


FUNCS['MIN'] = f_min
FUNCS['MAX'] = f_max



# ===== SUMPRODUCT with compensated summation =====
def f_sumproduct_k(*args):
    arrs = []
    shape = None
    for a in args:
        if a.__class__ is Ref:
            sh_ = (a.r2 - a.r1 + 1, a.c2 - a.c1 + 1)
            if shape is None:
                shape = sh_
            elif shape != sh_:
                return VALUE
            arrs.append(rvals(a))
        else:
            if a.__class__ is XErr:
                return a
            if shape is None:
                shape = (1, 1)
            elif shape != (1, 1):
                return VALUE
            arrs.append([a])
    if not arrs:
        return VALUE
    n = len(arrs[0])
    prods = []
    for i in range(n):
        p_ = 1.0
        bad = False
        for arr in arrs:
            v = arr[i]
            if v.__class__ is XErr:
                return v
            if v.__class__ is bool:
                v = 1.0 if v else 0.0
            if v.__class__ is not float:
                bad = True
            else:
                p_ *= v
        if not bad:
            prods.append(p_)
    return math.fsum(prods)


FUNCS['SUMPRODUCT'] = f_sumproduct_k



# ===== refinements: MOD zero divisor first, 2-D MATCH is #VALUE!, booleans match numeric criteria =====
def f_mod(n=MISSING, d=MISSING, *_):
    b = num_of(sc_arg(d))
    if b.__class__ is XErr:
        return b
    a = num_of(sc_arg(n))
    if b == 0:
        return DIV0
    if a.__class__ is XErr:
        return a
    return a - b * math.floor(a / b)


_old_match_fn = f_match


def f_match(v=MISSING, rng=MISSING, typ=MISSING, *_):
    if rng.__class__ is Ref and rng.r1 != rng.r2 and rng.c1 != rng.c2:
        return VALUE
    return _old_match_fn(v, rng, typ)


FUNCS['MOD'] = f_mod
FUNCS['MATCH'] = f_match


def _num_pred_bool(crit_val, op):
    n = crit_val
    if op == '=':
        return lambda v: (v.__class__ is float or v.__class__ is bool) and aeq(float(v), n)
    if op == '<>':
        return lambda v: not ((v.__class__ is float or v.__class__ is bool) and aeq(float(v), n))
    return None


_old_crit_pred = crit_pred


def crit_pred(crit):
    if crit.__class__ is bool:
        return _num_pred_bool(1.0 if crit else 0.0, '=')
    return _old_crit_pred(crit)



# ===== VAR / STDEV: two-pass sequential summation =====
def _var_lo(args):
    v = nums_from(args)
    if v.__class__ is XErr:
        return v
    n = len(v)
    if n < 2:
        return DIV0
    s = 0.0
    for x in v:
        s += x
    m = s / n
    t = 0.0
    for x in v:
        d = x - m
        t += d * d
    return t / (n - 1)



if __name__ == '__main__':
    args = sys.argv[1:]
    if args and args[0] == 'eval':
        args = args[1:]
    sys.exit(main(args))
