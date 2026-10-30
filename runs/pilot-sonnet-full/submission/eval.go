package main

import (
	"math"
	"strconv"
	"strings"
	"unicode"
)

type Ctx struct {
	wb   *Wb
	sh   *Sheet
	r, c int
	fe   Val
	ll   bool
	reeval int
	lastFirstBool bool
	fullRange bool
	lastRef bool
	flags []bool
}

func (cx *Ctx) fin(v Val) Val {
	if v.k == KErr {
		if cx.fe.k == KErr {
			return cx.fe
		}
		cx.fe = v
		cx.ll = false
		return v
	}
	cx.ll = v.k == KBool
	return v
}

type Fn func(cx *Ctx, a []*Node) Val

func (wb *Wb) calcAll() {
	for _, f := range wb.formulas {
		wb.get(f.sh, f.r, f.c)
	}
}

func (wb *Wb) get(sh *Sheet, r, c int) Val {
	if r < 0 || c < 0 || r >= sh.R || c >= sh.C {
		return Val{}
	}
	ix := r*sh.C + c
	if sh.st[ix] == 2 {
		return sh.v[ix]
	}
	fi := sh.fi[ix]
	if fi < 0 {
		return sh.v[ix]
	}
	if sh.st[ix] == 1 {
		return mkErr(eVal)
	}
	sh.st[ix] = 1
	f := wb.formulas[fi]
	cx := Ctx{wb: wb, sh: sh, r: r, c: c}
	v := cx.safeEval(f.root)
	if cx.fe.k == KErr {
		v = cx.fe
	}
	if v.k == KEmpty {
		v = mkNum(0)
	}
	sh.v[ix] = v
	sh.st[ix] = 2
	return v
}

func approxEqual(a, b float64) bool {
	if a == b {
		return true
	}
	x := a - b
	if a < 0 {
		a = -a
	}
	if x < 0 {
		x = -x
	}
	return x < a*(1.0/(16777216.0*1048576.0))
}

func approxAdd(a, b float64) float64 {
	if ((a < 0 && b > 0) || (b < 0 && a > 0)) && approxEqual(a, -b) {
		return 0
	}
	return a + b
}

func approxSub(a, b float64) float64 {
	if ((a < 0 && b < 0) || (a > 0 && b > 0)) && approxEqual(a, b) {
		return 0
	}
	return a - b
}

func parseNum(s string) (float64, bool) {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0, false
	}
	c := s[0]
	if !(c >= '0' && c <= '9') && c != '-' && c != '+' && c != '.' {
		return 0, false
	}
	for i := 0; i < len(s); i++ {
		ch := s[i]
		if !((ch >= '0' && ch <= '9') || ch == '.' || ch == '-' || ch == '+' || ch == 'e' || ch == 'E') {
			// percent
			if ch == '%' && i == len(s)-1 {
				f, ok := parseNum(s[:i])
				return f / 100, ok
			}
			return 0, false
		}
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return 0, false
	}
	return f, true
}

func parseThousands(s string) (float64, bool) {
	s = strings.TrimSpace(s)
	i := strings.IndexByte(s, ',')
	if i < 1 {
		return 0, false
	}
	ip := s
	fp := ""
	if d := strings.IndexByte(s, '.'); d >= 0 {
		ip, fp = s[:d], s[d:]
	}
	parts := strings.Split(ip, ",")
	neg := false
	if len(parts[0]) > 0 && (parts[0][0] == '-' || parts[0][0] == '+') {
		neg = parts[0][0] == '-'
		parts[0] = parts[0][1:]
	}
	if len(parts[0]) < 1 || len(parts[0]) > 3 {
		return 0, false
	}
	for k, p := range parts {
		if (k > 0 && len(p) != 3) || strings.Trim(p, "0123456789") != "" {
			return 0, false
		}
	}
	f, err := strconv.ParseFloat(strings.Join(parts, "")+fp, 64)
	if err != nil {
		return 0, false
	}
	if neg {
		f = -f
	}
	return f, true
}

func parseISODate(s string) (float64, bool) {
	s = strings.TrimSpace(s)
	if len(s) == 10 && s[4] == '-' && s[7] == '-' {
		y, e1 := strconv.Atoi(s[0:4])
		m, e2 := strconv.Atoi(s[5:7])
		d, e3 := strconv.Atoi(s[8:10])
		if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
			return float64(dateSerial(y, m, d)), true
		}
	}
	return 0, false
}

