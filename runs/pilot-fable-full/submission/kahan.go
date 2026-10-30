package main

import "math"

// KahanSum mirrors LibreOffice's Neumaier summation with a delayed last term.
type KahanSum struct {
	sum, err, mem float64
	hasMem        bool
}

func (k *KahanSum) add(x float64) {
	if x == 0 {
		return
	}
	if !k.hasMem {
		k.mem = x
		k.hasMem = true
		return
	}
	y := k.mem
	t := k.sum + y
	if math.Abs(k.sum) >= math.Abs(y) {
		k.err += (k.sum - t) + y
	} else {
		k.err += (y - t) + k.sum
	}
	k.sum = t
	k.mem = x
}

func approxAdd(a, b float64) float64 {
	if ((a < 0 && b > 0) || (b < 0 && a > 0)) && approxEqual(a, -b) {
		return 0
	}
	return a + b
}

func (k *KahanSum) get() float64 {
	total := k.sum + k.err
	if !k.hasMem {
		return total
	}
	return approxAdd(total, k.mem)
}
