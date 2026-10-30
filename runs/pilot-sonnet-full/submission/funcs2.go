package main

import (
	"math"
	"strings"
)

type idxKey struct{}
type lookupIdx struct{}

func lookupEq(x Val, cell Val, wild *crit) bool {
	switch x.k {
	case KNum, KBool:
		return (cell.k == KNum || cell.k == KBool) && approxEqual(cell.n, x.n)
	case KStr:
		if cell.k == KNum || cell.k == KBool {
			return lowerStr(x.s) == fmtNum(cell.n)
		}
		if cell.k != KStr {
			return false
		}
		if wild != nil && wild.hasWld {
			return wildMatch(wild.wild, []rune(lowerStr(cell.s)))
		}
		return lowerStr(cell.s) == lowerStr(x.s)
	}
	return false
}

func normLookup(v Val) Val {
	if v.k == KEmpty {
		return mkNum(0)
	}
	return v
}

func wildCrit(x Val) *crit {
	if x.k == KStr && isWild(x.s) {
		c := crit{op: '=', hasWld: true}
		c.str = lowerStr(x.s)
		c.wild = []rune(c.str)
		return &c
	}
	return nil
}

// find position in a 1-D sequence of n values accessed by get; type 0 exact, 1 asc approx, -1 desc approx
func findPos(x Val, n int, get func(i int) Val, mode int) int {
	if mode == 0 {
		w := wildCrit(x)
		for i := 0; i < n; i++ {
			if lookupEq(x, get(i), w) {
				return i
			}
		}
		return -1
	}
	best := -1
	for i := 0; i < n; i++ {
		c := get(i)
		if c.k == KEmpty || c.k == KErr {
			continue
		}
		if (c.k == KStr) != (x.k == KStr) {
			continue
		}
		d := cmpVals(c, x)
		if mode == 1 {
			if d <= 0 {
				best = i
			} else {
				break
			}
		} else {
			if d >= 0 {
				best = i
			} else {
				break
			}
		}
	}
	return best
}

func fVLookup(vertical bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		if len(a) < 3 {
			return mkErr(eVal)
		}
		x := cx.eval(a[0])
		rg, ok := nodeRng(a[1])
		if !ok {
			return mkErr(eVal)
		}
		ci, e := cx.scalarNum(a[2])
		if e.k == KErr {
			return e
		}
		{
			lim0 := rg.cols()
			if !vertical {
				lim0 = rg.rows()
			}
			if int(ci) < 1 || int(ci) > lim0 {
				return mkErr(eVal)
			}
		}
		if x.k == KErr {
			return x
		}
		x = normLookup(x)
		approx := true
		if len(a) > 3 {
			b, e := toBool(cx.eval(a[3]))
			if e.k == KErr {
				return e
			}
			approx = b
		}
		idx := int(ci)
		if idx < 1 {
			return mkErr(eVal)
		}
		var n, lim int
		if vertical {
			n, lim = rg.rows(), rg.cols()
		} else {
			n, lim = rg.cols(), rg.rows()
		}
		if idx > lim {
			return mkErr(eVal)
		}
		get := func(i int) Val {
			if vertical {
				return cx.at(rg, i, 0)
			}
			return cx.at(rg, 0, i)
		}
		mode := 0
		if approx {
			mode = 1
		}
		var p int
		if mode == 0 {
			p = cx.findExact(x, rg, !vertical, n, get)
		} else {
			p = findPos(x, n, get, mode)
		}
		if p < 0 {
			return mkErr(eNA)
		}
		var rv Val
		if vertical {
			rv = cx.at(rg, p, idx-1)
		} else {
			rv = cx.at(rg, idx-1, p)
		}
		if rv.k == KBool {
			rv.k = KNum
		}
		return rv
	}
}

