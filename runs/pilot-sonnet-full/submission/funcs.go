package main

import (
	"math"
	"sort"
	"strconv"
	"strings"
)

var funcTable map[string]Fn

type kahan struct{ s, c float64 }

func (k *kahan) add(x float64) {
	t := k.s + x
	if math.Abs(k.s) >= math.Abs(x) {
		k.c += (k.s - t) + x
	} else {
		k.c += (x - t) + k.s
	}
	k.s = t
}
func (k *kahan) get() float64 { return k.s + k.c }

func init() {
	funcTable = map[string]Fn{
		"ABS":         m1(math.Abs),
		"SIGN":        m1(func(x float64) float64 { if x > 0 { return 1 }; if x < 0 { return -1 }; return 0 }),
		"INT":         m1(approxFloor),
		"SQRT":        fSqrt,
		"EXP":         m1(math.Exp),
		"LN":          fLn,
		"LOG10":       fLog10,
		"LOG":         fLog,
		"PI":          func(cx *Ctx, a []*Node) Val { return mkNum(math.Pi) },
		"POWER":       fPower,
		"MOD":         fMod,
		"ROUND":       fRound(0),
		"ROUNDDOWN":   fRound(1),
		"ROUNDUP":     fRound(2),
		"TRUNC":       fRound(1),
		"SUM":         fSum,
		"PRODUCT":     fProduct,
		"SUMSQ":       fSumsq,
		"MAX":         fMax(true),
		"MIN":         fMax(false),
		"AVERAGE":     fAverage,
		"MEDIAN":      fMedian,
		"LARGE":       fLarge(true),
		"SMALL":       fLarge(false),
		"STDEV":       fVar(true),
		"VAR":         fVar(false),
		"COUNT":       fCount,
		"COUNTA":      fCountA,
		"COUNTBLANK":  fCountBlank,
		"IF":          fIf,
		"IFS":         fIfs,
		"IFERROR":     fIfError,
		"IFNA":        fIfNA,
		"AND":         fAndOr(0),
		"OR":          fAndOr(1),
		"XOR":         fAndOr(2),
		"NOT":         fNot,
		"SWITCH":      fSwitch,
		"CHOOSE":      fChoose,
		"ISBLANK":     fIs(func(v Val) bool { return v.k == KEmpty }),
		"ISNUMBER":    fIs(func(v Val) bool { return v.k == KNum || v.k == KBool }),
		"ISTEXT":      fIs(func(v Val) bool { return v.k == KStr }),
		"ISLOGICAL":   fIs2(func(v Val) bool { return v.k == KBool }, true),
		"ISERROR":     fIs(func(v Val) bool { return v.k == KErr }),
		"ISNA":        fIs(func(v Val) bool { return v.k == KErr && int(v.n) == eNA }),
		"N":           fN,
		"T":           fT,
		"TRUE":        func(cx *Ctx, a []*Node) Val { return mkBool(true) },
		"FALSE":       func(cx *Ctx, a []*Node) Val { return mkBool(false) },
		"LEN":         fLen,
		"LEFT":        fLeftRight(true),
		"RIGHT":       fLeftRight(false),
		"MID":         fMid,
		"UPPER":       fCase(0),
		"LOWER":       fCase(1),
		"PROPER":      fCase(2),
		"TRIM":        fTrim,
		"REPT":        fRept,
		"REPLACE":     fReplace,
		"SUBSTITUTE":  fSubstitute,
		"FIND":        fFind(true),
		"SEARCH":      fFind(false),
		"EXACT":       fExact,
		"CONCAT":      fConcat(true),
		"CONCATENATE": fConcat(false),
		"TEXTJOIN":    fTextJoin,
		"TEXT":        fText,
		"VALUE":       fValue,
		"DATE":        fDate,
		"YEAR":        fDatePart(0),
		"MONTH":       fDatePart(1),
		"DAY":         fDatePart(2),
		"WEEKDAY":     fWeekday,
		"EDATE":       fEdate(false),
		"EOMONTH":     fEdate(true),
		"DAYS":        fDays,
		"DATEDIF":     fDatedif,
		"SUMIF":       fSumIf(0),
		"AVERAGEIF":   fSumIf(1),
		"COUNTIF":     fCountIf,
		"SUMIFS":      fSumIfs(0),
		"AVERAGEIFS":  fSumIfs(1),
		"COUNTIFS":    fSumIfs(2),
		"SUMPRODUCT":  fSumProduct,
		"VLOOKUP":     fVLookup(true),
		"HLOOKUP":     fVLookup(false),
		"MATCH":       fMatch,
		"INDEX":       fIndex,
		"XLOOKUP":     fXLookup,
	}
}

