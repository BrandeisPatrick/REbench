package main

import (
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

type fnImpl func(ctx *Ctx, args []Node) Val

var funcs map[string]fnImpl

func init() {
	funcs = map[string]fnImpl{
		"ABS": num1(math.Abs), "EXP": num1(math.Exp), "PI": fnPI,
		"SQRT": fnSQRT, "LN": fnLN, "LOG": fnLOG, "LOG10": fnLOG10, "POWER": fnPOWER,
		"INT": num1(math.Floor), "SIGN": fnSIGN, "MOD": fnMOD,
		"ROUND": fnROUND, "ROUNDDOWN": fnROUNDDOWN, "ROUNDUP": fnROUNDUP, "TRUNC": fnTRUNC,
		"SUM": fnSUM, "SUMSQ": fnSUMSQ, "PRODUCT": fnPRODUCT, "AVERAGE": fnAVERAGE,
		"MIN": fnMIN, "MAX": fnMAX, "MEDIAN": fnMEDIAN, "LARGE": fnLARGE, "SMALL": fnSMALL,
		"STDEV": fnSTDEV, "VAR": fnVAR, "COUNT": fnCOUNT, "COUNTA": fnCOUNTA, "COUNTBLANK": fnCOUNTBLANK,
		"SUMPRODUCT": fnSUMPRODUCT,
		"SUMIF":      fnSUMIF, "SUMIFS": fnSUMIFS, "COUNTIF": fnCOUNTIF, "COUNTIFS": fnCOUNTIFS,
		"AVERAGEIF": fnAVERAGEIF, "AVERAGEIFS": fnAVERAGEIFS,
		"IF": fnIF, "IFERROR": fnIFERROR, "IFNA": fnIFNA, "IFS": fnIFS, "SWITCH": fnSWITCH,
		"AND": fnAND, "OR": fnOR, "XOR": fnXOR, "NOT": fnNOT, "CHOOSE": fnCHOOSE,
		"TRUE":    func(ctx *Ctx, a []Node) Val { return boolVal(true) },
		"FALSE":   func(ctx *Ctx, a []Node) Val { return boolVal(false) },
		"ISBLANK": fnISBLANK, "ISERROR": fnISERROR, "ISNA": fnISNA, "ISNUMBER": fnISNUMBER,
		"ISTEXT": fnISTEXT, "ISLOGICAL": fnISLOGICAL, "N": fnN, "T": fnT,
		"LEFT": fnLEFT, "RIGHT": fnRIGHT, "MID": fnMID, "LEN": fnLEN, "LOWER": fnLOWER, "UPPER": fnUPPER,
		"PROPER": fnPROPER, "TRIM": fnTRIM, "REPT": fnREPT, "SUBSTITUTE": fnSUBSTITUTE, "REPLACE": fnREPLACE,
		"FIND": fnFIND, "SEARCH": fnSEARCH, "EXACT": fnEXACT, "CONCAT": fnCONCAT, "CONCATENATE": fnCONCATENATE,
		"TEXTJOIN": fnTEXTJOIN, "VALUE": fnVALUE, "TEXT": fnTEXT,
		"DATE": fnDATE, "DATEDIF": fnDATEDIF, "DAY": fnDAY, "DAYS": fnDAYS, "EDATE": fnEDATE, "EOMONTH": fnEOMONTH,
		"MONTH": fnMONTH, "WEEKDAY": fnWEEKDAY, "YEAR": fnYEAR,
		"VLOOKUP": fnVLOOKUP, "HLOOKUP": fnHLOOKUP, "MATCH": fnMATCH, "INDEX": fnINDEX, "XLOOKUP": fnXLOOKUP,
	}
}

// ---------- helpers ----------

func arg(ctx *Ctx, args []Node, i int) Val {
	if i >= len(args) || args[i] == nil {
		return emptyVal
	}
	return deref(ctx, eval(ctx, args[i]))
}

func argRaw(ctx *Ctx, args []Node, i int) Val {
	if i >= len(args) || args[i] == nil {
		return emptyVal
	}
	return eval(ctx, args[i])
}

func argNum(ctx *Ctx, args []Node, i int) (float64, int) {
	v := arg(ctx, args, i)
	return toNum(v)
}

func argNumDef(ctx *Ctx, args []Node, i int, def float64) (float64, int) {
	if i >= len(args) || args[i] == nil {
		return def, 0
	}
	v := arg(ctx, args, i)
	return toNum(v)
}

func argStr(ctx *Ctx, args []Node, i int) (string, int) {
	v := arg(ctx, args, i)
	return toStr(v)
}

func num1(f func(float64) float64) fnImpl {
	return func(ctx *Ctx, args []Node) Val {
		x, e := argNum(ctx, args, 0)
		if e != 0 {
			return errVal(e)
		}
		return numVal(f(x))
	}
}

// iterate over cells of a range (row-major); callback returns false to stop
func forRange(ctx *Ctx, r Ref, f func(v Val) bool) {
	sh := ctx.book.sheets[r.sheet]
	for row := r.r1; row <= r.r2; row++ {
		for col := r.c1; col <= r.c2; col++ {
			if col >= len(sh.cols) || row >= len(sh.cols[col]) {
				if !f(emptyVal) {
					return
				}
				continue
			}
			cell := sh.cols[col][row]
			var v Val
			if cell == nil {
				v = emptyVal
			} else if cell.state == 2 {
				v = cell.v
			} else {
				v = ctx.book.cellValue(r.sheet, row, col)
			}
			if !f(v) {
				return
			}
		}
	}
}

// collect numeric values from args (SUM semantics): ranges skip text/empty, direct strings -> #VALUE!
func collectNums(ctx *Ctx, args []Node) ([]float64, int) {
	var out []float64
	var errc int
	for i := len(args) - 1; i >= 0; i-- {
		if args[i] == nil {
			continue
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			if v.ref.r1 == v.ref.r2 && v.ref.c1 == v.ref.c2 {
				cv := deref(ctx, v)
				if cv.k == KErr {
					return nil, int(cv.n)
				}
				if cv.k == KNum || cv.k == KBool {
					out = append(out, cv.n)
				}
				continue
			}
			forRange(ctx, v.ref, func(cv Val) bool {
				if cv.k == KErr {
					errc = int(cv.n)
					return false
				}
				if cv.k == KNum || cv.k == KBool {
					out = append(out, cv.n)
				}
				return true
			})
			if errc != 0 {
				return nil, errc
			}
			continue
		}
		switch v.k {
		case KErr:
			return nil, int(v.n)
		case KNum, KBool:
			out = append(out, v.n)
		case KStr:
			return nil, ErrValue
		}
	}
	return out, 0
}

// ---------- math ----------

func fnPI(ctx *Ctx, args []Node) Val { return numVal(math.Pi) }

func fnSQRT(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	if x < 0 {
		return errVal(ErrValue)
	}
	return numVal(math.Sqrt(x))
}

func fnLN(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	if x <= 0 {
		return errVal(ErrValue)
	}
	return numVal(math.Log(x))
}

func fnLOG10(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	if x <= 0 {
		return errVal(ErrValue)
	}
	return numVal(math.Log10(x))
}

func fnLOG(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	b, e := argNumDef(ctx, args, 1, 10)
	if e != 0 {
		return errVal(e)
	}
	if x <= 0 || b <= 0 || b == 1 {
		return errVal(ErrValue)
	}
	if b == 10 {
		return numVal(math.Log10(x))
	}
	if b == 2 {
		return numVal(math.Log2(x))
	}
	return numVal(math.Log(x) / math.Log(b))
}

func fnPOWER(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	y, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	return powVal(x, y)
}

func fnSIGN(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	if x > 0 {
		return numVal(1)
	} else if x < 0 {
		return numVal(-1)
	}
	return numVal(0)
}

func fnMOD(ctx *Ctx, args []Node) Val {
	a, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	b, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	if b == 0 {
		return errVal(ErrDiv0)
	}
	r := a - b*math.Floor(a/b)
	if math.Abs(r) >= math.Abs(b) {
		r = 0
	}
	return numVal(r)
}

func fnROUND(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	d, e := argNumDef(ctx, args, 1, 0)
	if e != 0 {
		return errVal(e)
	}
	return numVal(roundTo(x, int(math.Trunc(d))))
}

func roundDir(x float64, digits int, up bool) float64 {
	if x == 0 {
		return 0
	}
	p := math.Pow(10, float64(digits))
	y := x * p
	s := strconv.FormatFloat(y, 'g', 15, 64)
	y, _ = strconv.ParseFloat(s, 64)
	var r float64
	if up {
		if y < 0 {
			r = -math.Ceil(-y)
		} else {
			r = math.Ceil(y)
		}
	} else {
		r = math.Trunc(y)
	}
	res := r / p
	if digits > 0 {
		s2 := strconv.FormatFloat(res, 'f', digits, 64)
		res, _ = strconv.ParseFloat(s2, 64)
	}
	return res
}

func fnROUNDUP(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	d, e := argNumDef(ctx, args, 1, 0)
	if e != 0 {
		return errVal(e)
	}
	return numVal(roundDir(x, int(math.Trunc(d)), true))
}

func fnROUNDDOWN(ctx *Ctx, args []Node) Val {
	x, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	d, e := argNumDef(ctx, args, 1, 0)
	if e != 0 {
		return errVal(e)
	}
	return numVal(roundDir(x, int(math.Trunc(d)), false))
}

func fnTRUNC(ctx *Ctx, args []Node) Val { return fnROUNDDOWN(ctx, args) }

func fnSUM(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	return numVal(kahanSum(xs))
}

func kahanSum(xs []float64) float64 {
	s := 0.0
	c := 0.0
	for _, x := range xs {
		t := s + x
		if math.Abs(s) >= math.Abs(x) {
			c += (s - t) + x
		} else {
			c += (x - t) + s
		}
		s = t
	}
	return s + c
}

func fnSUMSQ(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	for i := range xs {
		xs[i] *= xs[i]
	}
	return numVal(kahanSum(xs))
}

func fnPRODUCT(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(xs) == 0 {
		return numVal(0)
	}
	s := 1.0
	for i := len(xs) - 1; i >= 0; i-- {
		s *= xs[i]
	}
	return Val{k: KNum, n: s}
}

func fnAVERAGE(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(xs) == 0 {
		return errVal(ErrDiv0)
	}
	return numVal(kahanSum(xs) / float64(len(xs)))
}

// MIN/MAX: LibreOffice format-type quirks. MIN takes the type of the first value of the
// first argument; MAX takes the type of the strict winner (args processed in reverse order).
func minmax(ctx *Ctx, args []Node, isMax bool) Val {
	best := Val{k: KEmpty}
	var errc int
	isBool := false
	typeSet := false
	consider := func(v Val) bool {
		if v.k == KErr {
			errc = int(v.n)
			return false
		}
		if v.k != KNum && v.k != KBool {
			return true
		}
		if !typeSet {
			isBool = v.k == KBool
			typeSet = true
		}
		if best.k == KEmpty {
			best = v
			return true
		}
		if isMax && v.n > best.n {
			best = v
			isBool = v.k == KBool
		} else if !isMax && v.n < best.n {
			best = v
		}
		return true
	}
	for ii := range args {
		i := ii
		if isMax {
			i = len(args) - 1 - ii
		}
		if args[i] == nil {
			continue
		}
		if !isMax && ii > 0 {
			typeSet = true // MIN: type fixed by first argument
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			if v.ref.r1 == v.ref.r2 && v.ref.c1 == v.ref.c2 {
				consider(deref(ctx, v))
			} else {
				forRange(ctx, v.ref, consider)
			}
		} else if v.k == KStr {
			return errVal(ErrValue)
		} else {
			consider(v)
		}
		if errc != 0 {
			return errVal(errc)
		}
	}
	if best.k == KEmpty {
		return numVal(0)
	}
	if isBool {
		return Val{k: KBool, n: best.n}
	}
	return Val{k: KNum, n: best.n}
}

func fnMIN(ctx *Ctx, args []Node) Val { return minmax(ctx, args, false) }
func fnMAX(ctx *Ctx, args []Node) Val { return minmax(ctx, args, true) }

func fnMEDIAN(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(xs) == 0 {
		return errVal(ErrValue)
	}
	sort.Float64s(xs)
	n := len(xs)
	if n%2 == 1 {
		return numVal(xs[n/2])
	}
	return numVal((xs[n/2-1] + xs[n/2]) / 2)
}

func fnLARGE(ctx *Ctx, args []Node) Val { return largeSmall(ctx, args, true) }
func fnSMALL(ctx *Ctx, args []Node) Val { return largeSmall(ctx, args, false) }

func largeSmall(ctx *Ctx, args []Node, large bool) Val {
	if len(args) < 2 {
		return errVal(ErrValue)
	}
	xs, e := collectNums(ctx, args[:1])
	if e != 0 {
		return errVal(e)
	}
	k, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	ki := int(math.Trunc(k))
	if ki < 1 || ki > len(xs) {
		return errVal(ErrValue)
	}
	sort.Float64s(xs)
	if large {
		return numVal(xs[len(xs)-ki])
	}
	return numVal(xs[ki-1])
}

func variance(xs []float64) float64 {
	n := float64(len(xs))
	m := 0.0
	for _, x := range xs {
		m += x
	}
	m /= n
	s := 0.0
	for _, x := range xs {
		s += (x - m) * (x - m)
	}
	return s / (n - 1)
}

func fnSTDEV(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(xs) < 2 {
		return errVal(ErrDiv0)
	}
	return numVal(math.Sqrt(variance(xs)))
}

func fnVAR(ctx *Ctx, args []Node) Val {
	xs, e := collectNums(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(xs) < 2 {
		return errVal(ErrDiv0)
	}
	return numVal(variance(xs))
}

func fnCOUNT(ctx *Ctx, args []Node) Val {
	n := 0
	for i := range args {
		if args[i] == nil {
			continue
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			forRange(ctx, v.ref, func(cv Val) bool {
				if cv.k == KNum || cv.k == KBool {
					n++
				}
				return true
			})
			continue
		}
		switch v.k {
		case KNum, KBool:
			n++
		case KStr:
			if _, ok := parseNumStr(v.s); ok {
				n++
			}
		}
	}
	return numVal(float64(n))
}

func fnCOUNTA(ctx *Ctx, args []Node) Val {
	n := 0
	for i := range args {
		if args[i] == nil {
			continue
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			forRange(ctx, v.ref, func(cv Val) bool {
				if cv.k != KEmpty {
					n++
				}
				return true
			})
			continue
		}
		n++
	}
	return numVal(float64(n))
}

func fnCOUNTBLANK(ctx *Ctx, args []Node) Val {
	n := 0
	v := argRaw(ctx, args, 0)
	if v.k != KRef {
		return errVal(ErrValue)
	}
	forRange(ctx, v.ref, func(cv Val) bool {
		if cv.k == KEmpty || (cv.k == KStr && cv.s == "") {
			n++
		}
		return true
	})
	return numVal(float64(n))
}

func rangeVals(ctx *Ctx, r Ref) []Val {
	out := make([]Val, 0, (r.r2-r.r1+1)*(r.c2-r.c1+1))
	forRange(ctx, r, func(v Val) bool {
		out = append(out, v)
		return true
	})
	return out
}

func fnSUMPRODUCT(ctx *Ctx, args []Node) Val {
	var arrs [][]Val
	for i := range args {
		v := argRaw(ctx, args, i)
		if v.k == KRef {
			arrs = append(arrs, rangeVals(ctx, v.ref))
		} else {
			if v.k == KErr {
				return v
			}
			arrs = append(arrs, []Val{v})
		}
	}
	if len(arrs) == 0 {
		return numVal(0)
	}
	n := len(arrs[0])
	for _, a := range arrs {
		if len(a) != n {
			return errVal(ErrValue)
		}
	}
	ps := make([]float64, n)
	for j := 0; j < n; j++ {
		p := 1.0
		for _, a := range arrs {
			v := a[j]
			if v.k == KErr {
				return v
			}
			if v.k == KNum || v.k == KBool {
				p *= v.n
			} else {
				p = 0
			}
		}
		ps[j] = p
	}
	return numVal(kahanSum(ps))
}

// ---------- criteria ----------

type crit struct {
	op          string
	isNum       bool
	num         float64
	str         string
	empty       bool
	hasWild     bool
	errc        int
	orig        string
	pat         []rune
	anyNonEmpty bool
}

func parseCrit(v Val) crit {
	var c crit
	c.op = "="
	switch v.k {
	case KNum, KBool:
		c.isNum = true
		c.num = v.n
		return c
	case KEmpty:
		c.empty = true
		return c
	case KErr:
		c.op = "err"
		c.errc = int(v.n)
		return c
	}
	s := v.s
	for _, op := range []string{"<>", "<=", ">=", "<", ">", "="} {
		if strings.HasPrefix(s, op) {
			c.op = op
			s = s[len(op):]
			break
		}
	}
	if s == "" {
		c.empty = true
		return c
	}
	if f, ok := parseNumStr(s); ok {
		c.isNum = true
		c.num = f
		return c
	}
	c.str = strings.ToLower(s)
	c.orig = s
	if s == "*" && (c.op == "=" || c.op == "<>") {
		c.anyNonEmpty = true
	}
	c.hasWild = strings.ContainsAny(s, "*?~")
	if c.hasWild {
		c.pat = []rune(c.str)
	}
	return c
}

func (c *crit) match(v Val) bool {
	if c.empty {
		if c.op == "<>" {
			return v.k != KEmpty
		}
		return v.k == KEmpty || (v.k == KStr && v.s == "")
	}
	if c.isNum {
		if v.k != KNum && v.k != KBool {
			return c.op == "<>"
		}
		return cmpOp(c.op, v.n, c.num)
	}
	if c.anyNonEmpty {
		if c.op == "=" {
			return v.k != KEmpty
		}
		return v.k == KEmpty
	}
	if v.k != KStr {
		if c.hasWild && v.k == KNum && (c.op == "=" || c.op == "<>") {
			m := wmStr(c.pat, fmtNum(v.n))
			if c.op == "<>" {
				return !m
			}
			return m
		}
		return c.op == "<>"
	}
	switch c.op {
	case "=":
		if c.hasWild {
			return wmStr(c.pat, v.s)
		}
		return strings.EqualFold(v.s, c.orig)
	case "<>":
		if c.hasWild {
			return !wmStr(c.pat, v.s)
		}
		return !strings.EqualFold(v.s, c.orig)
	}
	ls := strings.ToLower(v.s)
	cmp := strings.Compare(ls, c.str)
	return cmpOp(c.op, float64(cmp), 0)
}

func cmpOp(op string, a, b float64) bool {
	switch op {
	case "=":
		return a == b
	case "<>":
		return a != b
	case "<":
		return a < b
	case ">":
		return a > b
	case "<=":
		return a <= b
	case ">=":
		return a >= b
	}
	return false
}

func wildMatch(pat, s string) bool {
	p := []rune(pat)
	t := []rune(s)
	return wm(p, t)
}

func wmStr(p []rune, s string) bool {
	// fast path: ascii lowercase without allocation
	var buf [64]rune
	t := buf[:0]
	for _, r := range s {
		t = append(t, unicode.ToLower(r))
	}
	return wm(p, t)
}

func wm(p, t []rune) bool {
	for len(p) > 0 {
		switch p[0] {
		case '*':
			for i := 0; i <= len(t); i++ {
				if wm(p[1:], t[i:]) {
					return true
				}
			}
			return false
		case '?':
			if len(t) == 0 {
				return false
			}
			p, t = p[1:], t[1:]
		case '~':
			if len(p) > 1 {
				p = p[1:]
			}
			if len(t) == 0 || t[0] != p[0] {
				return false
			}
			p, t = p[1:], t[1:]
		default:
			if len(t) == 0 || t[0] != p[0] {
				return false
			}
			p, t = p[1:], t[1:]
		}
	}
	return len(t) == 0
}

type critRange struct {
	r Ref
	c crit
}

// generic *IFS: sumRange may be nil (count)
func ifsEval(ctx *Ctx, sumRef *Ref, pairs []critRange, mode int) Val {
	if len(pairs) == 0 {
		return errVal(ErrValue)
	}
	for _, p := range pairs {
		if p.c.op == "err" {
			return errVal(p.c.errc)
		}
	}
	nr := pairs[0].r.r2 - pairs[0].r.r1 + 1
	nc := pairs[0].r.c2 - pairs[0].r.c1 + 1
	for _, p := range pairs[1:] {
		if p.r.r2-p.r.r1+1 != nr || p.r.c2-p.r.c1+1 != nc {
			return errVal(ErrValue)
		}
	}
	sum := 0.0
	comp := 0.0
	cnt := 0
	book := ctx.book
	for i := 0; i < nr; i++ {
		for j := 0; j < nc; j++ {
			ok := true
			for k := range pairs {
				p := &pairs[k]
				v := book.cellValue(p.r.sheet, p.r.r1+i, p.r.c1+j)
				if !p.c.match(v) {
					ok = false
					break
				}
			}
			if !ok {
				continue
			}
			if mode == 0 {
				cnt++
				continue
			}
			v := book.cellValue(sumRef.sheet, sumRef.r1+i, sumRef.c1+j)
			if v.k == KErr {
				return v
			}
			if v.k == KNum || v.k == KBool {
				t := sum + v.n
				if math.Abs(sum) >= math.Abs(v.n) {
					comp += (sum - t) + v.n
				} else {
					comp += (v.n - t) + sum
				}
				sum = t
				cnt++
			}
		}
	}
	sum += comp
	switch mode {
	case 0:
		return numVal(float64(cnt))
	case 1:
		return numVal(sum)
	default:
		if cnt == 0 {
			return errVal(ErrDiv0)
		}
		return numVal(sum / float64(cnt))
	}
}

func getRef(ctx *Ctx, args []Node, i int) (Ref, bool) {
	v := argRaw(ctx, args, i)
	if v.k != KRef {
		return Ref{}, false
	}
	return v.ref, true
}

func fnCOUNTIF(ctx *Ctx, args []Node) Val {
	r, ok := getRef(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	c := parseCrit(arg(ctx, args, 1))
	return ifsEval(ctx, nil, []critRange{{r, c}}, 0)
}

func fnSUMIF(ctx *Ctx, args []Node) Val {
	r, ok := getRef(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	c := parseCrit(arg(ctx, args, 1))
	sr := r
	if len(args) > 2 && args[2] != nil {
		s, ok := getRef(ctx, args, 2)
		if !ok {
			return errVal(ErrValue)
		}
		sr = Ref{sheet: s.sheet, r1: s.r1, c1: s.c1, r2: s.r1 + (r.r2 - r.r1), c2: s.c1 + (r.c2 - r.c1)}
	}
	return ifsEval(ctx, &sr, []critRange{{r, c}}, 1)
}

func fnAVERAGEIF(ctx *Ctx, args []Node) Val {
	r, ok := getRef(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	c := parseCrit(arg(ctx, args, 1))
	sr := r
	if len(args) > 2 && args[2] != nil {
		s, ok := getRef(ctx, args, 2)
		if !ok {
			return errVal(ErrValue)
		}
		sr = Ref{sheet: s.sheet, r1: s.r1, c1: s.c1, r2: s.r1 + (r.r2 - r.r1), c2: s.c1 + (r.c2 - r.c1)}
	}
	return ifsEval(ctx, &sr, []critRange{{r, c}}, 2)
}

func ifsPairs(ctx *Ctx, args []Node, start int) ([]critRange, bool) {
	var pairs []critRange
	for i := start; i+1 < len(args); i += 2 {
		r, ok := getRef(ctx, args, i)
		if !ok {
			return nil, false
		}
		c := parseCrit(arg(ctx, args, i+1))
		pairs = append(pairs, critRange{r, c})
	}
	return pairs, true
}

func fnCOUNTIFS(ctx *Ctx, args []Node) Val {
	pairs, ok := ifsPairs(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	return ifsEval(ctx, nil, pairs, 0)
}

func fnSUMIFS(ctx *Ctx, args []Node) Val {
	sr, ok := getRef(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	pairs, ok := ifsPairs(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	return ifsEval(ctx, &sr, pairs, 1)
}

func fnAVERAGEIFS(ctx *Ctx, args []Node) Val {
	sr, ok := getRef(ctx, args, 0)
	if !ok {
		return errVal(ErrValue)
	}
	pairs, ok := ifsPairs(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	return ifsEval(ctx, &sr, pairs, 2)
}

// ---------- logical ----------

func fnIF(ctx *Ctx, args []Node) Val {
	c := arg(ctx, args, 0)
	if c.k == KErr {
		return c
	}
	b, e := toBool(c)
	if e != 0 {
		return errVal(e)
	}
	ctx.lastLogical = false
	if b {
		if len(args) < 2 {
			return boolVal(true)
		}
		if args[1] == nil {
			return numVal(0)
		}
		return argRaw(ctx, args, 1)
	}
	if len(args) < 3 {
		return boolVal(false)
	}
	if args[2] == nil {
		return numVal(0)
	}
	return argRaw(ctx, args, 2)
}

func fnIFERROR(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	ctx.lastLogical = false
	if v.k == KErr {
		return argRaw(ctx, args, 1)
	}
	return v
}

func fnIFNA(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	ctx.lastLogical = false
	if v.k == KErr && int(v.n) == ErrNA {
		return argRaw(ctx, args, 1)
	}
	return v
}

func fnIFS(ctx *Ctx, args []Node) Val {
	for i := 0; i+1 < len(args); i += 2 {
		c := arg(ctx, args, i)
		if c.k == KErr {
			return c
		}
		b, e := toBool(c)
		if e != 0 {
			return errVal(e)
		}
		ctx.lastLogical = false
		if b {
			return argRaw(ctx, args, i+1)
		}
	}
	return errVal(ErrNA)
}

func fnSWITCH(ctx *Ctx, args []Node) Val {
	x := arg(ctx, args, 0)
	if x.k == KErr {
		return x
	}
	ctx.lastLogical = false
	i := 1
	for ; i+1 < len(args); i += 2 {
		c := arg(ctx, args, i)
		if c.k == KErr {
			return c
		}
		match := false
		if x.k == KNum || x.k == KBool {
			cn, e := toNum(c)
			if e != 0 {
				return errVal(e)
			}
			match = approxEqual(x.n, cn)
		} else {
			cs, e := toStr(c)
			if e != 0 {
				return errVal(e)
			}
			xs, _ := toStr(x)
			match = strings.EqualFold(xs, cs)
		}
		if match {
			return argRaw(ctx, args, i+1)
		}
	}
	if i < len(args) {
		return argRaw(ctx, args, i)
	}
	return errVal(ErrNA)
}

func logicalCollect(ctx *Ctx, args []Node) ([]bool, int) {
	var out []bool
	var errc int
	for i := range args {
		if args[i] == nil {
			continue
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			if v.ref.r1 == v.ref.r2 && v.ref.c1 == v.ref.c2 {
				cv := deref(ctx, v)
				if cv.k == KErr {
					return nil, int(cv.n)
				}
				if cv.k == KNum || cv.k == KBool {
					out = append(out, cv.n != 0)
				}
				continue
			}
			forRange(ctx, v.ref, func(cv Val) bool {
				if cv.k == KErr {
					errc = int(cv.n)
					return false
				}
				if cv.k == KNum || cv.k == KBool {
					out = append(out, cv.n != 0)
				}
				return true
			})
			if errc != 0 {
				return nil, errc
			}
			continue
		}
		if v.k == KStr {
			return nil, ErrValue
		}
		b, e := toBool(v)
		if e != 0 {
			return nil, e
		}
		out = append(out, b)
	}
	return out, 0
}

func fnAND(ctx *Ctx, args []Node) Val {
	bs, e := logicalCollect(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(bs) == 0 {
		return errVal(ErrValue)
	}
	for _, b := range bs {
		if !b {
			return boolVal(false)
		}
	}
	return boolVal(true)
}

func fnOR(ctx *Ctx, args []Node) Val {
	bs, e := logicalCollect(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(bs) == 0 {
		return errVal(ErrValue)
	}
	for _, b := range bs {
		if b {
			return boolVal(true)
		}
	}
	return boolVal(false)
}

func fnXOR(ctx *Ctx, args []Node) Val {
	bs, e := logicalCollect(ctx, args)
	if e != 0 {
		return errVal(e)
	}
	if len(bs) == 0 {
		return errVal(ErrValue)
	}
	r := false
	for _, b := range bs {
		if b {
			r = !r
		}
	}
	return boolVal(r)
}

func fnNOT(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	b, e := toBool(v)
	if e != 0 {
		return errVal(e)
	}
	return boolVal(!b)
}

func fnCHOOSE(ctx *Ctx, args []Node) Val {
	k, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	ki := int(math.Trunc(k))
	ctx.lastLogical = false
	if ki < 1 || ki >= len(args) {
		return errVal(ErrValue)
	}
	return argRaw(ctx, args, ki)
}

// ---------- info ----------

func fnISBLANK(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	return boolVal(v.k == KEmpty)
}
func fnISERROR(ctx *Ctx, args []Node) Val { return boolVal(arg(ctx, args, 0).k == KErr) }
func fnISNA(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	return boolVal(v.k == KErr && int(v.n) == ErrNA)
}
func fnISNUMBER(ctx *Ctx, args []Node) Val {
	k := arg(ctx, args, 0).k
	return boolVal(k == KNum || k == KBool)
}
func fnISTEXT(ctx *Ctx, args []Node) Val { return boolVal(arg(ctx, args, 0).k == KStr) }
func fnISLOGICAL(ctx *Ctx, args []Node) Val {
	if len(args) > 0 {
		switch args[0].(type) {
		case *NumNode, *StrNode:
			return boolVal(ctx.lastLogical)
		}
	}
	return boolVal(arg(ctx, args, 0).k == KBool)
}

func fnN(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	switch v.k {
	case KNum, KBool:
		return numVal(v.n)
	case KErr:
		return v
	}
	return numVal(0)
}

func fnT(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	if v.k == KStr {
		return v
	}
	if v.k == KErr {
		return v
	}
	return strVal("")
}

// ---------- text ----------

func fnLEFT(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	n, e := argNumDef(ctx, args, 1, 1)
	if e != 0 {
		return errVal(e)
	}
	if n < 0 {
		return errVal(ErrValue)
	}
	r := []rune(s)
	k := int(n)
	if k > len(r) {
		k = len(r)
	}
	return strVal(string(r[:k]))
}

func fnRIGHT(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	n, e := argNumDef(ctx, args, 1, 1)
	if e != 0 {
		return errVal(e)
	}
	if n < 0 {
		return errVal(ErrValue)
	}
	r := []rune(s)
	k := int(n)
	if k > len(r) {
		k = len(r)
	}
	return strVal(string(r[len(r)-k:]))
}

func fnMID(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	st, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	n, e := argNum(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	if st < 1 || n < 0 {
		return errVal(ErrValue)
	}
	r := []rune(s)
	a := int(st) - 1
	if a > len(r) {
		return strVal("")
	}
	b := a + int(n)
	if b > len(r) {
		b = len(r)
	}
	return strVal(string(r[a:b]))
}

func fnLEN(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	return numVal(float64(utf8.RuneCountInString(s)))
}

func fnLOWER(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	return strVal(strings.ToLower(s))
}

func fnUPPER(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	return strVal(strings.ToUpper(s))
}

func fnPROPER(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	var b strings.Builder
	prevLetter := false
	for _, r := range s {
		if unicode.IsLetter(r) {
			if prevLetter {
				b.WriteRune(unicode.ToLower(r))
			} else {
				b.WriteRune(unicode.ToUpper(r))
			}
			prevLetter = true
		} else {
			b.WriteRune(r)
			prevLetter = false
		}
	}
	return strVal(b.String())
}

func fnTRIM(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	return strVal(strings.Join(strings.Fields(s), " "))
}

func fnREPT(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	n, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	if n < 0 {
		return errVal(ErrValue)
	}
	return strVal(strings.Repeat(s, int(n)))
}

func fnSUBSTITUTE(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	old, e := argStr(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	nw, e := argStr(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	if old == "" {
		return strVal(s)
	}
	if len(args) > 3 && args[3] != nil {
		n, e := argNum(ctx, args, 3)
		if e != 0 {
			return errVal(e)
		}
		k := int(n)
		if k < 1 {
			return errVal(ErrValue)
		}
		idx := 0
		pos := 0
		for {
			i := strings.Index(s[pos:], old)
			if i < 0 {
				return strVal(s)
			}
			idx++
			if idx == k {
				return strVal(s[:pos+i] + nw + s[pos+i+len(old):])
			}
			pos += i + len(old)
		}
	}
	return strVal(strings.ReplaceAll(s, old, nw))
}

func fnREPLACE(ctx *Ctx, args []Node) Val {
	s, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	st, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	n, e := argNum(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	nw, e := argStr(ctx, args, 3)
	if e != 0 {
		return errVal(e)
	}
	if st < 1 || n < 0 {
		return errVal(ErrValue)
	}
	r := []rune(s)
	a := int(st) - 1
	if a > len(r) {
		a = len(r)
	}
	b := a + int(n)
	if b > len(r) {
		b = len(r)
	}
	return strVal(string(r[:a]) + nw + string(r[b:]))
}

func findSearch(ctx *Ctx, args []Node, caseSens bool) Val {
	needle, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	hay, e := argStr(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	st, e := argNumDef(ctx, args, 2, 1)
	if e != 0 {
		return errVal(e)
	}
	hr := []rune(hay)
	start := int(st) - 1
	if start < 0 || start > len(hr) {
		return errVal(ErrValue)
	}
	if needle == "" {
		return errVal(ErrValue)
	}
	nr := []rune(needle)
	if !caseSens {
		nr = []rune(strings.ToLower(needle))
		hr = []rune(strings.ToLower(hay))
		if strings.ContainsAny(needle, "*?~") {
			// wildcard search: find first position where pattern matches a prefix of remaining
			for i := start; i < len(hr); i++ {
				for j := i; j <= len(hr); j++ {
					if wm(nr, hr[i:j]) {
						return numVal(float64(i + 1))
					}
				}
			}
			return errVal(ErrValue)
		}
	}
	for i := start; i+len(nr) <= len(hr); i++ {
		ok := true
		for j := range nr {
			if hr[i+j] != nr[j] {
				ok = false
				break
			}
		}
		if ok {
			return numVal(float64(i + 1))
		}
	}
	return errVal(ErrValue)
}

func fnFIND(ctx *Ctx, args []Node) Val   { return findSearch(ctx, args, true) }
func fnSEARCH(ctx *Ctx, args []Node) Val { return findSearch(ctx, args, false) }

func fnEXACT(ctx *Ctx, args []Node) Val {
	a, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	b, e := argStr(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	return boolVal(a == b)
}

func concatAll(ctx *Ctx, args []Node, skipEmpty bool, delim string) Val {
	var parts []string
	var errc int
	add := func(v Val) bool {
		if v.k == KErr {
			errc = int(v.n)
			return false
		}
		s, _ := toStr(v)
		if skipEmpty && s == "" {
			return true
		}
		parts = append(parts, s)
		return true
	}
	for i := range args {
		if args[i] == nil {
			continue
		}
		v := eval(ctx, args[i])
		if v.k == KRef {
			forRange(ctx, v.ref, add)
		} else {
			add(v)
		}
		if errc != 0 {
			return errVal(errc)
		}
	}
	return strVal(strings.Join(parts, delim))
}

func fnCONCAT(ctx *Ctx, args []Node) Val { return concatAll(ctx, args, false, "") }

func fnCONCATENATE(ctx *Ctx, args []Node) Val {
	var b strings.Builder
	for i := range args {
		s, e := argStr(ctx, args, i)
		if e != 0 {
			return errVal(e)
		}
		b.WriteString(s)
	}
	return strVal(b.String())
}

func fnTEXTJOIN(ctx *Ctx, args []Node) Val {
	d, e := argStr(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	skip, e := toBool(arg(ctx, args, 1))
	if e != 0 {
		return errVal(e)
	}
	if len(args) < 3 {
		return errVal(ErrValue)
	}
	return concatAll(ctx, args[2:], skip, d)
}

func fnVALUE(ctx *Ctx, args []Node) Val {
	raw := argRaw(ctx, args, 0)
	v := deref(ctx, raw)
	if v.k == KErr || (v.k == KBool && raw.k != KRef) {
		return v
	}
	if v.k == KStr {
		f, ok := parseNumStr(v.s)
		if !ok {
			return errVal(ErrValue)
		}
		return numVal(f)
	}
	f, e := toNum(v)
	if e != 0 {
		return errVal(e)
	}
	return numVal(f)
}

func fnTEXT(ctx *Ctx, args []Node) Val {
	v := arg(ctx, args, 0)
	if v.k == KErr {
		return v
	}
	f, e := argStr(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	if v.k == KStr {
		x, ok := parseNumStr(v.s)
		if !ok {
			return v
		}
		v = numVal(x)
	}
	x, _ := toNum(v)
	return strVal(formatNumber(x, f))
}

// ---------- dates ----------

func fnDATE(ctx *Ctx, args []Node) Val {
	y, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	m, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	d, e := argNum(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	yi, mi, di := int(math.Trunc(y)), int(math.Trunc(m)), int(math.Trunc(d))
	if yi >= 0 && yi < 100 {
		yi += 1900
	}
	t := yi*12 + (mi - 1)
	ny := t / 12
	nm := t%12 + 1
	if t < 0 {
		ny = (t - 11) / 12
		nm = t - ny*12 + 1
	}
	s := daysFromCivil(ny, nm, 1) + int64(di) - 1
	return numVal(float64(s))
}

func dateArg(ctx *Ctx, args []Node, i int) (int64, int) {
	f, e := argNum(ctx, args, i)
	if e != 0 {
		return 0, e
	}
	return int64(math.Floor(f)), 0
}

func fnYEAR(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	y, _, _ := civilFromDays(d)
	return numVal(float64(y))
}

func fnMONTH(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	_, m, _ := civilFromDays(d)
	return numVal(float64(m))
}

func fnDAY(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	_, _, dd := civilFromDays(d)
	return numVal(float64(dd))
}

func fnDAYS(ctx *Ctx, args []Node) Val {
	a, e := argNum(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	b, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	return numVal(a - b)
}

func fnEDATE(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	m, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	return numVal(float64(addMonths(d, int(math.Trunc(m)))))
}

func fnEOMONTH(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	m, e := argNum(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	y, mm, _ := civilFromDays(d)
	t := y*12 + (mm - 1) + int(math.Trunc(m))
	ny := t / 12
	nm := t%12 + 1
	if t < 0 {
		ny = (t - 11) / 12
		nm = t - ny*12 + 1
	}
	return numVal(float64(daysFromCivil(ny, nm, daysInMonth(ny, nm))))
}

func fnWEEKDAY(ctx *Ctx, args []Node) Val {
	d, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	t, e := argNumDef(ctx, args, 1, 1)
	if e != 0 {
		return errVal(e)
	}
	// serial 0 = 1899-12-30 = Saturday
	dow := int(((d % 7) + 7) % 7) // 0=Sat,1=Sun,...,6=Fri
	switch int(t) {
	case 1:
		return numVal(float64((dow+6)%7 + 1)) // Sun=1
	case 2:
		return numVal(float64((dow+5)%7 + 1)) // Mon=1
	case 3:
		return numVal(float64((dow + 5) % 7)) // Mon=0
	}
	return errVal(ErrValue)
}

func fnDATEDIF(ctx *Ctx, args []Node) Val {
	a, e := dateArg(ctx, args, 0)
	if e != 0 {
		return errVal(e)
	}
	b, e := dateArg(ctx, args, 1)
	if e != 0 {
		return errVal(e)
	}
	u, e := argStr(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	if a > b {
		return errVal(ErrValue)
	}
	y1, m1, d1 := civilFromDays(a)
	y2, m2, d2 := civilFromDays(b)
	switch strings.ToUpper(u) {
	case "D":
		return numVal(float64(b - a))
	case "M":
		months := (y2-y1)*12 + (m2 - m1)
		if d2 < d1 {
			months--
		}
		return numVal(float64(months))
	case "Y":
		years := y2 - y1
		if m2 < m1 || (m2 == m1 && d2 < d1) {
			years--
		}
		return numVal(float64(years))
	case "MD":
		if d2 >= d1 {
			return numVal(float64(d2 - d1))
		}
		pm := m2 - 1
		py := y2
		if pm < 1 {
			pm = 12
			py--
		}
		return numVal(float64(d2 + daysInMonth(py, pm) - d1))
	case "YM":
		months := m2 - m1
		if d2 < d1 {
			months--
		}
		if months < 0 {
			months += 12
		}
		return numVal(float64(months))
	case "YD":
		sy := y2
		if m2 < m1 || (m2 == m1 && d2 < d1) {
			sy = y2 - 1
		}
		s := daysFromCivil(sy, m1, d1)
		return numVal(float64(b - s))
	}
	return errVal(ErrValue)
}

// ---------- lookup ----------

func valsEqual(a, b Val) bool {
	an := a.k == KNum || a.k == KBool
	bn := b.k == KNum || b.k == KBool
	if an && bn {
		return a.n == b.n
	}
	if a.k == KStr && b.k == KStr {
		return strings.EqualFold(a.s, b.s)
	}
	if a.k == KEmpty && b.k == KEmpty {
		return true
	}
	return false
}

// returns index of match in vector or -1
func lookupExact(key Val, vec []Val) int {
	if key.k == KStr {
		if strings.ContainsAny(key.s, "*?~") {
			pat := []rune(strings.ToLower(key.s))
			for i, v := range vec {
				if v.k == KStr && wmStr(pat, v.s) {
					return i
				}
				if v.k == KNum && wmStr(pat, fmtNum(v.n)) {
					return i
				}
			}
			return -1
		}
		if n, ok := parseNumStr(key.s); ok {
			nk := numVal(n)
			for i, v := range vec {
				if valsEqual(key, v) || valsEqual(nk, v) {
					return i
				}
			}
			return -1
		}
	}
	for i, v := range vec {
		if valsEqual(key, v) {
			return i
		}
	}
	return -1
}

// largest value <= key (assumes sorted ascending); -1 if none
func lookupApprox(key Val, vec []Val) int {
	best := -1
	for i, v := range vec {
		if v.k == KEmpty {
			continue
		}
		c := compareVals(v, key)
		if c <= 0 {
			if (v.k == KStr) == (key.k == KStr) {
				best = i
			}
		} else {
			break
		}
	}
	return best
}

func fnVLOOKUP(ctx *Ctx, args []Node) Val {
	key := arg(ctx, args, 0)
	if key.k == KErr {
		return key
	}
	r, ok := getRef(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	col, e := argNum(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	sorted := true
	if len(args) > 3 && args[3] != nil {
		b, e := toBool(arg(ctx, args, 3))
		if e != 0 {
			return errVal(e)
		}
		sorted = b
	}
	ci := int(col)
	if ci < 1 || ci > r.c2-r.c1+1 {
		return errVal(ErrValue)
	}
	keyCol := Ref{sheet: r.sheet, r1: r.r1, c1: r.c1, r2: r.r2, c2: r.c1}
	var idx int
	if sorted {
		idx = lookupApprox(key, rangeVals(ctx, keyCol))
	} else {
		idx = exactFind(ctx, key, keyCol)
	}
	if idx < 0 {
		return errVal(ErrNA)
	}
	res := ctx.book.cellValue(r.sheet, r.r1+idx, r.c1+ci-1)
	if res.k == KBool {
		res.k = KNum
	}
	return res
}

func fnHLOOKUP(ctx *Ctx, args []Node) Val {
	key := arg(ctx, args, 0)
	if key.k == KErr {
		return key
	}
	r, ok := getRef(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	row, e := argNum(ctx, args, 2)
	if e != 0 {
		return errVal(e)
	}
	sorted := true
	if len(args) > 3 && args[3] != nil {
		b, e := toBool(arg(ctx, args, 3))
		if e != 0 {
			return errVal(e)
		}
		sorted = b
	}
	ri := int(row)
	if ri < 1 || ri > r.r2-r.r1+1 {
		return errVal(ErrValue)
	}
	keyRow := Ref{sheet: r.sheet, r1: r.r1, c1: r.c1, r2: r.r1, c2: r.c2}
	var idx int
	if sorted {
		idx = lookupApprox(key, rangeVals(ctx, keyRow))
	} else {
		idx = exactFind(ctx, key, keyRow)
	}
	if idx < 0 {
		return errVal(ErrNA)
	}
	res := ctx.book.cellValue(r.sheet, r.r1+ri-1, r.c1+idx)
	if res.k == KBool {
		res.k = KNum
	}
	return res
}

func fnMATCH(ctx *Ctx, args []Node) Val {
	key := arg(ctx, args, 0)
	if key.k == KErr {
		return key
	}
	r, ok := getRef(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	mt, e := argNumDef(ctx, args, 2, 1)
	if e != 0 {
		return errVal(e)
	}
	if r.r1 != r.r2 && r.c1 != r.c2 {
		return errVal(ErrValue)
	}
	idx := -1
	if mt == 0 {
		idx = exactFind(ctx, key, r)
		if idx < 0 {
			return errVal(ErrNA)
		}
		return numVal(float64(idx + 1))
	}
	vec := rangeVals(ctx, r)
	switch {
	case mt == 0:
		idx = lookupExact(key, vec)
	case mt > 0:
		idx = lookupApprox(key, vec)
	default:
		for i, v := range vec {
			if v.k == KEmpty {
				continue
			}
			if compareVals(v, key) >= 0 {
				idx = i
			} else {
				break
			}
		}
	}
	if idx < 0 {
		return errVal(ErrNA)
	}
	return numVal(float64(idx + 1))
}

func fnINDEX(ctx *Ctx, args []Node) Val {
	r, ok := getRef(ctx, args, 0)
	if !ok {
		v := arg(ctx, args, 0)
		if v.k == KErr {
			return v
		}
		return errVal(ErrValue)
	}
	row, e := argNumDef(ctx, args, 1, 0)
	if e != 0 {
		return errVal(e)
	}
	col, e := argNumDef(ctx, args, 2, 0)
	if e != 0 {
		return errVal(e)
	}
	ri, ci := int(row), int(col)
	nr := r.r2 - r.r1 + 1
	nc := r.c2 - r.c1 + 1
	if len(args) < 3 || args[2] == nil {
		if nr == 1 && nc > 1 {
			ci = ri
			ri = 1
		} else if nc == 1 {
			ci = 1
		} else {
			return errVal(ErrValue)
		}
	}
	if ri == 0 || ci == 0 {
		if ri == 0 && nr == 1 {
			ri = 1
		} else if ci == 0 && nc == 1 {
			ci = 1
		} else {
			return errVal(ErrValue)
		}
	}
	if ri < 0 || ci < 0 {
		return errVal(ErrValue)
	}
	if ri > nr || ci > nc {
		return errVal(ErrRef)
	}
	return ctx.book.cellValue(r.sheet, r.r1+ri-1, r.c1+ci-1)
}

func fnXLOOKUP(ctx *Ctx, args []Node) Val {
	key := arg(ctx, args, 0)
	if key.k == KErr {
		return key
	}
	lr, ok := getRef(ctx, args, 1)
	if !ok {
		return errVal(ErrValue)
	}
	rr, ok := getRef(ctx, args, 2)
	if !ok {
		return errVal(ErrValue)
	}
	if (rr.r1 != rr.r2 && rr.c1 != rr.c2) || (lr.r1 != lr.r2 && lr.c1 != lr.c2) {
		return errVal(ErrValue)
	}
	mode, e := argNumDef(ctx, args, 4, 0)
	if e != 0 {
		return errVal(e)
	}
	smode, e := argNumDef(ctx, args, 5, 1)
	if e != 0 {
		return errVal(e)
	}
	idx := -1
	if int(mode) == 0 && smode > 0 {
		if key.k == KStr && strings.ContainsAny(key.s, "*?~") {
			idx = -1
			for i, v := range rangeVals(ctx, lr) {
				if valsEqual(key, v) {
					idx = i
					break
				}
			}
		} else {
			idx = exactFind(ctx, key, lr)
		}
		if idx < 0 {
			if len(args) > 3 && args[3] != nil {
				return arg(ctx, args, 3)
			}
			return errVal(ErrNA)
		}
		if rr.r2-rr.r1 >= rr.c2-rr.c1 {
			return ctx.book.cellValue(rr.sheet, rr.r1+idx, rr.c1)
		}
		return ctx.book.cellValue(rr.sheet, rr.r1, rr.c1+idx)
	}
	vec := rangeVals(ctx, lr)
	n := len(vec)
	order := make([]int, n)
	for i := range order {
		if smode < 0 {
			order[i] = n - 1 - i
		} else {
			order[i] = i
		}
	}
	switch int(mode) {
	case 0:
		for _, i := range order {
			if valsEqual(key, vec[i]) {
				idx = i
				break
			}
		}
	case 2:
		ks := strings.ToLower(key.s)
		for _, i := range order {
			v := vec[i]
			if key.k == KStr && v.k == KStr {
				if wildMatch(ks, strings.ToLower(v.s)) {
					idx = i
					break
				}
			} else if valsEqual(key, v) {
				idx = i
				break
			}
		}
	case -1, 1:
		// exact or next smaller/larger
		var bestV Val
		for _, i := range order {
			v := vec[i]
			if v.k == KEmpty || (v.k == KStr) != (key.k == KStr) {
				continue
			}
			c := compareVals(v, key)
			if c == 0 {
				idx = i
				break
			}
			if int(mode) == -1 && c < 0 {
				if idx < 0 || compareVals(v, bestV) > 0 {
					idx = i
					bestV = v
				}
			} else if int(mode) == 1 && c > 0 {
				if idx < 0 || compareVals(v, bestV) < 0 {
					idx = i
					bestV = v
				}
			}
		}
	}
	if idx < 0 {
		if len(args) > 3 && args[3] != nil {
			return arg(ctx, args, 3)
		}
		return errVal(ErrNA)
	}
	// return array: map index into return range
	if rr.r2-rr.r1 >= rr.c2-rr.c1 {
		return ctx.book.cellValue(rr.sheet, rr.r1+idx, rr.c1)
	}
	return ctx.book.cellValue(rr.sheet, rr.r1, rr.c1+idx)
}
