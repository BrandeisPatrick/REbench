package main

import (
	"math"
	"sort"
	"strings"
	"unicode"
)

// iterate values of an argument: scalars, ranges (cell values), arrays
func (ctx *Ctx) eachValue(v Value, fn func(Value, bool) bool) bool {
	switch v.k {
	case KRange:
		rg := v.rg
		for c := rg.c1; c <= rg.c2; c++ {
			for r := rg.r1; r <= rg.r2; r++ {
				if !fn(ctx.book.cellValue(rg.sh, r, c), true) {
					return false
				}
			}
		}
		return true
	case KArray:
		for _, x := range v.ar.v {
			if !fn(x, true) {
				return false
			}
		}
		return true
	}
	return fn(v, false)
}

// collect numbers for aggregate functions. countMode: strings convertible counted
func (ctx *Ctx) collectNums(args []*Node, nums *[]float64, isCount bool) Value {
	var errv Value
	for ai := len(args) - 1; ai >= 0; ai-- {
		a := args[ai]
		if a == nil {
			*nums = append(*nums, 0)
			continue
		}
		v := ctx.eval(a)
		ok := ctx.eachValue(v, func(x Value, fromRange bool) bool {
			switch x.k {
			case KNum, KBool:
				*nums = append(*nums, x.n)
			case KStr:
				if !fromRange {
					if isCount {
						if _, ok := parseNumStr(x.s); ok {
							*nums = append(*nums, 0)
						}
					} else {
						errv = ErrValue
						return false
					}
				}
			case KErr:
				if !isCount {
					errv = x
					return false
				}
			}
			return true
		})
		if !ok {
			return errv
		}
	}
	return Value{}
}

// emulate LibreOffice's result-format tracking for MAX/MIN: args are processed last-to-first;
// inline/computed values force NUMBER, a single-cell ref adopts the cell type when it improves
// the extremum, a range adopts the type of its first numeric cell.
func (ctx *Ctx) maxMinIsBool(args []*Node, isMax bool) bool {
	have := false
	cur := 0.0
	fmtBool := false
	improves := func(v float64) bool {
		if !have {
			return true
		}
		if isMax {
			return v > cur
		}
		return v < cur
	}
	for ai := len(args) - 1; ai >= 0; ai-- {
		a := args[ai]
		if a == nil {
			if improves(0) {
				cur = 0
				have = true
			}
			fmtBool = false
			continue
		}
		v := ctx.eval(a)
		if v.k == KRange {
			rg := v.rg
			single := rg.r1 == rg.r2 && rg.c1 == rg.c2
			first := true
			ctx.eachValue(v, func(x Value, fromRange bool) bool {
				if x.k != KNum && x.k != KBool {
					return true
				}
				if single {
					if x.k == KNum {
						fmtBool = false
					}
					if improves(x.n) {
						cur = x.n
						have = true
						if x.k == KBool {
							fmtBool = true
						}
					}
					return true
				}
				if first {
					fmtBool = x.k == KBool
					first = false
				}
				if improves(x.n) {
					cur = x.n
					have = true
				}
				return true
			})
			continue
		}
		d := ctx.deref(v)
		if d.k == KNum || d.k == KBool {
			if improves(d.n) {
				cur = d.n
				have = true
			}
			fmtBool = false
		}
	}
	return fmtBool
}

// first value's "boolness" for MAX/MIN typing
func (ctx *Ctx) firstIsBool(a *Node) bool {
	if a == nil {
		return false
	}
	v := ctx.eval(a)
	res := false
	ctx.eachValue(v, func(x Value, fromRange bool) bool {
		if x.k == KNum {
			return false
		}
		if x.k == KBool {
			res = true
			return false
		}
		return true
	})
	return res
}

type criterion struct {
	op      byte // '=', 'n', '<', '>', 'l', 'g'
	isNum   bool
	num     float64
	str     string
	wild    bool
	isEmpty bool // criterion "" : matches empty
	fromStr bool
	none    bool // criterion from an empty cell: matches nothing
}

func (ctx *Ctx) makeCriterion(v Value) criterion {
	v = ctx.deref(v)
	var cr criterion
	cr.op = '='
	switch v.k {
	case KNum, KBool:
		cr.isNum = true
		cr.num = v.n
		return cr
	case KEmpty:
		cr.none = true
		return cr
	case KErr:
		cr.str = v.s
		return cr
	}
	s := v.s
	cr.fromStr = true
	if strings.HasPrefix(s, "<>") {
		cr.op = 'n'
		s = s[2:]
	} else if strings.HasPrefix(s, "<=") {
		cr.op = 'l'
		s = s[2:]
	} else if strings.HasPrefix(s, ">=") {
		cr.op = 'g'
		s = s[2:]
	} else if strings.HasPrefix(s, "<") {
		cr.op = '<'
		s = s[1:]
	} else if strings.HasPrefix(s, ">") {
		cr.op = '>'
		s = s[1:]
	} else if strings.HasPrefix(s, "=") {
		cr.op = '='
		s = s[1:]
	}
	if s == "" {
		cr.isEmpty = true
		return cr
	}
	if f, ok := parseNumStr(s); ok {
		cr.isNum = true
		cr.num = f
		cr.str = s
		return cr
	}
	cr.str = s
	cr.wild = hasWild(s) && (cr.op == '=' || cr.op == 'n')
	return cr
}