func m1(f func(float64) float64) Fn {
	return func(cx *Ctx, a []*Node) Val {
		if len(a) < 1 {
			return mkErr(eVal)
		}
		x, e := cx.scalarNum(a[0])
		if e.k == KErr {
			return e
		}
		return checkNum(f(x))
	}
}

func fSqrt(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	if x < 0 {
		return mkErr(eVal)
	}
	return mkNum(math.Sqrt(x))
}
func fLn(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	if x <= 0 {
		return mkErr(eVal)
	}
	return mkNum(math.Log(x))
}
func fLog10(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	if x <= 0 {
		return mkErr(eVal)
	}
	return mkNum(math.Log10(x))
}
func fLog(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	b := 10.0
	if len(a) > 1 {
		b, e = cx.scalarNum(a[1])
		if e.k == KErr {
			return e
		}
	}
	if x <= 0 || b <= 0 {
		return mkErr(eVal)
	}
	if b == 1 {
		return mkErr(eVal)
	}
	if b == 10 {
		return mkNum(math.Log10(x))
	}
	return mkNum(math.Log(x) / math.Log(b))
}
func fPower(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	y, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	return checkNum(math.Pow(x, y))
}
func fMod(cx *Ctx, a []*Node) Val {
	x, e0 := cx.scalarNum(a[0])
	y, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	if y == 0 {
		return mkErr(eDiv)
	}
	if e0.k == KErr {
		return e0
	}
	q := approxFloor(x / y)
	return checkNum(approxSub(x, y*q))
}

// decimal rounding on the 15-digit representation
func roundDec(x float64, n int, mode int) float64 {
	if x == 0 || math.IsNaN(x) || math.IsInf(x, 0) {
		return x
	}
	neg := x < 0
	ax := math.Abs(x)
	s := strconv.FormatFloat(ax, 'e', 14, 64)
	ei := strings.IndexByte(s, 'e')
	exp, _ := strconv.Atoi(s[ei+1:])
	digits := []byte(s[:1] + s[2:ei]) // 15 digits
	k := exp + 1 + n                  // number of digits kept
	if k >= 15 {
		return x
	}
	var keep []byte
	up := false
	if k < 0 {
		keep = nil
		for _, d := range digits {
			if d != '0' {
				if mode == 2 {
					up = true
				}
			}
		}
	} else {
		keep = append(keep, digits[:k]...)
		rest := digits[k:]
		switch mode {
		case 0:
			up = rest[0] >= '5'
		case 1:
			up = false
		case 2:
			for _, d := range rest {
				if d != '0' {
					up = true
				}
			}
		}
	}
	// build integer of kept digits, value = intval * 10^(exp+1-k)
	num := new(bigInt)
	_ = num
	ds := string(keep)
	if ds == "" {
		ds = "0"
	}
	if up {
		b := []byte(ds)
		i := len(b) - 1
		for i >= 0 {
			if b[i] == '9' {
				b[i] = '0'
				i--
			} else {
				b[i]++
				break
			}
		}
		if i < 0 {
			b = append([]byte{'1'}, b...)
		}
		ds = string(b)
	}
	f, _ := strconv.ParseFloat(ds+"e"+strconv.Itoa(exp+1-k), 64)
	if neg {
		f = -f
	}
	return f
}

type bigInt struct{}

func fRound(mode int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		x, e := cx.scalarNum(a[0])
		if e.k == KErr {
			return e
		}
		n := 0.0
		if len(a) > 1 {
			n, e = cx.scalarNum(a[1])
			if e.k == KErr {
				return e
			}
		}
		if n > 32767 || n < -32768 {
			return mkErr(eVal)
		}
		return checkNum(roundDec(x, int(math.Trunc(n)), mode))
	}
}

