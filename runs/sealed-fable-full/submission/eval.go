package main

import (
	"math"
	"strconv"
	"strings"
)

var lazyFuncs = map[string]bool{"IF": true, "IFS": true, "SWITCH": true, "CHOOSE": true, "IFERROR": true, "IFNA": true,
	"ISLOGICAL": true, "ISERROR": true, "ISNA": true, "COUNT": true, "COUNTA": true, "XLOOKUP": true, "AND": true, "OR": true, "XOR": true}

type Ctx struct {
	book        *Book
	sh          int
	lastLogical bool
}

func (b *Book) cellValue(sheet, r, c int) Val {
	sh := b.sheets[sheet]
	cell := sh.get(r, c)
	if cell == nil {
		return emptyVal
	}
	if cell.state == 2 {
		return cell.v
	}
	if cell.state == 1 {
		return errVal(ErrValue) // circular
	}
	cell.state = 1
	if cell.ast == nil {
		cell.ast = parseFormula(cell.formula, b, sheet)
	}
	ctx := &Ctx{book: b, sh: sheet}
	v := safeEval(ctx, cell.ast)
	v = deref(ctx, v)
	if v.k == KEmpty {
		v = numVal(0)
	} else if v.k == KNum {
		v = numVal(v.n)
	}
	cell.v = v
	cell.state = 2
	return v
}

func safeEval(ctx *Ctx, n Node) (v Val) {
	defer func() {
		if r := recover(); r != nil {
			v = errVal(ErrValue)
		}
	}()
	return eval(ctx, n)
}

// deref converts a reference to a scalar value.
func deref(ctx *Ctx, v Val) Val {
	if v.k != KRef {
		return v
	}
	r := v.ref
	if r.r1 == r.r2 && r.c1 == r.c2 {
		return ctx.book.cellValue(r.sheet, r.r1, r.c1)
	}
	// implicit intersection not supported
	return errVal(ErrValue)
}

func eval(ctx *Ctx, n Node) Val {
	v := eval0(ctx, n)
	switch n.(type) {
	case *BinNode, *UnNode, *FuncNode:
		if v.k == KRef {
			ctx.lastLogical = deref(ctx, v).k == KBool
		} else {
			ctx.lastLogical = v.k == KBool
		}
	}
	return v
}

func eval0(ctx *Ctx, n Node) Val {
	switch x := n.(type) {
	case *ValNode:
		return x.v
	case *NumNode:
		return numVal(x.v)
	case *StrNode:
		return strVal(x.s)
	case *BoolNode:
		return boolVal(x.b)
	case *ErrNode:
		return errVal(x.e)
	case *RefNode:
		return Val{k: KRef, ref: x.ref}
	case *UnNode:
		v := deref(ctx, eval(ctx, x.x))
		if v.k == KErr {
			return v
		}
		f, e := toNum(v)
		if e != 0 {
			return errVal(e)
		}
		if x.op == "-" {
			return numVal(-f)
		}
		return numVal(f / 100)
	case *BinNode:
		l := eval(ctx, x.l)
		if l.k == KErr {
			return l
		}
		r := eval(ctx, x.r)
		if r.k == KErr {
			return r
		}
		r = deref(ctx, r)
		if r.k == KErr {
			return r
		}
		l = deref(ctx, l)
		if l.k == KErr {
			// right operand is converted first in LibreOffice
			switch x.op {
			case "+", "-", "*", "/", "^":
				if _, e := toNum(r); e != 0 {
					return errVal(e)
				}
			case "&":
				if _, e := toStr(r); e != 0 {
					return errVal(e)
				}
			}
			return l
		}
		return binop(x.op, l, r)
	case *FuncNode:
		if len(x.args) >= 2 && !lazyFuncs[x.name] {
			vals := make([]Node, len(x.args))
			store := make([]ValNode, len(x.args))
			var firstErr Val
			for i, a := range x.args {
				if a == nil {
					continue
				}
				v := eval(ctx, a)
				if v.k == KErr {
					if firstErr.k != KErr {
						firstErr = v
					}
				} else if fn, ok := a.(*FuncNode); ok && (fn.name == "IFS" || fn.name == "SWITCH") {
					firstErr = Val{}
				}
				store[i].v = v
				vals[i] = &store[i]
			}
			if firstErr.k == KErr {
				return firstErr
			}
			for i := len(vals) - 1; i >= 0; i-- {
				if vals[i] == nil {
					continue
				}
				vn := vals[i].(*ValNode)
				if vn.v.k == KErr {
					return vn.v
				}
				if vn.v.k == KRef && vn.v.ref.r1 == vn.v.ref.r2 && vn.v.ref.c1 == vn.v.ref.c2 {
					d := deref(ctx, vn.v)
					if d.k == KErr {
						return d
					}
				}
			}
			return x.fn(ctx, vals)
		}
		return x.fn(ctx, x.args)
	}
	return errVal(ErrName)
}