func (cr *criterion) match(v Value) bool {
	if cr.none {
		return false
	}
	if cr.isEmpty {
		empty := v.k == KEmpty || (v.k == KStr && v.s == "")
		if cr.op == 'n' {
			return !empty
		}
		return empty
	}
	if cr.isNum {
		switch v.k {
		case KNum, KBool:
			c := 0
			if !approxEqual(v.n, cr.num) {
				if v.n < cr.num {
					c = -1
				} else {
					c = 1
				}
			}
			return cmpOp(cr.op, c)
		case KStr:
			if cr.fromStr && (cr.op == '=' || cr.op == 'n') {
				eq := eqStrCI(v.s, cr.str)
				if cr.op == '=' {
					return eq
				}
				return !eq
			}
			return cr.op == 'n'
		case KEmpty:
			return cr.op == 'n'
		}
		return false
	}
	// text criterion
	var s string
	switch v.k {
	case KStr:
		s = v.s
	case KNum, KBool:
		if cr.op != '=' && cr.op != 'n' {
			return false
		}
		s = cellDisplay(v)
	case KEmpty:
		return cr.op == 'n'
	default:
		return false
	}
	if cr.wild {
		m := wildMatch(cr.str, s)
		if cr.op == '=' {
			return m
		}
		return !m
	}
	c := cmpStrCI(s, cr.str)
	return cmpOp(cr.op, c)
}

func cmpOp(op byte, c int) bool {
	switch op {
	case '=':
		return c == 0
	case 'n':
		return c != 0
	case '<':
		return c < 0
	case '>':
		return c > 0
	case 'l':
		return c <= 0
	case 'g':
		return c >= 0
	}
	return false
}

func (ctx *Ctx) rangeOf(a *Node) (*Range, Value) {
	if a == nil {
		return nil, ErrValue
	}
	v := ctx.eval(a)
	if v.k == KErr {
		return nil, v
	}
	if v.k != KRange {
		return nil, ErrValue
	}
	return v.rg, Value{}
}

// *IFS core: returns matched mask over the criteria ranges dims
func (ctx *Ctx) ifsMatch(pairs []*Node) (*Range, []bool, Value) {
	if fb, fm, ok, fe := ctx.fastIfs(pairs); ok {
		return fb, fm, fe
	}
	var base *Range
	var mask []bool
	for i := 0; i+1 < len(pairs); i += 2 {
		rg, e := ctx.rangeOf(pairs[i])
		if e.k == KErr {
			return nil, nil, e
		}
		cv := ctx.eval(pairs[i+1])
		cv = ctx.deref(cv)
		if cv.k == KErr {
			return nil, nil, cv
		}
		cr := ctx.makeCriterion(cv)
		rows := rg.r2 - rg.r1 + 1
		cols := rg.c2 - rg.c1 + 1
		if base == nil {
			base = rg
			mask = make([]bool, rows*cols)
			for k := range mask {
				mask[k] = true
			}
		} else if rows != base.r2-base.r1+1 || cols != base.c2-base.c1+1 {
			return nil, nil, ErrValue
		}
		k := 0
		for r := rg.r1; r <= rg.r2; r++ {
			for c := rg.c1; c <= rg.c2; c++ {
				if mask[k] {
					cl := rg.sh.at(r, c)
					var v Value
					if cl != nil {
						if cl.k == KFormula {
							v = ctx.book.cellValue(rg.sh, r, c)
						} else {
							v = Value{k: cl.k, n: cl.n, s: cl.s}
						}
					}
					if !cr.match(v) {
						mask[k] = false
					}
				}
				k++
			}
		}
	}
	return base, mask, Value{}
}

func (ctx *Ctx) sumRangeValues(sr *Range, base *Range, mask []bool, fn func(float64)) Value {
	rows := base.r2 - base.r1 + 1
	cols := base.c2 - base.c1 + 1
	k := 0
	for i := 0; i < rows; i++ {
		for j := 0; j < cols; j++ {
			if mask[k] {
				v := ctx.book.cellValue(sr.sh, sr.r1+i, sr.c1+j)
				if v.k == KNum || v.k == KBool {
					fn(v.n)
				} else if v.k == KErr {
					return v
				}
			}
			k++
		}
	}
	return Value{}
}

func (ctx *Ctx) argNum(n *Node, def float64) (float64, Value) {
	if n == nil {
		return def, Value{}
	}
	return ctx.toNum(ctx.eval(n))
}
func (ctx *Ctx) argStr(n *Node) (string, Value) {
	if n == nil {
		return "", Value{}
	}
	return ctx.toStr(ctx.eval(n))
}
func (ctx *Ctx) argInt(n *Node, def int) (int, Value) {
	if n == nil {
		return def, Value{}
	}
	return ctx.toInt(ctx.eval(n))
}

func runeLen(s string) int {
	n := 0
	for range s {
		n++
	}
	return n
}

var errAware = map[string]bool{"IF": true, "IFERROR": true, "IFNA": true, "IFS": true, "SWITCH": true, "CHOOSE": true,
	"ISBLANK": true, "ISERROR": true, "ISNA": true, "ISNUMBER": true, "ISTEXT": true, "ISLOGICAL": true,
	"COUNT": true, "COUNTA": true, "COUNTBLANK": true, "N": true, "T": true, "XLOOKUP": true}

var resetFn = map[string]bool{"IFS": true, "IFERROR": true, "IFNA": true, "SWITCH": true}

var jumpFn = map[string]bool{"IF": true, "IFERROR": true, "IFNA": true, "IFS": true, "SWITCH": true, "CHOOSE": true}

var logicalFn = map[string]bool{"ISBLANK": true, "ISERROR": true, "ISNA": true, "ISNUMBER": true, "ISTEXT": true, "ISLOGICAL": true}