func fMatch(cx *Ctx, a []*Node) Val {
	if len(a) < 2 {
		return mkErr(eVal)
	}
	x := cx.eval(a[0])
	if x.k == KErr {
		return x
	}
	x = normLookup(x)
	rg, ok := nodeRng(a[1])
	if !ok {
		return mkErr(eNA)
	}
	mt := 1.0
	if len(a) > 2 {
		var e Val
		mt, e = cx.scalarNum(a[2])
		if e.k == KErr {
			return e
		}
	}
	var n int
	var get func(i int) Val
	if rg.cols() == 1 {
		n = rg.rows()
		get = func(i int) Val { return cx.at(rg, i, 0) }
	} else if rg.rows() == 1 {
		n = rg.cols()
		get = func(i int) Val { return cx.at(rg, 0, i) }
	} else {
		return mkErr(eNA)
	}
	mode := 0
	if mt > 0 {
		mode = 1
	} else if mt < 0 {
		mode = -1
	}
	var p int
	if mode == 0 {
		p = cx.findExact(x, rg, rg.cols() != 1, n, get)
	} else {
		p = findPos(x, n, get, mode)
	}
	if p < 0 {
		return mkErr(eNA)
	}
	return mkNum(float64(p + 1))
}

func fIndex(cx *Ctx, a []*Node) Val {
	if len(a) < 2 {
		return mkErr(eVal)
	}
	rg, ok := nodeRng(a[0])
	if !ok {
		v := cx.eval(a[0])
		return v
	}
	ri, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	ci := 1.0
	hasC := false
	if len(a) > 2 {
		ci, e = cx.scalarNum(a[2])
		if e.k == KErr {
			return e
		}
		hasC = true
	}
	r, c := int(ri), int(ci)
	if !hasC && rg.rows() == 1 && rg.cols() > 1 {
		c = r
		r = 1
	}
	if r < 0 || c < 0 {
		return mkErr(eVal)
	}
	if c > rg.cols() || r > rg.rows() {
		return mkErr(eRef)
	}
	if r == 0 && rg.rows() == 1 {
		r = 1
	}
	if c == 0 && rg.cols() == 1 {
		c = 1
	}
	if r == 0 && c == 0 {
		return mkErr(eVal)
	}
	if r == 0 {
		row := cx.r - rg.r1
		if row < 0 || row >= rg.rows() {
			return mkErr(eVal)
		}
		return cx.at(rg, row, c-1)
	}
	if c == 0 {
		col := cx.c - rg.c1
		if col < 0 || col >= rg.cols() {
			return mkErr(eVal)
		}
		return cx.at(rg, r-1, col)
	}
	if r > rg.rows() || c > rg.cols() {
		return mkErr(eRef)
	}
	return cx.at(rg, r-1, c-1)
}