// toNum coerces a value to a number (arithmetic context).
func toNum(v Val) (float64, Val) {
	switch v.k {
	case KNum, KBool:
		return v.n, Val{}
	case KEmpty:
		return 0, Val{}
	case KStr:
		if f, ok := parseNum(v.s); ok {
			return f, Val{}
		}
		if f, ok := parseISODate(v.s); ok {
			return f, Val{}
		}
		if f, ok := parseThousands(v.s); ok {
			return f, Val{}
		}
		if f, ok := parseDateStr(v.s); ok {
			return f, Val{}
		}
		if strings.EqualFold(v.s, "true") {
			return 1, Val{}
		}
		if strings.EqualFold(v.s, "false") {
			return 0, Val{}
		}
		return 0, mkErr(eVal)
	}
	return 0, v
}

func fmtNum(f float64) string {
	if f == 0 {
		return "0"
	}
	if f == math.Trunc(f) && math.Abs(f) < 1e16 {
		return strconv.FormatInt(int64(f), 10)
	}
	neg := f < 0
	ax := math.Abs(f)
	exp := int(math.Floor(math.Log10(ax)))
	var r float64
	for tries := 0; tries < 3; tries++ {
		k := 14 - exp
		var y float64
		if k >= 0 {
			y = ax * math.Pow10(k)
		} else {
			y = ax / math.Pow10(-k)
		}
		r = math.Floor(y + 0.5)
		if r >= 1e15 {
			exp++
			continue
		}
		if r < 1e14 {
			exp--
			continue
		}
		break
	}
	digits := strconv.FormatInt(int64(r), 10)
	digits = strings.TrimRight(digits, "0")
	if digits == "" {
		digits = "0"
	}
	var out string
	if exp >= 16 {
		out = digits[:1]
		if len(digits) > 1 {
			out += "." + digits[1:]
		}
		sign := "+"
		if exp < 0 {
			sign = "-"
			exp = -exp
		}
		es := strconv.Itoa(exp)
		for len(es) < 3 {
			es = "0" + es
		}
		out += "E" + sign + es
	} else if exp >= 0 {
		if len(digits) <= exp+1 {
			out = digits + strings.Repeat("0", exp+1-len(digits))
		} else {
			out = digits[:exp+1] + "." + digits[exp+1:]
		}
	} else {
		out = "0." + strings.Repeat("0", -exp-1) + digits
	}
	if neg {
		out = "-" + out
	}
	return out
}

func toStr(v Val) (string, Val) {
	switch v.k {
	case KStr:
		return v.s, Val{}
	case KNum:
		return fmtNum(v.n), Val{}
	case KBool:
		return fmtNum(v.n), Val{}
	case KEmpty:
		return "", Val{}
	}
	return "", v
}

func lowerStr(s string) string {
	ascii := true
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			ascii = false
			break
		}
	}
	if ascii {
		return strings.ToLower(s)
	}
	return strings.Map(unicode.ToLower, s)
}

func cmpVals(a, b Val) int {
	if a.k == KEmpty {
		if b.k == KStr {
			a = mkStr("")
		} else {
			a = mkNum(0)
		}
	}
	if b.k == KEmpty {
		if a.k == KStr {
			b = mkStr("")
		} else {
			b = mkNum(0)
		}
	}
	if a.k == KStr && b.k == KStr {
		x, y := lowerStr(a.s), lowerStr(b.s)
		if x < y {
			return -1
		} else if x > y {
			return 1
		}
		return 0
	}
	if a.k == KStr {
		return 1
	}
	if b.k == KStr {
		return -1
	}
	if approxEqual(a.n, b.n) {
		return 0
	}
	if a.n < b.n {
		return -1
	}
	return 1
}

func (cx *Ctx) eval(n *Node) Val {
	switch n.op {
	case NRef:
		cx.lastRef = true
		return cx.wb.get(n.sh, n.r1, n.c1)
	case NConst:
		cx.lastRef = false
		return n.val
	case NCall:
		if n.pass {
			return cx.eval0(n)
		}
		v := cx.eval0(n)
		cx.lastRef = false
		return v
	}
	v := cx.eval0(n)
	cx.lastRef = false
	return v
}