func fSum(cx *Ctx, a []*Node) Val {
	v, e := cx.nums(a)
	if e.k == KErr {
		return e
	}
	s, c := 0.0, 0.0
	for _, x := range v {
		t := s + x
		if math.Abs(s) >= math.Abs(x) {
			c += (s - t) + x
		} else {
			c += (x - t) + s
		}
		s = t
	}
	return checkNum(s + c)
}
func fProduct(cx *Ctx, a []*Node) Val {
	v, e := cx.nums(a)
	if e.k == KErr {
		return e
	}
	if len(v) == 0 {
		return mkNum(0)
	}
	p := 1.0
	for _, x := range v {
		p *= x
	}
	return checkNum(p)
}
func fSumsq(cx *Ctx, a []*Node) Val {
	v, e := cx.nums(a)
	if e.k == KErr {
		return e
	}
	p := 0.0
	for _, x := range v {
		p += x * x
	}
	return checkNum(p)
}
func fMax(isMax bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		v, e := cx.nums(a)
		if e.k == KErr {
			return e
		}
		if len(v) == 0 {
			return mkNum(0)
		}
		m := v[0]
		mi := 0
		for i, x := range v[1:] {
			if (isMax && x > m) || (!isMax && x < m) {
				m = x
				mi = i + 1
			}
		}
		if cx.lastFirstBool && mi < len(cx.flags) && cx.flags[mi] {
			return Val{k: KBool, n: m}
		}
		return mkNum(m)
	}
}
func fAverage(cx *Ctx, a []*Node) Val {
	v, e := cx.nums(a)
	if e.k == KErr {
		return e
	}
	if len(v) == 0 {
		return mkErr(eDiv)
	}
	var kk kahan
	for _, x := range v {
		kk.add(x)
	}
	return mkNum(kk.get() / float64(len(v)))
}
func fMedian(cx *Ctx, a []*Node) Val {
	v, e := cx.nums(a)
	if e.k == KErr {
		return e
	}
	if len(v) == 0 {
		return mkErr(eVal)
	}
	sort.Float64s(v)
	n := len(v)
	if n%2 == 1 {
		return mkNum(v[n/2])
	}
	return mkNum((v[n/2-1] + v[n/2]) / 2)
}
func fLarge(large bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		if len(a) < 2 {
			return mkErr(eVal)
		}
		v, e := cx.nums(a[:1])
		if e.k == KErr {
			return e
		}
		k, e := cx.scalarNum(a[1])
		if e.k == KErr {
			return e
		}
		var ki int
		if large {
			ki = int(math.Ceil(k))
		} else {
			ki = int(math.Floor(k))
		}
		if ki < 1 || ki > len(v) {
			return mkErr(eVal)
		}
		sort.Float64s(v)
		if large {
			return mkNum(v[len(v)-ki])
		}
		return mkNum(v[ki-1])
	}
}
func fVar(sd bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		v, e := cx.nums(a)
		if e.k == KErr {
			return e
		}
		n := float64(len(v))
		if n < 2 {
			return mkErr(eDiv)
		}
		var ks kahan
		for _, x := range v {
			ks.add(x)
		}
		m := ks.get() / n
		vs := 0.0
		for _, x := range v {
			vs += (x - m) * (x - m)
		}
		s := vs / (n - 1)
		if sd {
			return mkNum(math.Sqrt(s))
		}
		return mkNum(s)
	}
}
func fCount(cx *Ctx, a []*Node) Val {
	n := 0
	saved := cx.fe
	defer func() { cx.fe = saved }()
	cx.visit(a, func(v Val, ref bool) bool {
		switch v.k {
		case KNum:
			n++
		case KBool:
			n++
		case KStr:
			if !ref {
				if _, e := toNum(v); e.k != KErr {
					n++
				}
			}
		}
		return true
	})
	return mkNum(float64(n))
}
func fCountA(cx *Ctx, a []*Node) Val {
	n := 0
	saved := cx.fe
	defer func() { cx.fe = saved }()
	cx.visit(a, func(v Val, ref bool) bool {
		if v.k != KEmpty {
			n++
		}
		return true
	})
	return mkNum(float64(n))
}
func fCountBlank(cx *Ctx, a []*Node) Val {
	rg, ok := nodeRng(a[0])
	if !ok {
		v := cx.eval(a[0])
		if v.k == KEmpty || (v.k == KStr && v.s == "") {
			return mkNum(1)
		}
		return mkNum(0)
	}
	n := 0
	for i := 0; i < rg.rows(); i++ {
		for j := 0; j < rg.cols(); j++ {
			v := cx.at(rg, i, j)
			if v.k == KEmpty || (v.k == KStr && v.s == "") {
				n++
			}
		}
	}
	return mkNum(float64(n))
}

