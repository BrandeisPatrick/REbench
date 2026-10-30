package main

import (
	"strings"
	"sync"
)

type lookupIndex struct {
	nums map[float64]int
	strs map[string]int
}

var idxMu sync.Mutex
var idxCache = map[Ref]*lookupIndex{}

// getIndex returns an exact-match index for a 1-D range (vector) or nil.
func getIndex(ctx *Ctx, r Ref) *lookupIndex {
	if r.c1 != r.c2 && r.r1 != r.r2 {
		return nil
	}
	n := (r.r2 - r.r1 + 1) * (r.c2 - r.c1 + 1)
	if n < 64 {
		return nil
	}
	idxMu.Lock()
	ix, ok := idxCache[r]
	idxMu.Unlock()
	if ok {
		return ix
	}
	ix = &lookupIndex{nums: make(map[float64]int, n), strs: make(map[string]int, n)}
	i := 0
	forRange(ctx, r, func(v Val) bool {
		switch v.k {
		case KNum, KBool:
			if _, ok := ix.nums[v.n]; !ok {
				ix.nums[v.n] = i
			}
		case KStr:
			k := strings.ToLower(v.s)
			if _, ok := ix.strs[k]; !ok {
				ix.strs[k] = i
			}
		}
		i++
		return true
	})
	idxMu.Lock()
	idxCache[r] = ix
	idxMu.Unlock()
	return ix
}

// exactFind returns index of first exact match of key in vector r, or -1.
func exactFind(ctx *Ctx, key Val, r Ref) int {
	if key.k == KStr {
		if strings.ContainsAny(key.s, "*?~") {
			return lookupExact(key, rangeVals(ctx, r))
		}
		if n, ok := parseNumStr(key.s); ok {
			if ix := getIndex(ctx, r); ix != nil {
				best := -1
				if i, ok := ix.strs[strings.ToLower(key.s)]; ok {
					best = i
				}
				if i, ok := ix.nums[n]; ok && (best < 0 || i < best) {
					best = i
				}
				return best
			}
			return lookupExact(key, rangeVals(ctx, r))
		}
	}
	if key.k == KNum || key.k == KBool || key.k == KStr {
		if ix := getIndex(ctx, r); ix != nil {
			if key.k == KStr {
				if i, ok := ix.strs[strings.ToLower(key.s)]; ok {
					return i
				}
				return -1
			}
			if i, ok := ix.nums[key.n]; ok {
				return i
			}
			return -1
		}
	}
	return lookupExact(key, rangeVals(ctx, r))
}
