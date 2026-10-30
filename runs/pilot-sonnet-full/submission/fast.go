package main

import (
	"math/bits"
)

type critKey struct {
	sh     *Sheet
	c, r1  int
	n      int
	op     byte
	isNum  bool
	num    float64
	str    string
	hasWld bool
}

var bitCache = map[critKey][]uint64{}

func (cx *Ctx) matchBits(rg Rng, c *crit) []uint64 {
	n := rg.rows()
	key := critKey{rg.sh, rg.c1, rg.r1, n, c.op, c.isNum, c.num, c.str, c.hasWld}
	if b, ok := bitCache[key]; ok {
		return b
	}
	b := make([]uint64, (n+63)/64)
	sh := rg.sh
	for i := 0; i < n; i++ {
		var v Val
		r := rg.r1 + i
		if r < sh.R && rg.c1 < sh.C {
			ix := r*sh.C + rg.c1
			if sh.st[ix] == 2 {
				v = sh.v[ix]
			} else {
				v = cx.wb.get(sh, r, rg.c1)
			}
		}
		if c.match(v) {
			b[i>>6] |= 1 << (uint(i) & 63)
		}
	}
	bitCache[key] = b
	return b
}

// condAgg: conditional aggregation. rgs/cs criteria pairs; sr optional sum range.
// returns sum, count(matching rows with numeric sum value, or all matches when sr==nil), error
func (cx *Ctx) condAgg(rgs []Rng, cs []crit, sr *Rng) (float64, int, Val) {
	R, C := rgs[0].rows(), rgs[0].cols()
	var sum kahan
	cnt := 0
	if C == 1 && R >= 64 {
		var acc []uint64
		for k := range rgs {
			b := cx.matchBits(rgs[k], &cs[k])
			if k == 0 {
				acc = make([]uint64, len(b))
				copy(acc, b)
			} else {
				for w := range acc {
					acc[w] &= b[w]
				}
			}
		}
		if sr == nil {
			for _, w := range acc {
				cnt += bits.OnesCount64(w)
			}
			return 0, cnt, Val{}
		}
		for wi, w := range acc {
			for w != 0 {
				t := bits.TrailingZeros64(w)
				w &= w - 1
				i := wi<<6 + t
				v := cx.at(*sr, i, 0)
				if v.k == KErr {
					return 0, 0, v
				}
				if v.k == KNum || v.k == KBool {
					sum.add(v.n)
					cnt++
				}
			}
		}
		return sum.get(), cnt, Val{}
	}
	for i := 0; i < R; i++ {
		for j := 0; j < C; j++ {
			match := true
			for k := range rgs {
				if !cs[k].match(cx.at(rgs[k], i, j)) {
					match = false
					break
				}
			}
			if !match {
				continue
			}
			if sr == nil {
				cnt++
				continue
			}
			v := cx.at(*sr, i, j)
			if v.k == KErr {
				return 0, 0, v
			}
			if v.k == KNum || v.k == KBool {
				sum.add(v.n)
				cnt++
			}
		}
	}
	return sum.get(), cnt, Val{}
}

// ---- exact lookup index ----
type lkKey struct {
	sh    *Sheet
	c, r  int
	n     int
	horiz bool
}
type lkIdx struct {
	nums map[float64]int32
	strs map[string]int32
}

var lkCache = map[lkKey]*lkIdx{}

func (cx *Ctx) findExact(x Val, rg Rng, horiz bool, n int, get func(i int) Val) int {
	if n < 100 || (x.k != KNum && x.k != KBool && x.k != KStr) {
		return findPos(x, n, get, 0)
	}
	if x.k == KStr && isWild(x.s) {
		return findPos(x, n, get, 0)
	}
	key := lkKey{rg.sh, rg.c1, rg.r1, n, horiz}
	ix, ok := lkCache[key]
	if !ok {
		ix = &lkIdx{nums: make(map[float64]int32, n), strs: make(map[string]int32, n)}
		for i := 0; i < n; i++ {
			v := get(i)
			switch v.k {
			case KNum, KBool:
				if _, ok := ix.nums[v.n]; !ok {
					ix.nums[v.n] = int32(i)
				}
			case KStr:
				k := lowerStr(v.s)
				if _, ok := ix.strs[k]; !ok {
					ix.strs[k] = int32(i)
				}
			}
		}
		lkCache[key] = ix
	}
	switch x.k {
	case KNum, KBool:
		if p, ok := ix.nums[x.n]; ok {
			return int(p)
		}
		if x.n == float64(int64(x.n)) {
			return -1
		}
		return findPos(x, n, get, 0)
	case KStr:
		if p, ok := ix.strs[lowerStr(x.s)]; ok {
			// a numeric cell may match earlier (string vs formatted number)
			if len(x.s) > 0 && (x.s[0] >= '0' && x.s[0] <= '9' || x.s[0] == '-' || x.s[0] == '.') {
				return findPos(x, n, get, 0)
			}
			return int(p)
		}
		if len(x.s) > 0 && (x.s[0] >= '0' && x.s[0] <= '9' || x.s[0] == '-' || x.s[0] == '.') {
			return findPos(x, n, get, 0)
		}
		return -1
	}
	return -1
}