// ---- logic ----
func toBool(v Val) (bool, Val) {
	switch v.k {
	case KNum, KBool:
		return v.n != 0, Val{}
	case KEmpty:
		return false, Val{}
	case KStr:
		u := strings.ToUpper(strings.TrimSpace(v.s))
		if u == "TRUE" {
			return true, Val{}
		}
		if u == "FALSE" {
			return false, Val{}
		}
		if nv, e := toNum(v); e.k != KErr {
			return nv != 0, Val{}
		}
		return false, mkErr(eVal)
	}
	return false, v
}

func fIf(cx *Ctx, a []*Node) Val {
	if len(a) < 2 {
		return mkErr(eVal)
	}
	c, e := toBool(cx.eval(a[0]))
	cx.ll = false
	if e.k == KErr {
		return cx.fin(e)
	}
	if c {
		return cx.eval(a[1])
	}
	if len(a) > 2 {
		v := cx.eval(a[2])
		if a[2].op == NConst && v.k == KEmpty {
			return mkNum(0)
		}
		return v
	}
	return mkBool(false)
}
func fIfs(cx *Ctx, a []*Node) Val {
	if len(a)%2 != 0 {
		return mkErr(eNA)
	}
	for i := 0; i < len(a); i += 2 {
		c, e := toBool(cx.eval(a[i]))
		cx.ll = false
		if e.k == KErr {
			return cx.fin(e)
		}
		if c {
			return cx.eval(a[i+1])
		}
	}
	return cx.fin(mkErr(eNA))
}
func fIfError(cx *Ctx, a []*Node) Val {
	saved := cx.fe
	v := cx.eval(a[0])
	if v.k == KErr {
		cx.fe = saved
		return cx.eval(a[1])
	}
	return v
}
func fIfNA(cx *Ctx, a []*Node) Val {
	saved := cx.fe
	v := cx.eval(a[0])
	if v.k == KErr && int(v.n) == eNA {
		cx.fe = saved
		return cx.eval(a[1])
	}
	return v
}
func fAndOr(mode int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		res := mode == 0
		cnt := 0
		par := false
		var ev Val
		cx.visit(a, func(v Val, ref bool) bool {
			var b bool
			switch v.k {
			case KErr:
				ev = v
				return false
			case KNum, KBool:
				b = v.n != 0
			case KStr:
				if ref {
					return true
				}
				ev = mkErr(eVal)
				return false
			case KEmpty:
				if ref {
					return true
				}
			}
			cnt++
			switch mode {
			case 0:
				res = res && b
			case 1:
				res = res || b
			case 2:
				par = par != b
			}
			return true
		})
		if ev.k == KErr {
			return ev
		}
		if cnt == 0 {
			return mkErr(eVal)
		}
		if mode == 2 {
			return mkBool(par)
		}
		return mkBool(res)
	}
}
func fNot(cx *Ctx, a []*Node) Val {
	b, e := toBool(cx.eval(a[0]))
	if e.k == KErr {
		return e
	}
	return mkBool(!b)
}
func fSwitch(cx *Ctx, a []*Node) Val {
	if len(a) < 3 {
		return mkErr(eVal)
	}
	x := cx.eval(a[0])
	if x.k == KErr {
		return cx.fin(x)
	}
	i := 1
	var skipped Val
	savedAll := cx.fe
	for ; i+1 < len(a); i += 2 {
		saved := cx.fe
		y := cx.eval(a[i])
		if y.k == KErr {
			cx.fe = saved
			if skipped.k != KErr {
				skipped = y
			}
			continue
		}
		if cmpVals(x, y) == 0 {
			return cx.eval(a[i+1])
		}
	}
	if i < len(a) {
		cx.fe = savedAll
		return cx.eval(a[i])
	}
	return cx.fin(mkErr(eNA))
}
func fChoose(cx *Ctx, a []*Node) Val {
	f, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	i := int(math.Trunc(f))
	if i < 1 || i >= len(a) {
		return cx.fin(mkErr(eVal))
	}
	return cx.eval(a[i])
}
func fIs(p func(Val) bool) Fn { return fIs2(p, false) }
func fIs2(p func(Val) bool, isLogical bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		if len(a) < 1 {
			return mkErr(eVal)
		}
		saved := cx.fe
		ll := cx.ll
		v := cx.eval(a[0])
		if v.k == KErr {
			cx.fe = saved
		}
		if isLogical && a[0].op == NConst && v.k != KBool {
			return mkBool(ll)
		}
		return mkBool(p(v))
	}
}
func fN(cx *Ctx, a []*Node) Val {
	v := cx.eval(a[0])
	switch v.k {
	case KNum, KBool:
		return mkNum(v.n)
	case KErr:
		return v
	}
	return mkNum(0)
}
func fT(cx *Ctx, a []*Node) Val {
	v := cx.eval(a[0])
	if v.k == KStr || v.k == KErr {
		return v
	}
	return mkStr("")
}

