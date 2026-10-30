package main

import (
	"sort"
	"strings"
)

type numPos struct {
	v   float64
	pos int32
}

type colCache struct {
	n      int
	nums   []numPos          // sorted by v
	strIdx map[string][]int32 // lowercased text / display string -> positions
	empty  []int32
}

func (ctx *Ctx) getColCache(rg *Range) *colCache {
	b := ctx.book
	if b.cols == nil {
		b.cols = map[rangeKey]*colCache{}
	}
	k := rangeKey{rg.sh, rg.r1, rg.c1, rg.r2, rg.c2}
	if cc, ok := b.cols[k]; ok {
		return cc
	}
	n := (rg.r2 - rg.r1 + 1) * (rg.c2 - rg.c1 + 1)
	cc := &colCache{n: n, strIdx: map[string][]int32{}}
	i := int32(0)
	for r := rg.r1; r <= rg.r2; r++ {
		for c := rg.c1; c <= rg.c2; c++ {
			v := b.cellValue(rg.sh, r, c)
			switch v.k {
			case KNum, KBool:
				cc.nums = append(cc.nums, numPos{v.n, i})
				s := strings.ToLower(cellDisplay(v))
				cc.strIdx[s] = append(cc.strIdx[s], i)
			case KStr:
				if v.s == "" {
					cc.empty = append(cc.empty, i)
				} else {
					s := strings.ToLower(v.s)
					cc.strIdx[s] = append(cc.strIdx[s], i)
				}
			case KEmpty:
				cc.empty = append(cc.empty, i)
			case KErr:
				s := strings.ToLower(v.s)
				cc.strIdx[s] = append(cc.strIdx[s], i)
			}
			i++
		}
	}
	sort.Slice(cc.nums, func(a, b int) bool { return cc.nums[a].v < cc.nums[b].v })
	b.cols[k] = cc
	return cc
}

type bitset []uint64

func newBits(n int, all bool) bitset {
	b := make(bitset, (n+63)/64)
	if all {
		for i := range b {
			b[i] = ^uint64(0)
		}
	}
	return b
}
func (b bitset) set(i int32)      { b[i>>6] |= 1 << (uint(i) & 63) }
func (b bitset) has(i int) bool   { return b[i>>6]&(1<<(uint(i)&63)) != 0 }
func (b bitset) and(o bitset) {
	for i := range b {
		b[i] &= o[i]
	}
}
func (b bitset) not() {
	for i := range b {
		b[i] = ^b[i]
	}
}

// returns nil if the criterion cannot be evaluated with the cache
func (ctx *Ctx) criterionBits(cr *criterion, cc *colCache) bitset {
	n := cc.n
	if cr.none {
		return newBits(n, false)
	}
	if cr.isEmpty {
		bs := newBits(n, false)
		for _, p := range cc.empty {
			bs.set(p)
		}
		if cr.op == 'n' {
			bs.not()
		}
		return bs
	}
	if cr.isNum {
		switch cr.op {
		case '=', 'n':
			bs := newBits(n, false)
			lo := sort.Search(len(cc.nums), func(i int) bool { return cc.nums[i].v >= cr.num-abs64(cr.num)*1e-14 })
			for i := lo; i < len(cc.nums) && cc.nums[i].v <= cr.num+abs64(cr.num)*1e-14; i++ {
				if approxEqual(cc.nums[i].v, cr.num) {
					bs.set(cc.nums[i].pos)
				}
			}
			if cr.fromStr {
				for _, p := range cc.strIdx[strings.ToLower(cr.str)] {
					bs.set(p)
				}
			}
			if cr.op == 'n' {
				bs.not()
			}
			return bs
		case '<', 'l', '>', 'g':
			bs := newBits(n, false)
			var lo, hi int
			switch cr.op {
			case '<':
				lo, hi = 0, sort.Search(len(cc.nums), func(i int) bool { return cc.nums[i].v >= cr.num || approxEqual(cc.nums[i].v, cr.num) })
			case 'l':
				lo, hi = 0, sort.Search(len(cc.nums), func(i int) bool { return cc.nums[i].v > cr.num && !approxEqual(cc.nums[i].v, cr.num) })
			case '>':
				lo, hi = sort.Search(len(cc.nums), func(i int) bool { return cc.nums[i].v > cr.num && !approxEqual(cc.nums[i].v, cr.num) }), len(cc.nums)
			case 'g':
				lo, hi = sort.Search(len(cc.nums), func(i int) bool { return cc.nums[i].v >= cr.num || approxEqual(cc.nums[i].v, cr.num) }), len(cc.nums)
			}
			for i := lo; i < hi; i++ {
				bs.set(cc.nums[i].pos)
			}
			return bs
		}
		return nil
	}
	// text criterion
	if cr.wild || (cr.op != '=' && cr.op != 'n') {
		return nil
	}
	bs := newBits(n, false)
	for _, p := range cc.strIdx[strings.ToLower(cr.str)] {
		bs.set(p)
	}
	if cr.op == 'n' {
		bs.not()
	}
	return bs
}

func abs64(x float64) float64 {
	if x < 0 {
		return -x
	}
	return x
}

// fast path for *IFS over large single-column ranges; ok=false -> use the generic path
func (ctx *Ctx) fastIfs(pairs []*Node) (base *Range, mask []bool, ok bool, err Value) {
	var bits bitset
	for i := 0; i+1 < len(pairs); i += 2 {
		rg, e := ctx.rangeOf(pairs[i])
		if e.k == KErr {
			return nil, nil, true, e
		}
		rows := rg.r2 - rg.r1 + 1
		cols := rg.c2 - rg.c1 + 1
		if cols != 1 || rows < 512 {
			return nil, nil, false, Value{}
		}
		cv := ctx.deref(ctx.eval(pairs[i+1]))
		if cv.k == KErr {
			return nil, nil, true, cv
		}
		cr := ctx.makeCriterion(cv)
		if base == nil {
			base = rg
		} else if rows != base.r2-base.r1+1 || cols != base.c2-base.c1+1 {
			return nil, nil, true, ErrValue
		}
		cc := ctx.getColCache(rg)
		bs := ctx.criterionBits(&cr, cc)
		if bs == nil {
			return nil, nil, false, Value{}
		}
		if bits == nil {
			bits = bs
		} else {
			bits.and(bs)
		}
	}
	if base == nil {
		return nil, nil, false, Value{}
	}
	n := base.r2 - base.r1 + 1
	mask = make([]bool, n)
	for i := 0; i < n; i++ {
		mask[i] = bits.has(i)
	}
	return base, mask, true, Value{}
}