func binop(op string, l, r Val) Val {
	switch op {
	case "&":
		b, e := toStr(r)
		if e != 0 {
			return errVal(e)
		}
		a, e := toStr(l)
		if e != 0 {
			return errVal(e)
		}
		return strVal(a + b)
	case "=", "<>", "<", ">", "<=", ">=":
		c := compareVals(l, r)
		switch op {
		case "=":
			return boolVal(c == 0)
		case "<>":
			return boolVal(c != 0)
		case "<":
			return boolVal(c < 0)
		case ">":
			return boolVal(c > 0)
		case "<=":
			return boolVal(c <= 0)
		case ">=":
			return boolVal(c >= 0)
		}
	}
	b, e := toNum(r)
	if e != 0 {
		return errVal(e)
	}
	a, e := toNum(l)
	if e != 0 {
		return errVal(e)
	}
	switch op {
	case "+":
		return numVal(a + b)
	case "-":
		return numVal(a - b)
	case "*":
		return numVal(a * b)
	case "/":
		if b == 0 {
			return errVal(ErrDiv0)
		}
		return numVal(a / b)
	case "^":
		return powVal(a, b)
	}
	return errVal(ErrName)
}

func powVal(a, b float64) Val {
	if a == 0 && b < 0 {
		return errVal(ErrNum)
	}
	if a == 0 && b == 0 {
		return numVal(1)
	}
	r := math.Pow(a, b)
	if math.IsNaN(r) || math.IsInf(r, 0) {
		// negative base with fractional exponent: try odd root
		if a < 0 {
			inv := 1 / b
			if math.Abs(inv-math.Round(inv)) < 1e-9 && int64(math.Round(inv))%2 != 0 {
				return numVal(-math.Pow(-a, b))
			}
		}
		return errVal(ErrNum)
	}
	return numVal(r)
}

// compareVals: numbers < strings; strings case-insensitive. Empty = 0 and "".
func compareVals(l, r Val) int {
	lnum := l.k == KNum || l.k == KBool
	rnum := r.k == KNum || r.k == KBool
	if l.k == KEmpty {
		if r.k == KStr {
			return cmpStr("", r.s)
		}
		lnum = true
		l.n = 0
	}
	if r.k == KEmpty {
		if l.k == KStr {
			return cmpStr(l.s, "")
		}
		rnum = true
		r.n = 0
	}
	if lnum && rnum {
		if approxEqual(l.n, r.n) {
			return 0
		}
		if l.n < r.n {
			return -1
		}
		return 1
	}
	if lnum {
		return -1
	}
	if rnum {
		return 1
	}
	return cmpStr(l.s, r.s)
}

func approxEqual(a, b float64) bool {
	if a == b {
		return true
	}
	x := a - b
	if x < 0 {
		x = -x
	}
	aa := a
	if aa < 0 {
		aa = -aa
	}
	return x < aa*(1.0/(16777216.0*16777216.0))
}

func cmpStr(a, b string) int {
	if a == b {
		return 0
	}
	la := strings.ToLower(a)
	lb := strings.ToLower(b)
	if la == lb {
		return 0
	}
	if la < lb {
		return -1
	}
	return 1
}

func toNum(v Val) (float64, int) {
	switch v.k {
	case KNum, KBool:
		return v.n, 0
	case KEmpty:
		return 0, 0
	case KErr:
		return 0, int(v.n)
	case KStr:
		f, ok := parseNumStr(v.s)
		if !ok {
			return 0, ErrValue
		}
		return f, 0
	}
	return 0, ErrValue
}