func (ctx *Ctx) callFunc(n *Node) Value {
	args := n.args
	na := len(args)
	if !n.errAware {
		// LibreOffice-style precedence: computed errors are sticky in evaluation order (first wins),
		// error-handling functions (IFS/IFERROR/IFNA/SWITCH) clear the pending error; reference errors
		// only count when no computed error exists (last for most functions, first for CONCATENATE).
		var pending, firstRef, lastRef Value
		for _, a := range args {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			if a.op == NFunc && resetFn[a.fn] {
				pending = Value{}
			}
			if ctx.isMulti(v) {
				continue
			}
			d := ctx.deref(v)
			if d.k != KErr {
				continue
			}
			if a.op == NRef {
				if firstRef.k != KErr {
					firstRef = d
				}
				lastRef = d
				continue
			}
			if pending.k != KErr {
				pending = d
			}
		}
		if pending.k == KErr {
			return pending
		}
		if firstRef.k == KErr {
			if n.fn == "CONCATENATE" || n.fn == "CONCAT" || n.fn == "TEXTJOIN" {
				return firstRef
			}
			return lastRef
		}
	}
	switch n.fn {
	case "TRUE":
		return Bool(true)
	case "FALSE":
		return Bool(false)
	case "PI":
		return Num(math.Pi)
	case "NA":
		return ErrNA
	case "IF":
		if na < 1 || args[0] == nil {
			return ErrValue
		}
		cv := ctx.deref(ctx.eval(args[0]))
		if cv.k == KErr {
			return cv
		}
		b, e := ctx.toBool(cv)
		if e.k == KErr {
			return e
		}
		ctx.lastLogical = false
		if b {
			if na < 2 {
				return Bool(true)
			}
			if args[1] == nil {
				return Num(0)
			}
			return ctx.eval(args[1])
		}
		if na < 3 || args[2] == nil {
			if na < 3 {
				return Bool(false)
			}
			return Num(0)
		}
		return ctx.eval(args[2])
	case "IFERROR", "IFNA":
		if na < 2 {
			return ErrValue
		}
		v := Value{k: KEmpty}
		if args[0] != nil {
			v = ctx.deref(ctx.eval(args[0]))
		}
		if v.k == KErr && (n.fn == "IFERROR" || v.s == "#N/A") {
			if args[1] == nil {
				return Num(0)
			}
			return ctx.eval(args[1])
		}
		return v
	case "IFS":
		for i := 0; i+1 < na; i += 2 {
			if args[i] == nil {
				return ErrValue
			}
			b, e := ctx.toBool(ctx.eval(args[i]))
			if e.k == KErr {
				return e
			}
			ctx.lastLogical = false
			if b {
				if args[i+1] == nil {
					return Num(0)
				}
				return ctx.eval(args[i+1])
			}
		}
		return ErrNA
	case "SWITCH":
		if na < 3 || args[0] == nil {
			return ErrValue
		}
		sv := ctx.deref(ctx.eval(args[0]))
		if sv.k == KErr {
			return sv
		}
		i := 1
		for ; i+1 < na; i += 2 {
			kv := ctx.deref(ctx.eval(args[i]))
			if kv.k == KErr {
				return kv
			}
			if compareValues(sv, kv) == 0 {
				if args[i+1] == nil {
					return Num(0)
				}
				return ctx.eval(args[i+1])
			}
		}
		if i < na && args[i] != nil {
			return ctx.eval(args[i])
		}
		return ErrNA
	case "CHOOSE":
		if na < 2 {
			return ErrValue
		}
		idx, e := ctx.argInt(args[0], 0)
		if e.k == KErr {
			return e
		}
		if idx < 1 || idx >= na {
			return ErrValue
		}
		if args[idx] == nil {
			return Num(0)
		}
		return ctx.eval(args[idx])
	case "AND", "OR", "XOR":
		cnt := 0
		res := n.fn == "AND"
		xr := false
		for _, a := range args {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			var errv Value
			ok := ctx.eachValue(v, func(x Value, fromRange bool) bool {
				var b bool
				switch x.k {
				case KNum, KBool:
					b = x.n != 0
				case KStr:
					if fromRange {
						return true
					}
					f, ok := parseNumStr(x.s)
					if !ok {
						errv = ErrValue
						return false
					}
					b = f != 0
				case KEmpty:
					if fromRange {
						return true
					}
					b = false
				case KErr:
					errv = x
					return false
				default:
					return true
				}
				cnt++
				switch n.fn {
				case "AND":
					res = res && b
				case "OR":
					res = res || b
				case "XOR":
					xr = xr != b
				}
				return true
			})
			if !ok {
				return errv
			}
		}
		if cnt == 0 {
			return ErrValue
		}
		if n.fn == "XOR" {
			return Bool(xr)
		}
		return Bool(res)
	case "NOT":
		if na < 1 {
			return ErrValue
		}
		b, e := ctx.toBool(ctx.eval(args[0]))
		if e.k == KErr {
			return e
		}
		return Bool(!b)
	case "ISBLANK", "ISERROR", "ISNA", "ISNUMBER", "ISTEXT", "ISLOGICAL":
		if na < 1 || args[0] == nil {
			return Bool(n.fn == "ISBLANK")
		}
		v := ctx.deref(ctx.eval(args[0]))
		switch n.fn {
		case "ISBLANK":
			return Bool(v.k == KEmpty)
		case "ISERROR":
			return Bool(v.k == KErr)
		case "ISNA":
			return Bool(v.k == KErr && v.s == "#N/A")
		case "ISNUMBER":
			return Bool(v.k == KNum || v.k == KBool)
		case "ISTEXT":
			return Bool(v.k == KStr)
		case "ISLOGICAL":
			if args[0].op == NStr || args[0].op == NNum {
				return Bool(ctx.lastLogical)
			}
			return Bool(v.k == KBool)
		}
	case "N":
		if na < 1 || args[0] == nil {
			return Num(0)
		}
		v := ctx.deref(ctx.eval(args[0]))
		switch v.k {
		case KNum, KBool:
			return Num(v.n)
		case KErr:
			return v
		}
		return Num(0)
	case "T":
		if na < 1 || args[0] == nil {
			return Str("")
		}
		v := ctx.deref(ctx.eval(args[0]))
		if v.k == KStr {
			return v
		}
		if v.k == KErr {
			return v
		}
		return Str("")
	case "SUM", "PRODUCT", "SUMSQ", "AVERAGE", "COUNT", "MAX", "MIN", "MEDIAN", "STDEV", "VAR":
		var nums []float64
		e := ctx.collectNums(args, &nums, n.fn == "COUNT")
		if e.k == KErr {
			return e
		}
		switch n.fn {
		case "SUM":
			var ks KahanSum
			for i := 0; i < len(nums); i++ {
				ks.add(nums[i])
			}
			return checkNum(ks.get())
		case "PRODUCT":
			if len(nums) == 0 {
				return Num(0)
			}
			p := 1.0
			for _, x := range nums {
				p *= x
			}
			return checkNum(p)
		case "SUMSQ":
			var ks KahanSum
			for i := 0; i < len(nums); i++ {
				ks.add(nums[i] * nums[i])
			}
			return checkNum(ks.get())
		case "AVERAGE":
			if len(nums) == 0 {
				return ErrDiv0
			}
			var ks KahanSum
			for i := 0; i < len(nums); i++ {
				ks.add(nums[i])
			}
			return checkNum(ks.get() / float64(len(nums)))
		case "COUNT":
			return Num(float64(len(nums)))
		case "MAX", "MIN":
			if len(nums) == 0 {
				return Num(0)
			}
			m := nums[0]
			for _, x := range nums[1:] {
				if (n.fn == "MAX" && x > m) || (n.fn == "MIN" && x < m) {
					m = x
				}
			}
			if ctx.maxMinIsBool(args, n.fn == "MAX") {
				return Value{k: KBool, n: m}
			}
			return Num(m)
		case "MEDIAN":
			if len(nums) == 0 {
				return ErrValue
			}
			sort.Float64s(nums)
			l := len(nums)
			if l%2 == 1 {
				return Num(nums[l/2])
			}
			return Num((nums[l/2-1] + nums[l/2]) / 2)
		case "STDEV", "VAR":
			if len(nums) < 2 {
				return ErrDiv0
			}
			var km KahanSum
			for i := 0; i < len(nums); i++ {
				km.add(nums[i])
			}
			m := km.get() / float64(len(nums))
			ss := 0.0
			for i := 0; i < len(nums); i++ {
				ss += (nums[i] - m) * (nums[i] - m)
			}
			v := ss / float64(len(nums)-1)
			if n.fn == "STDEV" {
				return Num(math.Sqrt(v))
			}
			return Num(v)
		}
	case "LARGE", "SMALL":
		if na < 2 {
			return ErrValue
		}
		var nums []float64
		e := ctx.collectNums(args[:1], &nums, false)
		if e.k == KErr {
			return e
		}
		kf, e := ctx.argNum(args[1], 0)
		if e.k == KErr {
			return e
		}
		k := int(math.Ceil(kf - 1e-9))
		if kf != math.Floor(kf) {
			k = int(math.Ceil(kf))
		}
		if k < 1 || k > len(nums) {
			return ErrValue
		}
		sort.Float64s(nums)
		if n.fn == "SMALL" {
			return Num(nums[k-1])
		}
		return Num(nums[len(nums)-k])
	case "COUNTA", "COUNTBLANK":
		cnt := 0
		for _, a := range args {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			ctx.eachValue(v, func(x Value, fromRange bool) bool {
				empty := x.k == KEmpty || (n.fn == "COUNTBLANK" && x.k == KStr && x.s == "")
				if n.fn == "COUNTA" {
					if !empty {
						cnt++
					}
				} else if empty {
					cnt++
				}
				return true
			})
		}
		return Num(float64(cnt))
	case "SUMIF", "AVERAGEIF", "COUNTIF":
		if na < 2 {
			return ErrValue
		}
		base, mask, e := ctx.ifsMatch(args[:2])
		if e.k == KErr {
			return e
		}
		if n.fn == "COUNTIF" {
			cnt := 0
			for _, m := range mask {
				if m {
					cnt++
				}
			}
			return Num(float64(cnt))
		}
		sr := base
		if na >= 3 && args[2] != nil {
			sr, e = ctx.rangeOf(args[2])
			if e.k == KErr {
				return e
			}
		}
		var ks KahanSum
		cnt := 0
		e = ctx.sumRangeValues(sr, base, mask, func(f float64) { ks.add(f); cnt++ })
		if e.k == KErr {
			return e
		}
		if n.fn == "SUMIF" {
			return Num(ks.get())
		}
		if cnt == 0 {
			return ErrDiv0
		}
		return Num(ks.get() / float64(cnt))
	case "SUMIFS", "AVERAGEIFS", "COUNTIFS":
		var sr *Range
		pairs := args
		if n.fn != "COUNTIFS" {
			if na < 3 {
				return ErrValue
			}
			var e Value
			sr, e = ctx.rangeOf(args[0])
			if e.k == KErr {
				return e
			}
			pairs = args[1:]
		}
		base, mask, e := ctx.ifsMatch(pairs)
		if e.k == KErr {
			return e
		}
		if n.fn == "COUNTIFS" {
			cnt := 0
			for _, m := range mask {
				if m {
					cnt++
				}
			}
			return Num(float64(cnt))
		}
		if sr.r2-sr.r1 != base.r2-base.r1 || sr.c2-sr.c1 != base.c2-base.c1 {
			return ErrValue
		}
		var ks KahanSum
		cnt := 0
		e = ctx.sumRangeValues(sr, base, mask, func(f float64) { ks.add(f); cnt++ })
		if e.k == KErr {
			return e
		}
		if n.fn == "SUMIFS" {
			return Num(ks.get())
		}
		if cnt == 0 {
			return ErrDiv0
		}
		return Num(ks.get() / float64(cnt))
	case "SUMPRODUCT":
		var arrs []*Array
		for _, a := range args {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			if v.k == KErr {
				return v
			}
			arr := ctx.toArray(v)
			if v.k == KRange {
				for _, x := range arr.v {
					if x.k == KErr {
						return x
					}
				}
			}
			arrs = append(arrs, arr)
		}
		if len(arrs) == 0 {
			return ErrValue
		}
		for _, a := range arrs[1:] {
			if a.rows != arrs[0].rows || a.cols != arrs[0].cols {
				return ErrValue
			}
		}
		var ks KahanSum
		for i := range arrs[0].v {
			p := 1.0
			for _, a := range arrs {
				x := a.v[i]
				switch x.k {
				case KNum, KBool:
					p *= x.n
				default:
					p = 0
				}
			}
			ks.add(p)
		}
		return Num(ks.get())
	case "ABS", "INT", "SQRT", "EXP", "LN", "LOG10", "SIGN":
		if na < 1 {
			return ErrValue
		}
		f, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		switch n.fn {
		case "ABS":
			return Num(math.Abs(f))
		case "INT":
			return Num(approxFloor(f))
		case "SQRT":
			if f < 0 {
				return ErrValue
			}
			return Num(math.Sqrt(f))
		case "EXP":
			return checkNum(math.Exp(f))
		case "LN":
			if f <= 0 {
				return ErrValue
			}
			return Num(math.Log(f))
		case "LOG10":
			if f <= 0 {
				return ErrValue
			}
			return Num(math.Log10(f))
		case "SIGN":
			if f > 0 {
				return Num(1)
			} else if f < 0 {
				return Num(-1)
			}
			return Num(0)
		}
	case "LOG":
		if na < 1 {
			return ErrValue
		}
		f, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		b := 10.0
		if na >= 2 {
			b, e = ctx.argNum(args[1], 10)
			if e.k == KErr {
				return e
			}
		}
		if f <= 0 || b <= 0 || b == 1 {
			return ErrValue
		}
		return Num(math.Log(f) / math.Log(b))
	case "POWER":
		if na < 2 {
			return ErrValue
		}
		a, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		b, e := ctx.argNum(args[1], 0)
		if e.k == KErr {
			return e
		}
		return powVal(a, b)
	case "MOD":
		if na < 2 {
			return ErrValue
		}
		a, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		b, e := ctx.argNum(args[1], 0)
		if e.k == KErr {
			return e
		}
		if b == 0 {
			return ErrDiv0
		}
		q := approxFloor(a / b)
		return Num(a - b*q)
	case "ROUND", "ROUNDUP", "ROUNDDOWN", "TRUNC":
		if na < 1 {
			return ErrValue
		}
		f, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		d := 0
		if na >= 2 {
			d, e = ctx.argInt(args[1], 0)
			if e.k == KErr {
				return e
			}
		}
		return Num(roundMode(f, d, n.fn))
	case "DATE":
		if na < 3 {
			return ErrValue
		}
		y, e := ctx.argInt(args[0], 0)
		if e.k == KErr {
			return e
		}
		m, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		d, e := ctx.argInt(args[2], 0)
		if e.k == KErr {
			return e
		}
		if y < 100 && y >= 0 {
			y += 1900
		}
		y += (m - 1) / 12
		mm := (m - 1) % 12
		if mm < 0 {
			mm += 12
			y--
		}
		m = mm + 1
		return Num(float64(daysFromCivil(y, m, 1) + d - 1))
	case "DAY", "MONTH", "YEAR", "WEEKDAY":
		if na < 1 {
			return ErrValue
		}
		f, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		ser := int(math.Floor(f))
		y, m, d := civilFromDays(ser)
		switch n.fn {
		case "DAY":
			return Num(float64(d))
		case "MONTH":
			return Num(float64(m))
		case "YEAR":
			return Num(float64(y))
		}
		typ := 1
		if na >= 2 && args[1] != nil {
			typ, e = ctx.argInt(args[1], 1)
			if e.k == KErr {
				return e
			}
		}
		dow := ((ser+6)%7 + 7) % 7 // 0=Sunday
		mon := (dow + 6) % 7      // 0=Monday
		switch typ {
		case 1:
			return Num(float64(dow + 1))
		case 2, 11:
			return Num(float64(mon + 1))
		case 3:
			return Num(float64(mon))
		case 12, 13, 14, 15, 16, 17:
			shift := typ - 11
			return Num(float64((mon-shift+7)%7 + 1))
		}
		return ErrValue
	case "DAYS":
		if na < 2 {
			return ErrValue
		}
		a, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		b, e := ctx.argNum(args[1], 0)
		if e.k == KErr {
			return e
		}
		return Num(a - b)
	case "EDATE", "EOMONTH":
		if na < 2 {
			return ErrValue
		}
		f, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		mo, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		y, m, d := civilFromDays(int(math.Floor(f)))
		t := y*12 + (m - 1) + mo
		y = t / 12
		m = t%12 + 1
		if t < 0 && t%12 != 0 {
			y--
			m = t%12 + 12 + 1
		}
		dim := daysInMonth(y, m)
		if n.fn == "EOMONTH" {
			d = dim
		} else if d > dim {
			d = dim
		}
		return Num(float64(daysFromCivil(y, m, d)))
	case "DATEDIF":
		if na < 3 {
			return ErrValue
		}
		a, e := ctx.argNum(args[0], 0)
		if e.k == KErr {
			return e
		}
		b, e := ctx.argNum(args[1], 0)
		if e.k == KErr {
			return e
		}
		u, e := ctx.argStr(args[2])
		if e.k == KErr {
			return e
		}
		if a > b {
			return ErrValue
		}
		s1, s2 := int(math.Floor(a)), int(math.Floor(b))
		y1, m1, d1 := civilFromDays(s1)
		y2, m2, d2 := civilFromDays(s2)
		switch strings.ToLower(u) {
		case "d":
			return Num(float64(s2 - s1))
		case "m":
			mo := (y2-y1)*12 + (m2 - m1)
			if d2 < d1 {
				mo--
			}
			return Num(float64(mo))
		case "y":
			yr := y2 - y1
			if m2 < m1 || (m2 == m1 && d2 < d1) {
				yr--
			}
			return Num(float64(yr))
		case "ym":
			mo := (y2-y1)*12 + (m2 - m1)
			if d2 < d1 {
				mo--
			}
			return Num(float64(((mo % 12) + 12) % 12))
		case "md":
			if d2 >= d1 {
				return Num(float64(d2 - d1))
			}
			py, pm := y2, m2-1
			if pm < 1 {
				pm = 12
				py--
			}
			ser := daysFromCivil(py, pm, 1) + d1 - 1
			return Num(float64(s2 - ser))
		case "yd":
			ser := daysFromCivil(y2, m1, 1) + d1 - 1
			if ser > s2 {
				ser = daysFromCivil(y2-1, m1, 1) + d1 - 1
			}
			return Num(float64(s2 - ser))
		}
		return ErrValue
	case "LEN", "UPPER", "LOWER", "PROPER", "TRIM":
		if na < 1 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		switch n.fn {
		case "LEN":
			return Num(float64(runeLen(s)))
		case "UPPER":
			return Str(strings.ToUpper(s))
		case "LOWER":
			return Str(strings.ToLower(s))
		case "PROPER":
			rs := []rune(s)
			prevLetter := false
			for i, r := range rs {
				if unicode.IsLetter(r) {
					if prevLetter {
						rs[i] = unicode.ToLower(r)
					} else {
						rs[i] = unicode.ToUpper(r)
					}
					prevLetter = true
				} else {
					prevLetter = false
				}
			}
			return Str(string(rs))
		case "TRIM":
			return Str(strings.Join(strings.Fields(s), " "))
		}
	case "LEFT", "RIGHT":
		if na < 1 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		cnt := 1
		if na >= 2 && args[1] != nil {
			cnt, e = ctx.argInt(args[1], 1)
			if e.k == KErr {
				return e
			}
		}
		if cnt < 0 {
			return ErrValue
		}
		rs := []rune(s)
		if cnt > len(rs) {
			cnt = len(rs)
		}
		if n.fn == "LEFT" {
			return Str(string(rs[:cnt]))
		}
		return Str(string(rs[len(rs)-cnt:]))
	case "MID":
		if na < 3 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		st, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		cnt, e := ctx.argInt(args[2], 0)
		if e.k == KErr {
			return e
		}
		if st < 1 || cnt < 0 {
			return ErrValue
		}
		rs := []rune(s)
		if st-1 >= len(rs) {
			return Str("")
		}
		end := st - 1 + cnt
		if end > len(rs) {
			end = len(rs)
		}
		return Str(string(rs[st-1 : end]))
	case "REPT":
		if na < 2 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		cnt, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		if cnt < 0 {
			return ErrValue
		}
		if cnt*len(s) > 10000000 {
			return ErrValue
		}
		return Str(strings.Repeat(s, cnt))
	case "EXACT":
		if na < 2 {
			return ErrValue
		}
		a, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		b, e := ctx.argStr(args[1])
		if e.k == KErr {
			return e
		}
		return Bool(a == b)
	case "FIND", "SEARCH":
		if na < 2 {
			return ErrValue
		}
		f, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		w, e := ctx.argStr(args[1])
		if e.k == KErr {
			return e
		}
		st := 1
		if na >= 3 && args[2] != nil {
			st, e = ctx.argInt(args[2], 1)
			if e.k == KErr {
				return e
			}
		}
		wr := []rune(w)
		if st < 1 || st > len(wr) || f == "" {
			if f == "" && n.fn == "FIND" {
				return ErrValue
			}
			if st < 1 || st > len(wr) {
				return ErrValue
			}
		}
		if n.fn == "FIND" {
			idx := strings.Index(string(wr[st-1:]), f)
			if idx < 0 {
				return ErrValue
			}
			return Num(float64(st + runeLen(string(wr[st-1:])[:idx])))
		}
		// SEARCH: case-insensitive with wildcards
		sub := wr[st-1:]
		fr := []rune(strings.ToLower(f))
		low := []rune(strings.ToLower(string(sub)))
		if hasWild(f) {
			for i := 0; i < len(low); i++ {
				for j := i; j <= len(low); j++ {
					if wmExact(fr, low[i:j]) {
						return Num(float64(st + i))
					}
				}
			}
			return ErrValue
		}
		idx := strings.Index(string(low), string(fr))
		if idx < 0 {
			return ErrValue
		}
		return Num(float64(st + runeLen(string(low)[:idx])))
	case "SUBSTITUTE":
		if na < 3 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		old, e := ctx.argStr(args[1])
		if e.k == KErr {
			return e
		}
		nw, e := ctx.argStr(args[2])
		if e.k == KErr {
			return e
		}
		if na >= 4 && args[3] != nil {
			inst, e := ctx.argInt(args[3], 1)
			if e.k == KErr {
				return e
			}
			if inst < 1 {
				return ErrValue
			}
			if old == "" {
				return Str(s)
			}
			pos := 0
			for k := 1; ; k++ {
				i := strings.Index(s[pos:], old)
				if i < 0 {
					return Str(s)
				}
				if k == inst {
					return Str(s[:pos+i] + nw + s[pos+i+len(old):])
				}
				pos += i + len(old)
			}
		}
		if old == "" {
			return Str(s)
		}
		return Str(strings.ReplaceAll(s, old, nw))
	case "REPLACE":
		if na < 4 {
			return ErrValue
		}
		s, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		st, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		cnt, e := ctx.argInt(args[2], 0)
		if e.k == KErr {
			return e
		}
		nw, e := ctx.argStr(args[3])
		if e.k == KErr {
			return e
		}
		if st < 1 || cnt < 0 {
			return ErrValue
		}
		rs := []rune(s)
		if st-1 > len(rs) {
			st = len(rs) + 1
		}
		end := st - 1 + cnt
		if end > len(rs) {
			end = len(rs)
		}
		return Str(string(rs[:st-1]) + nw + string(rs[end:]))
	case "CONCAT", "CONCATENATE":
		var b strings.Builder
		for _, a := range args {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			var errv Value
			ok := ctx.eachValue(v, func(x Value, fromRange bool) bool {
				s, e := ctx.toStr(x)
				if e.k == KErr {
					errv = e
					return false
				}
				b.WriteString(s)
				return true
			})
			if !ok {
				return errv
			}
		}
		return Str(b.String())
	case "TEXTJOIN":
		if na < 3 {
			return ErrValue
		}
		delim, e := ctx.argStr(args[0])
		if e.k == KErr {
			return e
		}
		ign, e := ctx.toBool(ctx.eval(args[1]))
		if e.k == KErr {
			return e
		}
		var parts []string
		for _, a := range args[2:] {
			if a == nil {
				continue
			}
			v := ctx.eval(a)
			var errv Value
			ok := ctx.eachValue(v, func(x Value, fromRange bool) bool {
				s, e := ctx.toStr(x)
				if e.k == KErr {
					errv = e
					return false
				}
				if ign && s == "" {
					return true
				}
				parts = append(parts, s)
				return true
			})
			if !ok {
				return errv
			}
		}
		return Str(strings.Join(parts, delim))
	case "VALUE":
		if na < 1 {
			return ErrValue
		}
		v := ctx.deref(ctx.eval(args[0]))
		if v.k == KErr {
			return v
		}
		if v.k == KNum || v.k == KBool {
			return Num(v.n)
		}
		if v.k == KEmpty {
			return Num(0)
		}
		f, ok := parseNumStr(v.s)
		if !ok {
			return ErrValue
		}
		return Num(f)
	case "TEXT":
		if na < 2 {
			return ErrValue
		}
		v := ctx.deref(ctx.eval(args[0]))
		if v.k == KErr {
			return v
		}
		fm, e := ctx.argStr(args[1])
		if e.k == KErr {
			return e
		}
		if v.k == KStr {
			if f, ok := parseNumStr(v.s); ok {
				return Str(formatText(f, fm))
			}
			return v
		}
		f, _ := ctx.toNum(v)
		return Str(formatText(f, fm))
	case "INDEX":
		if na < 2 {
			return ErrValue
		}
		rg, e := ctx.rangeOf(args[0])
		if e.k == KErr {
			return e
		}
		ri, e := ctx.argInt(args[1], 0)
		if e.k == KErr {
			return e
		}
		ci := 0
		hasCol := na >= 3 && args[2] != nil
		if hasCol {
			ci, e = ctx.argInt(args[2], 0)
			if e.k == KErr {
				return e
			}
		}
		rows := rg.r2 - rg.r1 + 1
		cols := rg.c2 - rg.c1 + 1
		if !hasCol && rows == 1 && cols > 1 {
			ci = ri
			ri = 1
		}
		if ri < 0 || ci < 0 || ri > rows || ci > cols {
			return ErrRef
		}
		if ri == 0 && ci == 0 {
			if rows == 1 && cols == 1 {
				return Value{k: KRange, rg: rg}
			}
			return ErrValue
		}
		if ri == 0 {
			return Value{k: KRange, rg: &Range{sh: rg.sh, r1: rg.r1, r2: rg.r2, c1: rg.c1 + ci - 1, c2: rg.c1 + ci - 1}}
		}
		if ci == 0 {
			if cols == 1 {
				ci = 1
			} else {
				return Value{k: KRange, rg: &Range{sh: rg.sh, r1: rg.r1 + ri - 1, r2: rg.r1 + ri - 1, c1: rg.c1, c2: rg.c2}}
			}
		}
		return Value{k: KRange, rg: &Range{sh: rg.sh, r1: rg.r1 + ri - 1, r2: rg.r1 + ri - 1, c1: rg.c1 + ci - 1, c2: rg.c1 + ci - 1}}
	case "MATCH":
		if na < 2 {
			return ErrValue
		}
		key := ctx.deref(ctx.eval(args[0]))
		if key.k == KErr {
			return key
		}
		rg, e := ctx.rangeOf(args[1])
		if e.k == KErr {
			return e
		}
		typ := 1
		if na >= 3 && args[2] != nil {
			typ, e = ctx.argInt(args[2], 1)
			if e.k == KErr {
				return e
			}
		}
		idx := -2
		if typ == 0 {
			idx = ctx.indexedExact(key, rg)
		}
		if idx == -2 {
			vals := ctx.vectorOf(rg)
			idx = ctx.lookupIndex(key, vals, typ)
		}
		if idx < 0 {
			return ErrNA
		}
		return Num(float64(idx + 1))
	case "VLOOKUP", "HLOOKUP":
		if na < 3 {
			return ErrValue
		}
		key := ctx.deref(ctx.eval(args[0]))
		if key.k == KErr {
			return key
		}
		rg, e := ctx.rangeOf(args[1])
		if e.k == KErr {
			return e
		}
		idx, e := ctx.argInt(args[2], 0)
		if e.k == KErr {
			return e
		}
		sorted := true
		if na >= 4 && args[3] != nil {
			sorted, e = ctx.toBool(ctx.eval(args[3]))
			if e.k == KErr {
				return e
			}
		}
		var vrg *Range
		var size int
		if n.fn == "VLOOKUP" {
			vrg = &Range{sh: rg.sh, r1: rg.r1, r2: rg.r2, c1: rg.c1, c2: rg.c1}
			size = rg.c2 - rg.c1 + 1
		} else {
			vrg = &Range{sh: rg.sh, r1: rg.r1, r2: rg.r1, c1: rg.c1, c2: rg.c2}
			size = rg.r2 - rg.r1 + 1
		}
		if idx < 1 || idx > size {
			return ErrValue
		}
		typ := 0
		if sorted {
			typ = 1
		}
		pos := -2
		if typ == 0 {
			pos = ctx.indexedExact(key, vrg)
		}
		if pos == -2 {
			vec := ctx.vectorOf(vrg)
			pos = ctx.lookupIndex(key, vec, typ)
		}
		if pos < 0 {
			return ErrNA
		}
		var v Value
		if n.fn == "VLOOKUP" {
			v = ctx.book.cellValue(rg.sh, rg.r1+pos, rg.c1+idx-1)
		} else {
			v = ctx.book.cellValue(rg.sh, rg.r1+idx-1, rg.c1+pos)
		}
		if v.k == KBool {
			return Num(v.n)
		}
		return v
	case "XLOOKUP":
		if na < 3 {
			return ErrValue
		}
		key := ctx.deref(ctx.eval(args[0]))
		if key.k == KErr {
			return key
		}
		lr, e := ctx.rangeOf(args[1])
		if e.k == KErr {
			return e
		}
		rr, e := ctx.rangeOf(args[2])
		if e.k == KErr {
			return e
		}
		mode := 0
		if na >= 5 && args[4] != nil {
			mode, e = ctx.argInt(args[4], 0)
			if e.k == KErr {
				return e
			}
		}
		smode := 1
		if na >= 6 && args[5] != nil {
			smode, e = ctx.argInt(args[5], 1)
			if e.k == KErr {
				return e
			}
		}
		vertical := lr.c1 == lr.c2
		if vertical {
			if rr.r2-rr.r1 != lr.r2-lr.r1 {
				return ErrValue
			}
		} else if rr.c2-rr.c1 != lr.c2-lr.c1 {
			return ErrValue
		}
		pos := -2
		if (mode == 0 || (mode == 2 && !(key.k == KStr && hasWild(key.s)))) && smode == 1 && key.k != KEmpty {
			pos = ctx.indexedExact(key, lr)
		}
		if pos == -2 {
			vec := ctx.vectorOf(lr)
			pos = ctx.xlookupIndex(key, vec, mode, smode)
		}
		if pos < 0 {
			if na >= 4 && args[3] != nil {
				return ctx.eval(args[3])
			}
			return ErrNA
		}
		var res Value
		if vertical {
			res = Value{k: KRange, rg: &Range{sh: rr.sh, r1: rr.r1 + pos, r2: rr.r1 + pos, c1: rr.c1, c2: rr.c2}}
		} else {
			res = Value{k: KRange, rg: &Range{sh: rr.sh, r1: rr.r1, r2: rr.r2, c1: rr.c1 + pos, c2: rr.c1 + pos}}
		}
		return res
	}
	return ErrName
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func (ctx *Ctx) argIntOpt(a []*Node) {}

func wmExact(p, t []rune) bool { return wm(p, t) }

func roundMode(f float64, d int, mode string) float64 {
	if d > 300 || d < -300 {
		return f
	}
	p := math.Pow(10, float64(d))
	neg := f < 0
	v := math.Abs(f) * p
	var r float64
	switch mode {
	case "ROUND":
		r = approxFloor(v + 0.5)
	case "ROUNDUP":
		r = approxCeil(v)
	default:
		r = approxFloor(v)
	}
	r = r / p
	if neg {
		r = -r
	}
	return r
}

func (ctx *Ctx) vectorOf(rg *Range) []Value {
	rows := rg.r2 - rg.r1 + 1
	cols := rg.c2 - rg.c1 + 1
	out := make([]Value, 0, rows*cols)
	for r := rg.r1; r <= rg.r2; r++ {
		for c := rg.c1; c <= rg.c2; c++ {
			out = append(out, ctx.book.cellValue(rg.sh, r, c))
		}
	}
	return out
}

func exactMatch(key, v Value, wild bool) bool {
	switch key.k {
	case KNum, KBool:
		return (v.k == KNum || v.k == KBool) && approxEqual(key.n, v.n)
	case KStr:
		if v.k == KEmpty {
			return false
		}
		s := cellDisplay(v)
		if wild && hasWild(key.s) {
			return wildMatch(key.s, s)
		}
		return eqStrCI(key.s, s)
	}
	return false
}

// typ: 0 exact, 1 ascending (last <= key), -1 descending (last >= key)
func (ctx *Ctx) lookupIndex(key Value, vec []Value, typ int) int {
	if key.k == KEmpty {
		return -1
	}
	if typ == 0 {
		for i, v := range vec {
			if exactMatch(key, v, true) {
				return i
			}
		}
		return -1
	}
	// binary search over non-empty entries
	idx := make([]int, 0, len(vec))
	for i, v := range vec {
		if v.k != KEmpty && v.k != KErr {
			idx = append(idx, i)
		}
	}
	lo, hi, res := 0, len(idx)-1, -1
	for lo <= hi {
		mid := (lo + hi) / 2
		c := compareValues(vec[idx[mid]], key)
		if (typ > 0 && c <= 0) || (typ < 0 && c >= 0) {
			res = idx[mid]
			lo = mid + 1
		} else {
			hi = mid - 1
		}
	}
	if res >= 0 && typ < 0 {
		// descending: require same type for numbers
		if key.k != KStr && vec[res].k == KStr {
			return -1
		}
	}
	return res
}

func (ctx *Ctx) xlookupIndex(key Value, vec []Value, mode, smode int) int {
	n := len(vec)
	if key.k == KEmpty {
		key = Str("")
	}
	order := func(i int) int {
		if smode < 0 {
			return n - 1 - i
		}
		return i
	}
	best := -1
	for ii := 0; ii < n; ii++ {
		i := order(ii)
		v := vec[i]
		if mode == 0 || mode == 2 {
			if exactMatch(key, v, mode == 2) {
				return i
			}
			continue
		}
		// same-type only
		kn := key.k == KNum || key.k == KBool
		vn := v.k == KNum || v.k == KBool
		if kn != vn || (key.k == KStr && v.k != KStr) {
			continue
		}
		c := compareValues(v, key)
		if c == 0 {
			return i
		}
		if mode == -1 && c < 0 {
			if best < 0 || compareValues(v, vec[best]) > 0 {
				best = i
			}
		} else if mode == 1 && c > 0 {
			if best < 0 || compareValues(v, vec[best]) < 0 {
				best = i
			}
		}
	}
	return best
}
