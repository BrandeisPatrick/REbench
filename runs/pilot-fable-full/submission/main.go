package main

import (
	"bufio"
	"fmt"
	"math"
	"os"
	"runtime"
	"strconv"
	"sync"
	"time"
)

func writeJSONString(w *bufio.Writer, s string) {
	w.WriteByte('"')
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch c {
		case '"':
			w.WriteString("\\\"")
		case '\\':
			w.WriteString("\\\\")
		case '\n':
			w.WriteString("\\n")
		case '\r':
			w.WriteString("\\r")
		case '\t':
			w.WriteString("\\t")
		default:
			if c < 0x20 {
				fmt.Fprintf(w, "\\u%04x", c)
			} else {
				w.WriteByte(c)
			}
		}
	}
	w.WriteByte('"')
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
	t0 := time.Now()
	book, err := loadBook(in)
	if err == nil {
		// parse all formulas in parallel
		var wg sync.WaitGroup
		for _, sh := range book.sheets {
			nw := runtime.NumCPU()
			if nw > 8 {
				nw = 8
			}
			chunk := (len(sh.forms) + nw - 1) / nw
			for w := 0; w < nw; w++ {
				lo := w * chunk
				hi := lo + chunk
				if hi > len(sh.forms) {
					hi = len(sh.forms)
				}
				if lo >= hi {
					continue
				}
				wg.Add(1)
				go func(sh *Sheet, lo, hi int) {
					defer wg.Done()
					for _, idx := range sh.forms[lo:hi] {
						cl := &sh.cells[idx]
						cl.f = parseFormula(cl.ftxt, book)
					}
				}(sh, lo, hi)
			}
		}
		wg.Wait()
	}
	if os.Getenv("CALC_TIME") != "" {
		fmt.Fprintln(os.Stderr, "load", time.Since(t0))
		t1 := time.Now()
		for _, sh := range book.sheets {
			for _, idx := range sh.forms {
				book.cellValue(sh, idx/sh.ncols, idx%sh.ncols)
			}
		}
		fmt.Fprintln(os.Stderr, "eval", time.Since(t1))
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "load error:", err)
		os.Exit(1)
	}
	f, err := os.Create(out)
	if err != nil {
		fmt.Fprintln(os.Stderr, "write error:", err)
		os.Exit(1)
	}
	w := bufio.NewWriterSize(f, 1<<20)
	w.WriteByte('{')
	firstSheet := true
	for _, sh := range book.sheets {
		if len(sh.forms) == 0 {
			continue
		}
		if !firstSheet {
			w.WriteByte(',')
		}
		firstSheet = false
		writeJSONString(w, sh.name)
		w.WriteString(":{")
		for i, idx := range sh.forms {
			r := idx / sh.ncols
			c := idx % sh.ncols
			v := book.cellValue(sh, r, c)
			if i > 0 {
				w.WriteByte(',')
			}
			writeJSONString(w, sh.fref[i])
			w.WriteString(":{\"t\":\"")
			switch v.k {
			case KNum:
				if math.IsInf(v.n, 0) || math.IsNaN(v.n) {
					w.WriteString("e\",\"v\":\"#NUM!\"}")
				} else {
					w.WriteString("n\",\"v\":")
					w.WriteString(strconv.FormatFloat(v.n, 'g', -1, 64))
					w.WriteByte('}')
				}
			case KStr:
				w.WriteString("s\",\"v\":")
				writeJSONString(w, v.s)
				w.WriteByte('}')
			case KBool:
				if v.n != 0 {
					w.WriteString("b\",\"v\":true}")
				} else {
					w.WriteString("b\",\"v\":false}")
				}
			case KErr:
				w.WriteString("e\",\"v\":")
				writeJSONString(w, v.s)
				w.WriteByte('}')
			default:
				w.WriteString("n\",\"v\":0}")
			}
		}
		w.WriteByte('}')
	}
	w.WriteByte('}')
	w.Flush()
	f.Close()
}