// ---- text ----
func fLen(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	return mkNum(float64(len([]rune(s))))
}
func fLeftRight(left bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		s, e := cx.scalarStr(a[0])
		if e.k == KErr {
			return e
		}
		n := 1.0
		if len(a) > 1 {
			n, e = cx.scalarNum(a[1])
			if e.k == KErr {
				return e
			}
		}
		if n < 0 || n > 2147483647 {
			return mkErr(eVal)
		}
		r := []rune(s)
		k := int(n)
		if k > len(r) {
			k = len(r)
		}
		if left {
			return mkStr(string(r[:k]))
		}
		return mkStr(string(r[len(r)-k:]))
	}
}
func fMid(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	st, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	n, e := cx.scalarNum(a[2])
	if e.k == KErr {
		return e
	}
	if st < 1 || n < 0 || n > 2147483647 || st > 2147483647 {
		return mkErr(eVal)
	}
	r := []rune(s)
	i := int(st) - 1
	if i >= len(r) {
		return mkStr("")
	}
	j := i + int(n)
	if j > len(r) {
		j = len(r)
	}
	return mkStr(string(r[i:j]))
}
func fCase(mode int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		s, e := cx.scalarStr(a[0])
		if e.k == KErr {
			return e
		}
		switch mode {
		case 0:
			return mkStr(strings.ToUpper(s))
		case 1:
			return mkStr(strings.ToLower(s))
		}
		r := []rune(s)
		prev := false
		for i, c := range r {
			isL := (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c > 127
			if isL {
				if prev {
					r[i] = []rune(strings.ToLower(string(c)))[0]
				} else {
					r[i] = []rune(strings.ToUpper(string(c)))[0]
				}
			}
			prev = isL
		}
		return mkStr(string(r))
	}
}
func fTrim(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	return mkStr(strings.Join(strings.FieldsFunc(s, func(r rune) bool { return r == ' ' }), " "))
}
func fRept(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	n, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	if n < 0 {
		return mkErr(eVal)
	}
	if float64(len(s))*n > 1e7 {
		return mkErr(eVal)
	}
	return mkStr(strings.Repeat(s, int(n)))
}
func fReplace(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	st, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	n, e := cx.scalarNum(a[2])
	if e.k == KErr {
		return e
	}
	ns, e := cx.scalarStr(a[3])
	if e.k == KErr {
		return e
	}
	if st < 1 || n < 0 || st > 2147483647 || n > 2147483647 {
		return mkErr(eVal)
	}
	r := []rune(s)
	i := int(st) - 1
	if i > len(r) {
		i = len(r)
	}
	j := i + int(n)
	if j > len(r) {
		j = len(r)
	}
	return mkStr(string(r[:i]) + ns + string(r[j:]))
}
func fSubstitute(cx *Ctx, a []*Node) Val {
	s, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	old, e := cx.scalarStr(a[1])
	if e.k == KErr {
		return e
	}
	nw, e := cx.scalarStr(a[2])
	if e.k == KErr {
		return e
	}
	if old == "" {
		return mkStr(s)
	}
	if len(a) > 3 {
		n, e := cx.scalarNum(a[3])
		if e.k == KErr {
			return e
		}
		if n < 1 {
			return mkErr(eVal)
		}
		cnt := 0
		pos := 0
		for {
			i := strings.Index(s[pos:], old)
			if i < 0 {
				return mkStr(s)
			}
			cnt++
			if cnt == int(n) {
				return mkStr(s[:pos+i] + nw + s[pos+i+len(old):])
			}
			pos += i + len(old)
		}
	}
	return mkStr(strings.ReplaceAll(s, old, nw))
}
func fFind(cs bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		f, e := cx.scalarStr(a[0])
		if e.k == KErr {
			return e
		}
		s, e := cx.scalarStr(a[1])
		if e.k == KErr {
			return e
		}
		st := 1.0
		if len(a) > 2 {
			st, e = cx.scalarNum(a[2])
			if e.k == KErr {
				return e
			}
		}
		r := []rune(s)
		if f == "" || st < 1 || int(st) > len(r) {
			return mkErr(eVal)
		}
		i0 := int(st) - 1
		if cs {
			fr := []rune(f)
			for i := i0; i+len(fr) <= len(r); i++ {
				if string(r[i:i+len(fr)]) == f {
					return mkNum(float64(i + 1))
				}
			}
			return mkErr(eVal)
		}
		pr := []rune(lowerStr(f))
		sr := []rune(lowerStr(s))
		if len(sr) != len(r) {
			sr = r
		}
		if !isWild(f) {
			for i := i0; i+len(pr) <= len(sr); i++ {
				if string(sr[i:i+len(pr)]) == string(pr) {
					return mkNum(float64(i + 1))
				}
			}
			return mkErr(eVal)
		}
		for i := i0; i <= len(sr); i++ {
			// match pattern at position i against any prefix
			for j := i; j <= len(sr); j++ {
				if wildMatch(pr, sr[i:j]) {
					return mkNum(float64(i + 1))
				}
			}
		}
		return mkErr(eVal)
	}
}
func fExact(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	y, e := cx.scalarStr(a[1])
	if e.k == KErr {
		return e
	}
	return mkBool(x == y)
}
func fConcat(allowRange bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		var sb strings.Builder
		var ev Val
		for _, n := range a {
			if allowRange {
				cx.visit([]*Node{n}, func(v Val, ref bool) bool {
					if ref && v.k == KBool {
						v = mkNum(v.n)
					}
					s, e := toStr(v)
					if e.k == KErr {
						ev = e
						return false
					}
					sb.WriteString(s)
					return true
				})
			} else {
				s, e := cx.scalarStr(n)
				if e.k == KErr {
					return e
				}
				sb.WriteString(s)
			}
			if ev.k == KErr {
				return ev
			}
		}
		return mkStr(sb.String())
	}
}
func fTextJoin(cx *Ctx, a []*Node) Val {
	if len(a) < 3 {
		return mkErr(eVal)
	}
	d, e := cx.scalarStr(a[0])
	if e.k == KErr {
		return e
	}
	ie, e := toBool(cx.eval(a[1]))
	if e.k == KErr {
		return e
	}
	var parts []string
	var ev Val
	cx.fullRange = !ie
	defer func() { cx.fullRange = false }()
	cx.visit(a[2:], func(v Val, ref bool) bool {
		if ref && v.k == KBool {
			v = mkNum(v.n)
		}
		s, er := toStr(v)
		if er.k == KErr {
			ev = er
			return false
		}
		if ie && s == "" {
			return true
		}
		parts = append(parts, s)
		return true
	})
	if ev.k == KErr {
		return ev
	}
	return mkStr(strings.Join(parts, d))
}
func fValue(cx *Ctx, a []*Node) Val {
	v := cx.eval(a[0])
	switch v.k {
	case KNum:
		return v
	case KBool:
		return v
	case KEmpty:
		return mkNum(0)
	case KStr:
		f, e := toNum(v)
		if e.k == KErr {
			return e
		}
		return mkNum(f)
	}
	return v
}

