package main

import "strings"

type rangeKey struct {
	sh             *Sheet
	r1, c1, r2, c2 int
}

type colIndex struct {
	nums map[float64]int
	strs map[string]int
}

func (ctx *Ctx) getIndex(rg *Range) *colIndex {
	b := ctx.book
	if b.idx == nil {
		b.idx = map[rangeKey]*colIndex{}
	}
	k := rangeKey{rg.sh, rg.r1, rg.c1, rg.r2, rg.c2}
	if ix, ok := b.idx[k]; ok {
		return ix
	}
	ix := &colIndex{nums: map[float64]int{}, strs: map[string]int{}}
	i := 0
	for r := rg.r1; r <= rg.r2; r++ {
		for c := rg.c1; c <= rg.c2; c++ {
			v := b.cellValue(rg.sh, r, c)
			switch v.k {
			case KNum, KBool:
				if _, ok := ix.nums[v.n]; !ok {
					ix.nums[v.n] = i
				}
				s := strings.ToLower(cellDisplay(v))
				if _, ok := ix.strs[s]; !ok {
					ix.strs[s] = i
				}
			case KStr:
				s := strings.ToLower(v.s)
				if _, ok := ix.strs[s]; !ok {
					ix.strs[s] = i
				}
			}
			i++
		}
	}
	b.idx[k] = ix
	return ix
}

// exact lookup using the hash index; returns -2 if not applicable
func (ctx *Ctx) indexedExact(key Value, rg *Range) int {
	n := (rg.r2 - rg.r1 + 1) * (rg.c2 - rg.c1 + 1)
	if n < 64 {
		return -2
	}
	switch key.k {
	case KNum, KBool:
		ix := ctx.getIndex(rg)
		if i, ok := ix.nums[key.n]; ok {
			return i
		}
		return -1
	case KStr:
		if hasWild(key.s) {
			return -2
		}
		ix := ctx.getIndex(rg)
		if i, ok := ix.strs[strings.ToLower(key.s)]; ok {
			return i
		}
		return -1
	}
	return -2
}