func (cx *Ctx) eval0(n *Node) Val {
	switch n.op {
	case NConst:
		return n.val
	case NRef:
		return cx.wb.get(n.sh, n.r1, n.c1)
	case NRange:
		// implicit intersection
		if n.r1 == n.r2 && n.c1 == n.c2 {
			return cx.wb.get(n.sh, n.r1, n.c1)
		}
		if n.c1 == n.c2 && cx.r >= n.r1 && cx.r <= n.r2 && n.sh != nil {
			return cx.wb.get(n.sh, cx.r, n.c1)
		}
		if n.r1 == n.r2 && cx.c >= n.c1 && cx.c <= n.c2 {
			return cx.wb.get(n.sh, n.r1, cx.c)
		}
		return mkErr(eVal)
	case NNeg:
		v := cx.eval(n.args[0])
		f, e := toNum(v)
		if e.k == KErr {
			return cx.fin(e)
		}
		return cx.fin(mkNum(-f))
	case NPos:
		return cx.eval(n.args[0])
	case NPct:
		v := cx.eval(n.args[0])
		f, e := toNum(v)
		if e.k == KErr {
			return cx.fin(e)
		}
		return cx.fin(mkNum(f / 100))
	case NCall:
		if n.pass {
			return n.fn(cx, n.args)
		}
		v := n.fn(cx, n.args)
		if v.k == KErr && cx.fe.k != KErr && len(n.args) > 1 && cx.reeval < 2000 {
			for _, a := range n.args {
				if a.op != NConst && a.op != NRef && a.op != NRange {
					cx.reeval++
					cx.eval(a)
				}
			}
		}
		if v.k == KErr && cx.fe.k == KErr {
			return cx.fe
		}
		if v.k == KErr && len(n.args) > 1 && cx.reeval < 2000 {
			for i := len(n.args) - 1; i >= 0; i-- {
				a := n.args[i]
				if a.op == NConst || a.op == NRange {
					continue
				}
				cx.reeval++
				av := cx.eval(a)
				if av.k == KErr {
					v = av
					break
				}
			}
		}
		if n.naLatent && v.k == KErr && int(v.n) == eNA && cx.fe.k != KErr {
			return v
		}
		if v.k == KErr && int(v.n) == eNA && cx.fe.k != KErr && n.sibIfs {
			return v
		}
		return cx.fin(v)
	case NBin:
		return cx.fin(cx.evalBin(n))
	}
	return mkErr(eVal)
}

func checkNum(f float64) Val {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return mkErr(eNum)
	}
	return mkNum(f)
}

func (cx *Ctx) evalBin(n *Node) Val {
	l := cx.eval(n.args[0])
	r := cx.eval(n.args[1])
	switch n.bop {
	case '+', '-', '*', '/', '^':
		b, e2 := toNum(r)
		if e2.k == KErr {
			return e2
		}
		a, e := toNum(l)
		if e.k == KErr {
			return e
		}
		switch n.bop {
		case '+':
			return checkNum(approxAdd(a, b))
		case '-':
			return checkNum(approxSub(a, b))
		case '*':
			return checkNum(a * b)
		case '/':
			if b == 0 {
				return mkErr(eDiv)
			}
			return checkNum(a / b)
		case '^':
			return checkNum(math.Pow(a, b))
		}
	case '&':
		if r.k == KErr {
			return r
		}
		if l.k == KErr {
			return l
		}
		a, _ := toStr(l)
		b, _ := toStr(r)
		return mkStr(a + b)
	default:
		if r.k == KErr {
			return r
		}
		if l.k == KErr {
			return l
		}
		c := cmpVals(l, r)
		switch n.bop {
		case '=':
			return mkBool(c == 0)
		case 'n':
			return mkBool(c != 0)
		case '<':
			return mkBool(c < 0)
		case '>':
			return mkBool(c > 0)
		case 'l':
			return mkBool(c <= 0)
		case 'g':
			return mkBool(c >= 0)
		}
	}
	return mkErr(eVal)
}

// ---- range helpers ----
type Rng struct {
	sh             *Sheet
	r1, c1, r2, c2 int
}

func (r Rng) rows() int { return r.r2 - r.r1 + 1 }
func (r Rng) cols() int { return r.c2 - r.c1 + 1 }

func nodeRng(n *Node) (Rng, bool) {
	if n.op == NRef || n.op == NRange {
		return Rng{n.sh, n.r1, n.c1, n.r2, n.c2}, true
	}
	return Rng{}, false
}

func (cx *Ctx) at(r Rng, i, j int) Val {
	return cx.wb.get(r.sh, r.r1+i, r.c1+j)
}

// visit each value in args; fromRef indicates the value is from a reference.
func (cx *Ctx) visit(args []*Node, fn func(v Val, fromRef bool) bool) {
	for _, a := range args {
		if rg, ok := nodeRng(a); ok {
			sh := rg.sh
			r2 := rg.r2
			c2 := rg.c2
			if !cx.fullRange {
				if r2 >= sh.R {
					r2 = sh.R - 1
				}
				if c2 >= sh.C {
					c2 = sh.C - 1
				}
			}
			for r := rg.r1; r <= r2; r++ {
				for c := rg.c1; c <= c2; c++ {
					if !fn(cx.wb.get(sh, r, c), true) {
						return
					}
				}
			}
		} else {
			v := cx.eval(a)
			if !fn(v, a.pass && cx.lastRef) {
				return
			}
		}
	}
}