// ---- lookup / criteria ----
type crit struct {
	op     byte // '=', 'n','<','>','l','g'
	isNum  bool
	num    float64
	str    string // lowercased
	wild   []rune
	hasWld bool
	empty  bool
	fromStr bool
}

func parseCrit(v Val) crit {
	var c crit
	c.op = '='
	switch v.k {
	case KNum, KBool:
		c.isNum = true
		c.num = v.n
		return c
	case KEmpty:
		c.isNum = true
		c.num = 0
		return c
	}
	s := v.s
	switch {
	case strings.HasPrefix(s, "<="):
		c.op, s = 'l', s[2:]
	case strings.HasPrefix(s, ">="):
		c.op, s = 'g', s[2:]
	case strings.HasPrefix(s, "<>"):
		c.op, s = 'n', s[2:]
	case strings.HasPrefix(s, "<"):
		c.op, s = '<', s[1:]
	case strings.HasPrefix(s, ">"):
		c.op, s = '>', s[1:]
	case strings.HasPrefix(s, "="):
		c.op, s = '=', s[1:]
	}
	if f, ok := parseNum(s); ok {
		c.isNum = true
		c.num = f
		c.fromStr = true
		c.str = lowerStr(s)
		return c
	}
	if strings.EqualFold(s, "true") || strings.EqualFold(s, "false") {
		c.isNum = true
		if strings.EqualFold(s, "true") {
			c.num = 1
		}
		return c
	}
	if s == "" {
		c.empty = true
	}
	c.str = lowerStr(s)
	if (c.op == '=' || c.op == 'n') && isWild(s) {
		c.hasWld = true
		c.wild = []rune(c.str)
	}
	return c
}

