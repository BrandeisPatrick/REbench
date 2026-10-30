package main

import (
	"math"
	"strings"
)

type Ctx struct {
	book        *Book
	sh          *Sheet
	r, c        int
	lastLogical bool
}

func (b *Book) cellValue(sh *Sheet, r, c int) Value {
	cl := sh.at(r, c)
	if cl == nil {
		return Value{k: KEmpty}
	}
	switch cl.k {
	case KEmpty:
		return Value{k: KEmpty}
	case KNum:
		return Value{k: KNum, n: cl.n}
	case KStr:
		return Value{k: KStr, s: cl.s}
	case KBool:
		return Value{k: KBool, n: cl.n}
	case KErr:
		return Value{k: KErr, s: cl.s}
	case KFormula:
		if cl.state == 2 {
			return cl.val
		}
		if cl.state == 1 {
			return ErrValue // circular
		}
		cl.state = 1
		if cl.f == nil {
			cl.f = parseFormula(cl.ftxt, b)
		}
		ctx := Ctx{book: b, sh: sh, r: r, c: c}
		v := ctx.eval(cl.f)
		v = ctx.finalize(v)
		cl.val = v
		cl.state = 2
		return v
	}
	return Value{k: KEmpty}
}

// finalize converts a formula result into a plain scalar
func (ctx *Ctx) finalize(v Value) Value {
	switch v.k {
	case KRange:
		rg := v.rg
		if rg.r1 == rg.r2 && rg.c1 == rg.c2 {
			v = ctx.book.cellValue(rg.sh, rg.r1, rg.c1)
		} else {
			// implicit intersection (by position, regardless of sheet, as LibreOffice does)
			if rg.c1 == rg.c2 && ctx.r >= rg.r1 && ctx.r <= rg.r2 {
				v = ctx.book.cellValue(rg.sh, ctx.r, rg.c1)
			} else if rg.r1 == rg.r2 && ctx.c >= rg.c1 && ctx.c <= rg.c2 {
				v = ctx.book.cellValue(rg.sh, rg.r1, ctx.c)
			} else {
				return ErrValue
			}
		}
	case KArray:
		if len(v.ar.v) == 0 {
			return ErrValue
		}
		v = v.ar.v[0]
	}
	if v.k == KEmpty {
		return Num(0)
	}
	return v
}

// deref: single-cell ranges -> cell value; multi -> implicit intersection or error; arrays -> first
func (ctx *Ctx) deref(v Value) Value {
	switch v.k {
	case KRange:
		rg := v.rg
		if rg.r1 == rg.r2 && rg.c1 == rg.c2 {
			cv := ctx.book.cellValue(rg.sh, rg.r1, rg.c1)
			ctx.lastLogical = cv.k == KBool
			return cv
		}
		if rg.c1 == rg.c2 && ctx.r >= rg.r1 && ctx.r <= rg.r2 {
			return ctx.book.cellValue(rg.sh, ctx.r, rg.c1)
		}
		if rg.r1 == rg.r2 && ctx.c >= rg.c1 && ctx.c <= rg.c2 {
			return ctx.book.cellValue(rg.sh, rg.r1, ctx.c)
		}
		return ErrValue
	case KArray:
		if len(v.ar.v) == 0 {
			return ErrValue
		}
		return v.ar.v[0]
	}
	return v
}

func (ctx *Ctx) toNum(v Value) (float64, Value) {
	v = ctx.deref(v)
	switch v.k {
	case KNum, KBool:
		return v.n, Value{}
	case KEmpty:
		return 0, Value{}
	case KStr:
		if f, ok := parseNumStr(v.s); ok {
			return f, Value{}
		}
		return 0, ErrValue
	case KErr:
		return 0, v
	}
	return 0, ErrValue
}

func (ctx *Ctx) toStr(v Value) (string, Value) {
	v = ctx.deref(v)
	switch v.k {
	case KNum:
		return fmtNum(v.n), Value{}
	case KBool:
		if v.n != 0 {
			return "1", Value{}
		}
		return "0", Value{}
	case KEmpty:
		return "", Value{}
	case KStr:
		return v.s, Value{}
	case KErr:
		return "", v
	}
	return "", ErrValue
}