func fXLookup(cx *Ctx, a []*Node) Val {
	if len(a) < 3 {
		return mkErr(eVal)
	}
	x := cx.eval(a[0])
	lr, ok := nodeRng(a[1])
	if !ok {
		return mkErr(eVal)
	}
	rr, ok := nodeRng(a[2])
	if !ok {
		return mkErr(eVal)
	}
	if (lr.cols() == 1 && rr.rows() != lr.rows()) || (lr.cols() != 1 && lr.rows() == 1 && rr.cols() != lr.cols()) || (lr.cols() != 1 && lr.rows() != 1) {
		return mkErr(eVal)
	}
	if x.k == KErr {
		return x
	}
	x = normLookup(x)
	mm, sm := 0, 1
	if len(a) > 4 {
		f, e := cx.scalarNum(a[4])
		if e.k == KErr {
			return e
		}
		mm = int(f)
	}
	if len(a) > 5 {
		f, e := cx.scalarNum(a[5])
		if e.k == KErr {
			return e
		}
		sm = int(f)
	}
	var n int
	var get func(i int) Val
	var ret func(i int) Val
	if lr.cols() == 1 {
		n = lr.rows()
		get = func(i int) Val { return cx.at(lr, i, 0) }
		if rr.rows() != n {
			return mkErr(eVal)
		}
		ret = func(i int) Val { return cx.at(rr, i, 0) }
	} else if lr.rows() == 1 {
		n = lr.cols()
		get = func(i int) Val { return cx.at(lr, 0, i) }
		if rr.cols() != n {
			return mkErr(eVal)
		}
		ret = func(i int) Val { return cx.at(rr, 0, i) }
	} else {
		return mkErr(eVal)
	}
	pos := -1
	var w *crit
	if mm == 2 {
		w = wildCrit(x)
	}
	order := func(f func(i int) bool) {
		if sm == -1 || sm == -2 {
			for i := n - 1; i >= 0; i-- {
				if f(i) {
					return
				}
			}
		} else {
			for i := 0; i < n; i++ {
				if f(i) {
					return
				}
			}
		}
	}
	if mm == 0 && sm == 1 {
		pos = cx.findExact(x, lr, lr.cols() != 1, n, get)
	} else {
		order(func(i int) bool {
			if lookupEq(x, get(i), w) {
				pos = i
				return true
			}
			return false
		})
	}
	if pos < 0 && (mm == -1 || mm == 1) {
		best := -1
		order(func(i int) bool {
			c := get(i)
			if c.k == KEmpty || c.k == KErr || (c.k == KStr) != (x.k == KStr) {
				return false
			}
			d := cmpVals(c, x)
			if (mm == -1 && d < 0) || (mm == 1 && d > 0) {
				if best < 0 {
					best = i
				} else {
					bd := cmpVals(c, get(best))
					if (mm == -1 && bd > 0) || (mm == 1 && bd < 0) {
						best = i
					}
				}
			}
			return false
		})
		pos = best
	}
	if pos < 0 {
		if len(a) > 3 && !(a[3].op == NConst && a[3].val.k == KEmpty) {
			return cx.eval(a[3])
		}
		return mkErr(eNA)
	}
	return ret(pos)
}

// ---- dates ----
func daysFromCivil(y, m, d int) int {
	if m <= 2 {
		y--
	}
	var era int
	if y >= 0 {
		era = y / 400
	} else {
		era = (y - 399) / 400
	}
	yoe := y - era*400
	mp := (m + 9) % 12
	doy := (153*mp+2)/5 + d - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	return era*146097 + doe - 719468
}

func civilFromDays(z int) (int, int, int) {
	z += 719468
	var era int
	if z >= 0 {
		era = z / 146097
	} else {
		era = (z - 146096) / 146097
	}
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	y := yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	d := doy - (153*mp+2)/5 + 1
	m := mp + 3
	if m > 12 {
		m -= 12
	}
	if m <= 2 {
		y++
	}
	return y, m, d
}

const serialOffset = 25569

func dateSerial(y, m, d int) int {
	// normalize month
	mm := y*12 + (m - 1)
	y = int(math.Floor(float64(mm) / 12))
	m = mm - y*12 + 1
	return daysFromCivil(y, m, 1) + (d - 1) + serialOffset
}

func serialCivil(s int) (int, int, int) { return civilFromDays(s - serialOffset) }

func daysInMonth(y, m int) int {
	return daysFromCivil(y, m+1, 1) - daysFromCivil(y, m, 1)
}

func fDate(cx *Ctx, a []*Node) Val {
	if len(a) < 3 {
		return mkErr(eVal)
	}
	var v [3]int
	for i := 0; i < 3; i++ {
		f, e := cx.scalarNum(a[i])
		if e.k == KErr {
			return e
		}
		if f > 32767.99 || f < -32768.99 {
			return mkErr(eVal)
		}
		v[i] = int(math.Trunc(f))
	}
	y := v[0]
	if y < 0 {
		return mkErr(eVal)
	}
	if y < 30 {
		y += 2000
	} else if y < 100 {
		y += 1900
	} else if y < 1583 {
		return mkErr(eVal)
	}
	return mkNum(float64(dateSerial(y, v[1], v[2])))
}

func fDatePart(part int) Fn {
	return func(cx *Ctx, a []*Node) Val {
		f, e := cx.scalarNum(a[0])
		if e.k == KErr {
			return e
		}
		if math.Abs(f) > 4e9 {
			return mkErr(eVal)
		}
		y, m, d := serialCivil(int(math.Floor(f)))
		switch part {
		case 0:
			return mkNum(float64(y))
		case 1:
			return mkNum(float64(m))
		}
		return mkNum(float64(d))
	}
}