func (c *crit) match(v Val) bool {
	switch v.k {
	case KErr:
		return false
	}
	if c.isNum {
		if v.k == KNum || v.k == KBool {
			x := v.n
			switch c.op {
			case '=':
				return approxEqual(x, c.num)
			case 'n':
				return !approxEqual(x, c.num)
			case '<':
				return x < c.num && !approxEqual(x, c.num)
			case '>':
				return x > c.num && !approxEqual(x, c.num)
			case 'l':
				return x < c.num || approxEqual(x, c.num)
			case 'g':
				return x > c.num || approxEqual(x, c.num)
			}
		}
		if v.k == KStr && c.fromStr && (c.op == '=' || c.op == 'n') {
			eq := lowerStr(v.s) == c.str
			return eq == (c.op == '=')
		}
		return c.op == 'n'
	}
	// string criterion
	if v.k == KEmpty {
		if c.empty {
			return c.op == '=' || (c.op != 'n' && false)
		}
		return c.op == 'n'
	}
	if v.k != KStr {
		return c.op == 'n'
	}
	ls := lowerStr(v.s)
	switch c.op {
	case '=', 'n':
		var m bool
		if c.hasWld {
			m = wildMatch(c.wild, []rune(ls))
		} else {
			m = ls == c.str
		}
		return m == (c.op == '=')
	case '<':
		return ls < c.str
	case '>':
		return ls > c.str
	case 'l':
		return ls <= c.str
	case 'g':
		return ls >= c.str
	}
	return false
}

func fCountIf(cx *Ctx, a []*Node) Val {
	if len(a) < 2 {
		return mkErr(eVal)
	}
	rg, ok := nodeRng(a[0])
	if !ok {
		return mkErr(eVal)
	}
	cv := cx.eval(a[1])
	if cv.k == KErr {
		return cv
	}
	c := parseCrit(cv)
	_, n, e := cx.condAgg([]Rng{rg}, []crit{c}, nil)
	if e.k == KErr {
		return e
	}
	return mkNum(float64(n))
}