func (ctx *Ctx) toBool(v Value) (bool, Value) {
	f, e := ctx.toNum(v)
	if e.k == KErr {
		return false, e
	}
	return f != 0, Value{}
}

func (ctx *Ctx) toInt(v Value) (int, Value) {
	f, e := ctx.toNum(v)
	if e.k == KErr {
		return 0, e
	}
	if math.IsNaN(f) || f > 1e15 || f < -1e15 {
		return 0, ErrValue
	}
	return int(approxFloor(f)), Value{}
}

func (ctx *Ctx) eval(n *Node) Value {
	switch n.op {
	case NNum:
		return Num(n.num)
	case NStr:
		return Str(n.str)
	case NBool:
		return Value{k: KBool, n: n.num}
	case NErr:
		return mkErr(n.str)
	case NRef:
		var sh *Sheet
		if n.sheet == -1 {
			sh = ctx.sh
		} else if n.sheet == -2 {
			return ErrRef
		} else {
			sh = ctx.book.sheets[n.sheet]
		}
		return Value{k: KRange, rg: &Range{sh: sh, r1: n.r1, c1: n.c1, r2: n.r2, c2: n.c2}}
	case NUnm:
		v := ctx.eval(n.args[0])
		return ctx.mapScalar(v, func(x Value) Value {
			f, e := ctx.toNum(x)
			if e.k == KErr {
				return e
			}
			return Num(-f)
		})
	case NPct:
		v := ctx.eval(n.args[0])
		return ctx.mapScalar(v, func(x Value) Value {
			f, e := ctx.toNum(x)
			if e.k == KErr {
				return e
			}
			return Num(f / 100)
		})
	case NBin:
		if n.cached {
			return n.cv
		}
		l := ctx.eval(n.args[0])
		r := ctx.eval(n.args[1])
		v := ctx.binary(n.bop, l, r)
		switch n.bop {
		case '=', 'n', '<', '>', 'l', 'g':
			ctx.lastLogical = true
		default:
			ctx.lastLogical = false
		}
		n.cv = v
		n.cached = true
		return v
	case NFunc:
		if n.cached {
			return n.cv
		}
		v := ctx.callFunc(n)
		ctx.lastLogical = n.isLogical
		n.cv = v
		n.cached = true
		return v
	}
	return ErrValue
}

func (ctx *Ctx) isMulti(v Value) bool {
	if v.k == KArray {
		return true
	}
	if v.k == KRange {
		return !(v.rg.r1 == v.rg.r2 && v.rg.c1 == v.rg.c2)
	}
	return false
}

func (ctx *Ctx) toArray(v Value) *Array {
	if v.k == KArray {
		return v.ar
	}
	if v.k == KRange {
		rg := v.rg
		rows := rg.r2 - rg.r1 + 1
		cols := rg.c2 - rg.c1 + 1
		a := &Array{rows: rows, cols: cols, v: make([]Value, rows*cols)}
		for i := 0; i < rows; i++ {
			for j := 0; j < cols; j++ {
				a.v[i*cols+j] = ctx.book.cellValue(rg.sh, rg.r1+i, rg.c1+j)
			}
		}
		return a
	}
	return &Array{rows: 1, cols: 1, v: []Value{v}}
}

func (ctx *Ctx) mapScalar(v Value, f func(Value) Value) Value {
	if ctx.isMulti(v) {
		a := ctx.toArray(v)
		out := &Array{rows: a.rows, cols: a.cols, v: make([]Value, len(a.v))}
		for i, x := range a.v {
			out.v[i] = f(x)
		}
		return Value{k: KArray, ar: out}
	}
	return f(ctx.deref(v))
}

