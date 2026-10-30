package main

import (
	"bufio"
	"fmt"
	"math"
	"os"
	"runtime"
	"runtime/debug"
	"runtime/pprof"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

func colName(c int) string {
	s := ""
	c++
	for c > 0 {
		c--
		s = string(rune('A'+c%26)) + s
		c /= 26
	}
	return s
}

func jsonStr(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\n':
			b.WriteString(`\n`)
		case '\r':
			b.WriteString(`\r`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if r < 0x20 {
				fmt.Fprintf(&b, `\u%04x`, r)
			} else {
				b.WriteRune(r)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}

// collect refs in an AST
func collectRefs(n Node, out *[]Ref) {
	switch x := n.(type) {
	case *RefNode:
		*out = append(*out, x.ref)
	case *BinNode:
		collectRefs(x.l, out)
		collectRefs(x.r, out)
	case *UnNode:
		collectRefs(x.x, out)
	case *FuncNode:
		for _, a := range x.args {
			if a != nil {
				collectRefs(a, out)
			}
		}
	}
}

func parallelPhase(book *Book) {
	nCPU := runtime.NumCPU()
	if nCPU > 4 {
		nCPU = 4
	}
	// which sheets have formulas
	hasF := make([]int, len(book.sheets))
	for si, sh := range book.sheets {
		for c := range sh.cols {
			for _, cell := range sh.cols[c] {
				if cell != nil && cell.hasF {
					hasF[si]++
				}
			}
		}
	}
	type job struct{ si, r, c int }
	var jobs []job
	// parse all formulas and classify independence in parallel
	type pj struct {
		cell  *Cell
		si    int
		r, c  int
		indep bool
	}
	var all []pj
	for si, sh := range book.sheets {
		for c := range sh.cols {
			for r, cell := range sh.cols[c] {
				if cell != nil && cell.hasF {
					all = append(all, pj{cell: cell, si: si, r: r, c: c})
				}
			}
		}
	}
	{
		var wg sync.WaitGroup
		chunk := (len(all) + nCPU - 1) / nCPU
		for w := 0; w < nCPU; w++ {
			lo, hi := w*chunk, (w+1)*chunk
			if hi > len(all) {
				hi = len(all)
			}
			if lo >= hi {
				continue
			}
			wg.Add(1)
			go func(part []pj) {
				defer wg.Done()
				var refs []Ref
				for pi := range part {
					p := &part[pi]
					p.cell.ast = parseFormula(p.cell.formula, book, p.si)
					refs = refs[:0]
					collectRefs(p.cell.ast, &refs)
					indep := true
					for _, rf := range refs {
						if hasF[rf.sheet] == 0 {
							continue
						}
						n := (rf.r2 - rf.r1 + 1) * (rf.c2 - rf.c1 + 1)
						if n > 4096 {
							indep = false
							break
						}
						tsh := book.sheets[rf.sheet]
						for cc := rf.c1; cc <= rf.c2 && indep; cc++ {
							for rr := rf.r1; rr <= rf.r2; rr++ {
								t := tsh.get(rr, cc)
								if t != nil && t.hasF {
									indep = false
									break
								}
							}
						}
						if !indep {
							break
						}
					}
					p.indep = indep
				}
			}(all[lo:hi])
		}
		wg.Wait()
	}
	for _, p := range all {
		if p.indep {
			jobs = append(jobs, job{p.si, p.r, p.c})
		}
	}
	if len(jobs) < 200 || nCPU < 2 {
		return
	}
	var wg sync.WaitGroup
	next := int64(0)
	for w := 0; w < nCPU; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				i := int(atomic.AddInt64(&next, 64)) - 64
				if i >= len(jobs) {
					return
				}
				end := i + 64
				if end > len(jobs) {
					end = len(jobs)
				}
				for _, j := range jobs[i:end] {
					book.cellValue(j.si, j.r, j.c)
				}
			}
		}()
	}
	wg.Wait()
}

func main() {
	var in, out string
	for i := 1; i < len(os.Args); i++ {
		switch os.Args[i] {
		case "--in":
			i++
			in = os.Args[i]
		case "--out":
			i++
			out = os.Args[i]
		}
	}
	if in == "" || out == "" {
		fmt.Fprintln(os.Stderr, "usage: calc eval --in book.xlsx --out values.json")
		os.Exit(2)
	}
	if pf := os.Getenv("CALC_PROF"); pf != "" {
		pff, _ := os.Create(pf)
		pprof.StartCPUProfile(pff)
		defer pprof.StopCPUProfile()
	}
	debug.SetGCPercent(-1)
	debug.SetMemoryLimit(3 << 30)
	t0 := time.Now()
	book, err := readBook(in)
	if os.Getenv("CALC_TIMING") != "" {
		fmt.Fprintln(os.Stderr, "load", time.Since(t0))
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	parallelPhase(book)
	if os.Getenv("CALC_TIMING") != "" {
		fmt.Fprintln(os.Stderr, "parallel", time.Since(t0))
	}
	f, err := os.Create(out)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	w := bufio.NewWriterSize(f, 1<<20)
	w.WriteByte('{')
	firstSheet := true
	for si, sh := range book.sheets {
		if !firstSheet {
			w.WriteByte(',')
		}
		firstSheet = false
		w.WriteString(jsonStr(sh.name))
		w.WriteString(":{")
		first := true
		for c := range sh.cols {
			for r, cell := range sh.cols[c] {
				if cell == nil || !cell.hasF {
					continue
				}
				v := book.cellValue(si, r, c)
				if !first {
					w.WriteByte(',')
				}
				first = false
				w.WriteByte('"')
				w.WriteString(colName(c))
				w.WriteString(strconv.Itoa(r + 1))
				w.WriteString(`":`)
				switch v.k {
				case KNum:
					x := v.n
					if math.IsNaN(x) || math.IsInf(x, 0) {
						w.WriteString(`{"t":"e","v":"#NUM!"}`)
					} else {
						w.WriteString(`{"t":"n","v":`)
						w.WriteString(strconv.FormatFloat(x, 'g', -1, 64))
						w.WriteByte('}')
					}
				case KStr:
					w.WriteString(`{"t":"s","v":`)
					w.WriteString(jsonStr(v.s))
					w.WriteByte('}')
				case KBool:
					if v.n != 0 {
						w.WriteString(`{"t":"b","v":true}`)
					} else {
						w.WriteString(`{"t":"b","v":false}`)
					}
				case KErr:
					w.WriteString(`{"t":"e","v":"`)
					w.WriteString(errNames[int(v.n)])
					w.WriteString(`"}`)
				default:
					w.WriteString(`{"t":"n","v":0}`)
				}
			}
		}
		w.WriteByte('}')
	}
	w.WriteByte('}')
	w.Flush()
	f.Close()
	if os.Getenv("CALC_TIMING") != "" {
		fmt.Fprintln(os.Stderr, "total", time.Since(t0))
	}
}