func parseNumStr(s string) (float64, bool) {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0, false
	}
	f, err := strconv.ParseFloat(s, 64)
	if err == nil && !math.IsInf(f, 0) && !math.IsNaN(f) {
		// reject things like "inf", "nan", hex
		c := s[0]
		if isDigit(c) || c == '-' || c == '+' || c == '.' {
			return f, true
		}
		return 0, false
	}
	// percent
	if strings.HasSuffix(s, "%") {
		f, err := strconv.ParseFloat(strings.TrimSpace(s[:len(s)-1]), 64)
		if err == nil {
			return f / 100, true
		}
	}
	// thousands separators (groups of three)
	if strings.Contains(s, ",") {
		t := strings.TrimPrefix(s, "$")
		parts := strings.Split(strings.SplitN(t, ".", 2)[0], ",")
		okGroups := true
		for i, p := range parts {
			if i > 0 && len(p) != 3 {
				okGroups = false
			}
		}
		if okGroups {
			f, err := strconv.ParseFloat(strings.ReplaceAll(t, ",", ""), 64)
			if err == nil {
				return f, true
			}
		}
	}
	// date yyyy-mm-dd
	if len(s) == 10 && s[4] == '-' && s[7] == '-' {
		y, e1 := strconv.Atoi(s[0:4])
		m, e2 := strconv.Atoi(s[5:7])
		d, e3 := strconv.Atoi(s[8:10])
		if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m) {
			return float64(daysFromCivil(y, m, d)), true
		}
	}
	// yyyy-m-d with optional time
	if strings.Count(s, "-") == 2 && len(s) >= 8 && isDigit(s[0]) {
		rest := ""
		ds := s
		if i := strings.IndexByte(s, ' '); i > 0 {
			ds, rest = s[:i], strings.TrimSpace(s[i+1:])
		}
		p := strings.Split(ds, "-")
		if len(p) == 3 {
			y, e1 := strconv.Atoi(p[0])
			m, e2 := strconv.Atoi(p[1])
			d, e3 := strconv.Atoi(p[2])
			if e1 == nil && e2 == nil && e3 == nil && len(p[0]) == 4 && m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m) {
				f := float64(daysFromCivil(y, m, d))
				if rest != "" {
					t, ok := parseDateStr(rest)
					if !ok || t >= 1 {
						return 0, false
					}
					f += t
				}
				return f, true
			}
		}
	}
	if f, ok := parseDateStr(s); ok {
		return f, true
	}
	// TRUE/FALSE
	switch strings.ToUpper(s) {
	case "TRUE":
		return 1, true
	case "FALSE":
		return 0, true
	}
	return 0, false
}

func toStr(v Val) (string, int) {
	switch v.k {
	case KStr:
		return v.s, 0
	case KNum:
		if math.IsInf(v.n, 0) || math.IsNaN(v.n) {
			return "", ErrNum
		}
		return fmtNum(v.n), 0
	case KBool:
		if v.n != 0 {
			return "1", 0
		}
		return "0", 0
	case KEmpty:
		return "", 0
	case KErr:
		return "", int(v.n)
	}
	return "", ErrValue
}

func toBool(v Val) (bool, int) {
	f, e := toNum(v)
	return f != 0, e
}

// General number formatting as LibreOffice does for string conversion.
func fmtNum(v float64) string {
	if v == 0 {
		return "0"
	}
	if math.IsInf(v, 0) || math.IsNaN(v) {
		return "#NUM!"
	}
	av := math.Abs(v)
	if v == math.Trunc(v) && av < 1e16 {
		return strconv.FormatFloat(v, 'f', 0, 64)
	}
	exp := int(math.Floor(math.Log10(av)))
	if exp >= 16 || exp < -16 {
		s := strconv.FormatFloat(v, 'E', 14, 64)
		// mantissa trim
		i := strings.IndexByte(s, 'E')
		mant := s[:i]
		ex := s[i+1:]
		if strings.Contains(mant, ".") {
			mant = strings.TrimRight(mant, "0")
			mant = strings.TrimRight(mant, ".")
		}
		sign := ex[0]
		digits := strings.TrimLeft(ex[1:], "0")
		for len(digits) < 3 {
			digits = "0" + digits
		}
		return mant + "E" + string(sign) + digits
	}
	dec := 14 - exp
	if dec < 0 {
		dec = 0
	}
	scaled := av * math.Pow(10, float64(dec))
	ri := math.Floor(scaled + 0.5)
	digits := strconv.FormatFloat(ri, 'f', 0, 64)
	var s string
	if dec > 0 {
		for len(digits) <= dec {
			digits = "0" + digits
		}
		s = digits[:len(digits)-dec] + "." + digits[len(digits)-dec:]
	} else {
		s = digits
	}
	if v < 0 {
		s = "-" + s
	}
	if strings.Contains(s, ".") {
		s = strings.TrimRight(s, "0")
		s = strings.TrimRight(s, ".")
	}
	if s == "-0" {
		return "0"
	}
	return s
}

// round half away from zero with 15-digit correction
func roundTo(v float64, digits int) float64 {
	if v == 0 {
		return 0
	}
	p := math.Pow(10, float64(digits))
	x := v * p
	// correct representation error
	s := strconv.FormatFloat(x, 'g', 15, 64)
	x2, _ := strconv.ParseFloat(s, 64)
	var r float64
	if x2 < 0 {
		r = -math.Floor(-x2 + 0.5)
	} else {
		r = math.Floor(x2 + 0.5)
	}
	res := r / p
	if digits > 0 {
		// re-parse to avoid artifacts
		s2 := strconv.FormatFloat(res, 'f', digits, 64)
		res, _ = strconv.ParseFloat(s2, 64)
	}
	return res
}