func (ctx *Ctx) binary(op byte, l, r Value) Value {
	if ctx.isMulti(l) || ctx.isMulti(r) {
		la := ctx.toArray(l)
		ra := ctx.toArray(r)
		rows := la.rows
		if ra.rows > rows {
			rows = ra.rows
		}
		cols := la.cols
		if ra.cols > cols {
			cols = ra.cols
		}
		out := &Array{rows: rows, cols: cols, v: make([]Value, rows*cols)}
		for i := 0; i < rows; i++ {
			for j := 0; j < cols; j++ {
				var lv, rv Value
				if la.rows == 1 && la.cols == 1 {
					lv = la.v[0]
				} else if i < la.rows && j < la.cols {
					lv = la.v[i*la.cols+j]
				} else if la.rows == 1 && j < la.cols {
					lv = la.v[j]
				} else if la.cols == 1 && i < la.rows {
					lv = la.v[i]
				} else {
					out.v[i*cols+j] = ErrNA
					continue
				}
				if ra.rows == 1 && ra.cols == 1 {
					rv = ra.v[0]
				} else if i < ra.rows && j < ra.cols {
					rv = ra.v[i*ra.cols+j]
				} else if ra.rows == 1 && j < ra.cols {
					rv = ra.v[j]
				} else if ra.cols == 1 && i < ra.rows {
					rv = ra.v[i]
				} else {
					out.v[i*cols+j] = ErrNA
					continue
				}
				out.v[i*cols+j] = ctx.binScalar(op, lv, rv)
			}
		}
		return Value{k: KArray, ar: out}
	}
	lr, rr := l.k == KRange, r.k == KRange
	ld, rd := ctx.deref(l), ctx.deref(r)
	if rd.k == KStr && ld.k == KErr {
		switch op {
		case '+', '-', '*', '/', '^':
			if _, ok := parseNumStr(rd.s); !ok {
				rd = ErrValue
			}
		}
	}
	if ld.k == KErr || rd.k == KErr {
		// emulate LibreOffice pop order: right first; ref errors only set if none yet, computed errors overwrite
		var g Value
		if rd.k == KErr {
			g = rd
		}
		if ld.k == KErr {
			if !lr || g.k != KErr {
				g = ld
			}
		}
		_ = rr
		return g
	}
	return ctx.binScalar(op, ld, rd)
}

func checkNum(f float64) Value {
	if math.IsInf(f, 0) || math.IsNaN(f) {
		return ErrNum
	}
	return Num(f)
}

func (ctx *Ctx) binScalar(op byte, l, r Value) Value {
	if r.k == KErr {
		return r
	}
	if l.k == KErr {
		return l
	}
	switch op {
	case '+', '-', '*', '/', '^':
		b, e := ctx.toNum(r)
		if e.k == KErr {
			return e
		}
		a, e := ctx.toNum(l)
		if e.k == KErr {
			return e
		}
		switch op {
		case '+':
			return checkNum(a + b)
		case '-':
			return checkNum(a - b)
		case '*':
			return checkNum(a * b)
		case '/':
			if b == 0 {
				return ErrDiv0
			}
			return checkNum(a / b)
		case '^':
			return powVal(a, b)
		}
	case '&':
		b, e := ctx.toStr(r)
		if e.k == KErr {
			return e
		}
		a, e := ctx.toStr(l)
		if e.k == KErr {
			return e
		}
		return Str(a + b)
	default:
		c := compareValues(l, r)
		switch op {
		case '=':
			return Bool(c == 0)
		case 'n':
			return Bool(c != 0)
		case '<':
			return Bool(c < 0)
		case '>':
			return Bool(c > 0)
		case 'l':
			return Bool(c <= 0)
		case 'g':
			return Bool(c >= 0)
		}
	}
	return ErrValue
}

func powVal(a, b float64) Value {
	if a == 0 && b == 0 {
		return Num(1)
	}
	if a < 0 && b != math.Floor(b) {
		inv := 1 / b
		ri := math.Round(inv)
		if approxEqual(inv, ri) && math.Mod(math.Abs(ri), 2) == 1 {
			return checkNum(-math.Pow(-a, b))
		}
	}
	return checkNum(math.Pow(a, b))
}

// generic comparison: numbers(bools, empty=0) < strings; strings case-insensitive
func compareValues(l, r Value) int {
	ln := l.k == KNum || l.k == KBool || l.k == KEmpty
	rn := r.k == KNum || r.k == KBool || r.k == KEmpty
	if ln && rn {
		if approxEqual(l.n, r.n) {
			return 0
		}
		if l.n < r.n {
			return -1
		}
		return 1
	}
	if l.k == KStr && r.k == KStr {
		return cmpStrCI(l.s, r.s)
	}
	if l.k == KEmpty && r.k == KStr {
		if r.s == "" {
			return 0
		}
		return -1
	}
	if l.k == KStr && r.k == KEmpty {
		if l.s == "" {
			return 0
		}
		return 1
	}
	if ln {
		return -1
	}
	return 1
}