// numeric aggregator: returns values list and error
func (cx *Ctx) nums(args []*Node) ([]float64, Val) {
	var out []float64
	var ev Val
	cx.lastFirstBool = false
	cx.flags = cx.flags[:0]
	decided := false
	for _, a := range args {
		cx.visit([]*Node{a}, func(v Val, ref bool) bool {
			switch v.k {
			case KNum, KBool:
				if !decided {
					decided = true
					cx.lastFirstBool = ref && v.k == KBool
				}
				out = append(out, v.n)
				cx.flags = append(cx.flags, ref && v.k == KBool)
			case KStr:
				if !ref {
					ev = mkErr(eVal)
					return false
				}
			case KErr:
				ev = v
				return false
			}
			return true
		})
		if ev.k == KErr {
			return out, ev
		}
	}
	return out, ev
}

func (cx *Ctx) scalarNum(n *Node) (float64, Val) {
	return toNum(cx.eval(n))
}

func (cx *Ctx) scalarStr(n *Node) (string, Val) {
	return toStr(cx.eval(n))
}

func isWild(s string) bool {
	return strings.ContainsAny(s, "*?~")
}

// wildcard match (case-insensitive: both inputs must be lowercased)
func wildMatch(p, s []rune) bool {
	for len(p) > 0 {
		switch p[0] {
		case '*':
			for len(p) > 0 && p[0] == '*' {
				p = p[1:]
			}
			if len(p) == 0 {
				return true
			}
			for i := 0; i <= len(s); i++ {
				if wildMatch(p, s[i:]) {
					return true
				}
			}
			return false
		case '?':
			if len(s) == 0 {
				return false
			}
			p = p[1:]
			s = s[1:]
		case '~':
			if len(p) > 1 && (p[1] == '*' || p[1] == '?' || p[1] == '~') {
				p = p[1:]
			}
			fallthrough
		default:
			if len(s) == 0 || s[0] != p[0] {
				return false
			}
			p = p[1:]
			s = s[1:]
		}
	}
	return len(s) == 0
}

func monthIdx(s string) int {
	s = strings.ToLower(s)
	for i, m := range monthNames {
		lm := strings.ToLower(m)
		if s == lm || s == lm[:3] {
			return i + 1
		}
	}
	return 0
}

func parseTimeStr(s string) (float64, bool) {
	p := strings.Split(s, ":")
	if len(p) < 2 || len(p) > 3 {
		return 0, false
	}
	h, e1 := strconv.Atoi(p[0])
	m, e2 := strconv.Atoi(p[1])
	sec := 0.0
	var e3 error
	if len(p) == 3 {
		sec, e3 = strconv.ParseFloat(p[2], 64)
	}
	if e1 != nil || e2 != nil || e3 != nil || h < 0 || m < 0 || m > 59 || sec < 0 || sec >= 60 {
		return 0, false
	}
	return (float64(h)*3600 + float64(m)*60 + sec) / 86400, true
}

func parseDateStr(s string) (float64, bool) {
	s = strings.TrimSpace(s)
	if strings.IndexByte(s, ':') > 0 && len(s) >= 3 {
		return parseTimeStr(s)
	}
	if len(s) < 6 {
		return 0, false
	}
	if i := strings.IndexByte(s, '/'); i > 0 {
		p := strings.Split(s, "/")
		if len(p) == 3 {
			m, e1 := strconv.Atoi(p[0])
			d, e2 := strconv.Atoi(p[1])
			y, e3 := strconv.Atoi(p[2])
			if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
				if y < 30 {
					y += 2000
				} else if y < 100 {
					y += 1900
				}
				return float64(dateSerial(y, m, d)), true
			}
		}
		return 0, false
	}
	if strings.Count(s, "-") == 2 {
		p := strings.Split(s, "-")
		if mi := monthIdx(p[1]); mi > 0 {
			d, e1 := strconv.Atoi(p[0])
			y, e3 := strconv.Atoi(p[2])
			if e1 == nil && e3 == nil && d >= 1 && d <= 31 {
				if y < 30 {
					y += 2000
				} else if y < 100 {
					y += 1900
				}
				return float64(dateSerial(y, mi, d)), true
			}
		}
		y, e1 := strconv.Atoi(p[0])
		m, e2 := strconv.Atoi(p[1])
		d, e3 := strconv.Atoi(p[2])
		if e1 == nil && e2 == nil && e3 == nil && len(p[0]) == 4 && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
			return float64(dateSerial(y, m, d)), true
		}
	}
	return 0, false
}

func (cx *Ctx) safeEval(n *Node) (v Val) {
	defer func() {
		if r := recover(); r != nil {
			v = mkErr(eVal)
		}
	}()
	return cx.eval(n)
}