// ---------- dates ----------

func daysFromCivil(y, m, d int) int64 {
	if m <= 2 {
		y--
	}
	var era int64
	if y >= 0 {
		era = int64(y) / 400
	} else {
		era = (int64(y) - 399) / 400
	}
	yoe := int64(y) - era*400
	mm := int64(m)
	if m > 2 {
		mm -= 3
	} else {
		mm += 9
	}
	doy := (153*mm+2)/5 + int64(d) - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	days := era*146097 + doe - 719468 // days since 1970-01-01
	return days + 25569               // serial: 1899-12-30 = 0
}

func civilFromDays(serial int64) (y, m, d int) {
	z := serial - 25569 + 719468
	var era int64
	if z >= 0 {
		era = z / 146097
	} else {
		era = (z - 146096) / 146097
	}
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	yy := yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	d = int(doy - (153*mp+2)/5 + 1)
	if mp < 10 {
		m = int(mp + 3)
	} else {
		m = int(mp - 9)
	}
	if m <= 2 {
		yy++
	}
	y = int(yy)
	return
}

func daysInMonth(y, m int) int {
	switch m {
	case 1, 3, 5, 7, 8, 10, 12:
		return 31
	case 4, 6, 9, 11:
		return 30
	}
	if (y%4 == 0 && y%100 != 0) || y%400 == 0 {
		return 29
	}
	return 28
}

func addMonths(serial int64, months int) int64 {
	y, m, d := civilFromDays(serial)
	t := y*12 + (m - 1) + months
	ny := t / 12
	nm := t%12 + 1
	if t < 0 {
		ny = (t - 11) / 12
		nm = t - ny*12 + 1
	}
	dim := daysInMonth(ny, nm)
	if d > dim {
		d = dim
	}
	return daysFromCivil(ny, nm, d)
}

var monAbbr = map[string]int{"jan": 1, "feb": 2, "mar": 3, "apr": 4, "may": 5, "jun": 6, "jul": 7, "aug": 8, "sep": 9, "oct": 10, "nov": 11, "dec": 12}

func fixYear(y int, digits int) int {
	if digits <= 2 {
		if y < 30 {
			return 2000 + y
		}
		return 1900 + y
	}
	return y
}

func parseDateStr(s string) (float64, bool) {
	if strings.HasPrefix(s, "$") {
		f, err := strconv.ParseFloat(s[1:], 64)
		if err == nil {
			return f, true
		}
	}
	// hh:mm[:ss]
	if p := strings.Split(s, ":"); len(p) == 2 || len(p) == 3 {
		h, e1 := strconv.Atoi(p[0])
		m, e2 := strconv.Atoi(p[1])
		sec := 0
		var e3 error
		if len(p) == 3 {
			sec, e3 = strconv.Atoi(p[2])
		}
		if e1 == nil && e2 == nil && e3 == nil && m >= 0 && m < 60 {
			return float64(h*3600+m*60+sec) / 86400, true
		}
	}
	// m/d/y
	if p := strings.Split(s, "/"); len(p) == 3 {
		m, e1 := strconv.Atoi(p[0])
		d, e2 := strconv.Atoi(p[1])
		y, e3 := strconv.Atoi(p[2])
		if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
			return float64(daysFromCivil(fixYear(y, len(p[2])), m, d)), true
		}
	}
	// d-mmm-yy or d-mmm-yyyy
	if p := strings.Split(s, "-"); len(p) == 3 {
		d, e1 := strconv.Atoi(p[0])
		m, ok := monAbbr[strings.ToLower(p[1])]
		if !ok && len(p[1]) > 3 {
			m, ok = monAbbr[strings.ToLower(p[1][:3])]
		}
		y, e3 := strconv.Atoi(p[2])
		if e1 == nil && ok && e3 == nil && d >= 1 && d <= 31 {
			return float64(daysFromCivil(fixYear(y, len(p[2])), m, d)), true
		}
	}
	// Mon d, yyyy
	if f := strings.Fields(strings.ReplaceAll(s, ",", " ")); len(f) == 3 {
		m, ok := monAbbr[strings.ToLower(f[0])]
		if !ok && len(f[0]) > 3 {
			m, ok = monAbbr[strings.ToLower(f[0][:3])]
		}
		d, e2 := strconv.Atoi(f[1])
		y, e3 := strconv.Atoi(f[2])
		if ok && e2 == nil && e3 == nil && d >= 1 && d <= 31 {
			return float64(daysFromCivil(fixYear(y, len(f[2])), m, d)), true
		}
	}
	return 0, false
}