func cmpStrCI(a, b string) int {
	// fast path ASCII
	i, j := 0, 0
	for i < len(a) && j < len(b) {
		ca, cb := a[i], b[j]
		if ca < 0x80 && cb < 0x80 {
			la, lb := lowerByte(ca), lowerByte(cb)
			if la != lb {
				if la < lb {
					return -1
				}
				return 1
			}
			i++
			j++
			continue
		}
		return strings.Compare(strings.ToLower(a[i:]), strings.ToLower(b[j:]))
	}
	if i < len(a) {
		return 1
	}
	if j < len(b) {
		return -1
	}
	return 0
}

func lowerByte(c byte) byte {
	if c >= 'A' && c <= 'Z' {
		return c + 32
	}
	return c
}

func eqStrCI(a, b string) bool {
	if len(a) != len(b) {
		// may still be equal with non-ascii; fall back
		return strings.EqualFold(a, b)
	}
	for i := 0; i < len(a); i++ {
		if lowerByte(a[i]) != lowerByte(b[i]) {
			if a[i] >= 0x80 || b[i] >= 0x80 {
				return strings.EqualFold(a, b)
			}
			return false
		}
	}
	return true
}

// wildcard match (case-insensitive), * ? and ~ escape
func wildMatch(pat, s string) bool {
	ascii := true
	for i := 0; i < len(pat); i++ {
		if pat[i] >= 0x80 {
			ascii = false
			break
		}
	}
	if ascii {
		for i := 0; i < len(s); i++ {
			if s[i] >= 0x80 {
				ascii = false
				break
			}
		}
	}
	if ascii {
		return wmb(pat, s)
	}
	p := []rune(strings.ToLower(pat))
	t := []rune(strings.ToLower(s))
	return wm(p, t)
}

// byte-based ASCII wildcard matcher, case-insensitive, no allocation
func wmb(p, t string) bool {
	for len(p) > 0 {
		switch p[0] {
		case '*':
			for len(p) > 0 && p[0] == '*' {
				p = p[1:]
			}
			if len(p) == 0 {
				return true
			}
			for i := 0; i <= len(t); i++ {
				if wmb(p, t[i:]) {
					return true
				}
			}
			return false
		case '?':
			if len(t) == 0 {
				return false
			}
			p = p[1:]
			t = t[1:]
		case '~':
			if len(p) > 1 {
				p = p[1:]
			}
			if len(t) == 0 || lowerByte(t[0]) != lowerByte(p[0]) {
				return false
			}
			p = p[1:]
			t = t[1:]
		default:
			if len(t) == 0 || lowerByte(t[0]) != lowerByte(p[0]) {
				return false
			}
			p = p[1:]
			t = t[1:]
		}
	}
	return len(t) == 0
}

func wm(p, t []rune) bool {
	for len(p) > 0 {
		switch p[0] {
		case '*':
			for len(p) > 0 && p[0] == '*' {
				p = p[1:]
			}
			if len(p) == 0 {
				return true
			}
			for i := 0; i <= len(t); i++ {
				if wm(p, t[i:]) {
					return true
				}
			}
			return false
		case '?':
			if len(t) == 0 {
				return false
			}
			p = p[1:]
			t = t[1:]
		case '~':
			if len(p) > 1 {
				p = p[1:]
			}
			if len(t) == 0 || t[0] != p[0] {
				return false
			}
			p = p[1:]
			t = t[1:]
		default:
			if len(t) == 0 || t[0] != p[0] {
				return false
			}
			p = p[1:]
			t = t[1:]
		}
	}
	return len(t) == 0
}

func hasWild(s string) bool {
	return strings.ContainsAny(s, "*?~")
}

// string form used for criteria/lookup matching of a cell
func cellDisplay(v Value) string {
	switch v.k {
	case KNum:
		return fmtNum(v.n)
	case KBool:
		if v.n != 0 {
			return "TRUE"
		}
		return "FALSE"
	case KStr:
		return v.s
	}
	return ""
}