func weekday(serial int, t int) int {
	// serial 0 = Saturday (1899-12-30). sunday-based 0..6: 
	s := ((serial-1)%7 + 7) % 7 // 0 = Sunday for serial 1
	switch t {
	case 1:
		return s + 1
	case 2:
		return (s+6)%7 + 1
	case 3:
		return (s + 6) % 7
	case 11, 12, 13, 14, 15, 16, 17:
		// 11: Mon=1 ... 17: Sun=1
		start := t - 10 // Monday=1 .. Sunday=7
		// day index with Monday=1..Sunday=7
		dm := (s+6)%7 + 1
		return ((dm-start)%7+7)%7 + 1
	}
	return -1
}

func fWeekday(cx *Ctx, a []*Node) Val {
	f, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	t := 1
	if len(a) > 1 {
		tf, e := cx.scalarNum(a[1])
		if e.k == KErr {
			return e
		}
		t = int(tf)
	}
	w := weekday(int(math.Floor(f)), t)
	if w < 0 {
		return mkErr(eVal)
	}
	return mkNum(float64(w))
}

func fEdate(eom bool) Fn {
	return func(cx *Ctx, a []*Node) Val {
		f, e := cx.scalarNum(a[0])
		if e.k == KErr {
			return e
		}
		mf, e := cx.scalarNum(a[1])
		if e.k == KErr {
			return e
		}
		if math.Abs(f) > 4e9 || math.Abs(mf) > 4e7 {
			return mkErr(eVal)
		}
		y, m, d := serialCivil(int(math.Floor(f)))
		mm := y*12 + (m - 1) + int(math.Trunc(mf))
		y = int(math.Floor(float64(mm) / 12))
		m = mm - y*12 + 1
		dim := daysInMonth(y, m)
		if eom {
			d = dim
		} else if d > dim {
			d = dim
		}
		return mkNum(float64(daysFromCivil(y, m, d) + serialOffset))
	}
}

func fDays(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	y, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	return mkNum(x - y)
}

func fDatedif(cx *Ctx, a []*Node) Val {
	x, e := cx.scalarNum(a[0])
	if e.k == KErr {
		return e
	}
	y, e := cx.scalarNum(a[1])
	if e.k == KErr {
		return e
	}
	u, e := cx.scalarStr(a[2])
	if e.k == KErr {
		return e
	}
	if math.Abs(x) > 4e9 || math.Abs(y) > 4e9 {
		return mkErr(eVal)
	}
	s1, s2 := int(math.Floor(x)), int(math.Floor(y))
	if s2 < s1 {
		return mkErr(eVal)
	}
	y1, m1, d1 := serialCivil(s1)
	y2, m2, d2 := serialCivil(s2)
	switch strings.ToUpper(u) {
	case "D":
		return mkNum(float64(s2 - s1))
	case "M":
		n := (y2-y1)*12 + (m2 - m1)
		if d2 < d1 {
			n--
		}
		return mkNum(float64(n))
	case "Y":
		n := y2 - y1
		if m2 < m1 || (m2 == m1 && d2 < d1) {
			n--
		}
		return mkNum(float64(n))
	case "YM":
		n := (y2-y1)*12 + (m2 - m1)
		if d2 < d1 {
			n--
		}
		return mkNum(float64(n % 12))
	case "MD":
		if d2 >= d1 {
			return mkNum(float64(d2 - d1))
		}
		pm, py := m2-1, y2
		if pm < 1 {
			pm, py = 12, y2-1
		}
		return mkNum(float64(daysInMonth(py, pm) + d2 - d1))
	case "YD":
		yy := y2
		if m2 < m1 || (m2 == m1 && d2 < d1) {
			yy--
		}
		st := dateSerial(yy, m1, d1)
		return mkNum(float64(s2 - st))
	}
	if s1 == s2 {
		return mkNum(0)
	}
	return mkErr(eVal)
}