func fSumIf(mode int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		if len(a) < 2 {
			return mkErr(eVal)
		}
		rg, ok := nodeRng(a[0])
		if !ok {
			return mkErr(eVal)
		}
		cv := cx.eval(a[1])
		if cv.k == KErr {
			return cv
		}
		c := parseCrit(cv)
		sr := rg
		if len(a) > 2 {
			sr, ok = nodeRng(a[2])
			if !ok {
				return mkErr(eVal)
			}
		}
		sum, cnt, e := cx.condAgg([]Rng{rg}, []crit{c}, &sr)
		if e.k == KErr {
			return e
		}
		if mode == 1 {
			if cnt == 0 {
				return mkErr(eDiv)
			}
			return mkNum(sum / float64(cnt))
		}
		return mkNum(sum)
	}
}

func fSumIfs(mode int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		var sr Rng
		var ok bool
		start := 0
		if mode != 2 {
			if len(a) < 3 {
				return mkErr(eVal)
			}
			sr, ok = nodeRng(a[0])
			if !ok {
				return mkErr(eVal)
			}
			start = 1
		}
		if (len(a)-start)%2 != 0 || len(a)-start < 2 {
			return mkErr(eVal)
		}
		var rgs []Rng
		var cs []crit
		for i := start; i < len(a); i += 2 {
			rg, ok := nodeRng(a[i])
			if !ok {
				return mkErr(eVal)
			}
			cv := cx.eval(a[i+1])
			if cv.k == KErr {
				return cv
			}
			rgs = append(rgs, rg)
			cs = append(cs, parseCrit(cv))
		}
		R, C := rgs[0].rows(), rgs[0].cols()
		for _, p := range rgs {
			if p.rows() != R || p.cols() != C {
				return mkErr(eVal)
			}
		}
		if mode != 2 && (sr.rows() != R || sr.cols() != C) {
			return mkErr(eVal)
		}
		var srp *Rng
		if mode != 2 {
			srp = &sr
		}
		sum, cnt, e := cx.condAgg(rgs, cs, srp)
		if e.k == KErr {
			return e
		}
		switch mode {
		case 0:
			return mkNum(sum)
		case 1:
			if cnt == 0 {
				return mkErr(eDiv)
			}
			return mkNum(sum / float64(cnt))
		}
		return mkNum(float64(cnt))
	}
}

func fSumProduct(cx *Ctx, a []*Node) Val {
	var rgs []Rng
	for _, n := range a {
		rg, ok := nodeRng(n)
		if !ok {
			return mkErr(eVal)
		}
		rgs = append(rgs, rg)
	}
	if len(rgs) == 0 {
		return mkErr(eVal)
	}
	R, C := rgs[0].rows(), rgs[0].cols()
	for _, r := range rgs {
		if r.rows() != R || r.cols() != C {
			return mkErr(eVal)
		}
	}
	var sum kahan
	for i := 0; i < R; i++ {
		for j := 0; j < C; j++ {
			p := 1.0
			for _, r := range rgs {
				v := cx.at(r, i, j)
				if v.k == KErr {
					return v
				}
				if v.k == KNum || v.k == KBool {
					p *= v.n
				} else {
					p = 0
				}
			}
			sum.add(p)
		}
	}
	return checkNum(sum.get())
}

func approxValue(x float64) float64 {
	if x == 0 || math.IsNaN(x) || math.IsInf(x, 0) {
		return x
	}
	ax := math.Abs(x)
	exp := int(math.Floor(math.Log10(ax)))
	k := 14 - exp
	var y float64
	if k >= 0 {
		y = ax * math.Pow10(k)
	} else {
		y = ax / math.Pow10(-k)
	}
	r := math.Floor(y + 0.5)
	var res float64
	if k >= 0 {
		res = r / math.Pow10(k)
	} else {
		res = r * math.Pow10(-k)
	}
	if x < 0 {
		return -res
	}
	return res
}

func approxFloor(x float64) float64 { return math.Floor(approxValue(x)) }
