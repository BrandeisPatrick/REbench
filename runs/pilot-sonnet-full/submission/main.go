package main

import (
	"archive/zip"
	"bufio"
	"bytes"
	"fmt"
	"io"
	"math"
	"os"
	"runtime"
	"runtime/debug"
	"sync"
	"strconv"
	"strings"
	"time"
)

const (
	KEmpty uint8 = iota
	KNum
	KStr
	KBool
	KErr
)

type Val struct {
	k uint8
	n float64
	s string
}

var errNames = []string{"#DIV/0!", "#VALUE!", "#N/A", "#NUM!", "#REF!", "#NAME?", "#NULL!"}

const (
	eDiv = iota
	eVal
	eNA
	eNum
	eRef
	eName
	eNull
)

func mkErr(c int) Val        { return Val{k: KErr, n: float64(c)} }
func mkNum(f float64) Val    { return Val{k: KNum, n: f} }
func mkStr(s string) Val     { return Val{k: KStr, s: s} }
func mkBool(b bool) Val {
	if b {
		return Val{k: KBool, n: 1}
	}
	return Val{k: KBool, n: 0}
}

type Sheet struct {
	name string
	R, C int
	v    []Val
	fi   []int32
	st   []uint8
	// raw load
	raw []rawCell
	// caches
	idx map[idxKey]*lookupIdx
}

type rawCell struct {
	r, c int
	v    Val
	f    string
}

type Formula struct {
	sh   *Sheet
	r, c int
	root *Node
}

type Wb struct {
	sheets   []*Sheet
	byName   map[string]*Sheet
	formulas []*Formula
}

func colIdx(s string) int {
	n := 0
	for i := 0; i < len(s); i++ {
		ch := s[i]
		if ch >= 'a' && ch <= 'z' {
			ch -= 32
		}
		n = n*26 + int(ch-'A') + 1
	}
	return n - 1
}

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

func unesc(b []byte) string {
	if bytes.IndexByte(b, '&') < 0 {
		return string(b)
	}
	s := string(b)
	var sb strings.Builder
	for i := 0; i < len(s); {
		if s[i] == '&' {
			j := strings.IndexByte(s[i:], ';')
			if j > 0 {
				ent := s[i+1 : i+j]
				rep := ""
				ok := true
				switch ent {
				case "lt":
					rep = "<"
				case "gt":
					rep = ">"
				case "amp":
					rep = "&"
				case "quot":
					rep = "\""
				case "apos":
					rep = "'"
				default:
					if len(ent) > 1 && ent[0] == '#' {
						var n int64
						var err error
						if ent[1] == 'x' || ent[1] == 'X' {
							n, err = strconv.ParseInt(ent[2:], 16, 32)
						} else {
							n, err = strconv.ParseInt(ent[1:], 10, 32)
						}
						if err == nil {
							rep = string(rune(n))
						} else {
							ok = false
						}
					} else {
						ok = false
					}
				}
				if ok {
					sb.WriteString(rep)
					i += j + 1
					continue
				}
			}
		}
		sb.WriteByte(s[i])
		i++
	}
	return sb.String()
}

func readZip(zr *zip.Reader, name string) []byte {
	for _, f := range zr.File {
		if f.Name == name {
			rc, err := f.Open()
			if err != nil {
				return nil
			}
			b, _ := io.ReadAll(rc)
			rc.Close()
			return b
		}
	}
	return nil
}

func attr(tag []byte, name string) string {
	key := []byte(" " + name + "=\"")
	i := bytes.Index(tag, key)
	if i < 0 {
		return ""
	}
	i += len(key)
	j := bytes.IndexByte(tag[i:], '"')
	if j < 0 {
		return ""
	}
	return unesc(tag[i : i+j])
}

func parseShared(b []byte) []string {
	var out []string
	pos := 0
	for {
		i := bytes.Index(b[pos:], []byte("<si"))
		if i < 0 {
			break
		}
		i += pos
		// ensure <si> or <si ...>
		if i+3 < len(b) && b[i+3] != '>' && b[i+3] != ' ' {
			pos = i + 3
			continue
		}
		j := bytes.Index(b[i:], []byte("</si>"))
		if j < 0 {
			break
		}
		seg := b[i : i+j]
		var sb strings.Builder
		p := 0
		for {
			a := bytes.Index(seg[p:], []byte("<t"))
			if a < 0 {
				break
			}
			a += p
			if a+2 < len(seg) && seg[a+2] != '>' && seg[a+2] != ' ' {
				p = a + 2
				continue
			}
			gt := bytes.IndexByte(seg[a:], '>')
			if gt < 0 {
				break
			}
			if seg[a+gt-1] == '/' {
				p = a + gt
				continue
			}
			e := bytes.Index(seg[a+gt:], []byte("</t>"))
			if e < 0 {
				break
			}
			sb.WriteString(unesc(seg[a+gt+1 : a+gt+e]))
			p = a + gt + e
		}
		out = append(out, sb.String())
		pos = i + j
	}
	return out
}

func parseSheetXML(b []byte, sh *Sheet, shared []string) {
	if len(b) < 256*1024 {
		sh.raw = parseChunk(b, shared)
		return
	}
	start := bytes.Index(b, []byte("<sheetData"))
	if start < 0 {
		sh.raw = parseChunk(b, shared)
		return
	}
	nch := runtime.NumCPU()
	if nch > 8 {
		nch = 8
	}
	if nch < 2 {
		sh.raw = parseChunk(b, shared)
		return
	}
	cuts := []int{0}
	for k := 1; k < nch; k++ {
		off := start + (len(b)-start)*k/nch
		i := bytes.Index(b[off:], []byte("<row "))
		if i < 0 {
			break
		}
		if off+i > cuts[len(cuts)-1] {
			cuts = append(cuts, off+i)
		}
	}
	cuts = append(cuts, len(b))
	res := make([][]rawCell, len(cuts)-1)
	var wg sync.WaitGroup
	for k := 0; k < len(cuts)-1; k++ {
		wg.Add(1)
		go func(k int) {
			defer wg.Done()
			res[k] = parseChunk(b[cuts[k]:cuts[k+1]], shared)
		}(k)
	}
	wg.Wait()
	total := 0
	for _, r := range res {
		total += len(r)
	}
	all := make([]rawCell, 0, total)
	for _, r := range res {
		all = append(all, r...)
	}
	sh.raw = all
}

func parseChunk(b []byte, shared []string) []rawCell {
	var raw []rawCell
	pos := 0
	for {
		i := bytes.Index(b[pos:], []byte("<c "))
		if i < 0 {
			break
		}
		i += pos
		gt := bytes.IndexByte(b[i:], '>')
		if gt < 0 {
			break
		}
		tag := b[i : i+gt]
		ref := attr(tag, "r")
		t := attr(tag, "t")
		var body []byte
		next := i + gt + 1
		if b[i+gt-1] == '/' {
			pos = next
			continue
		}
		e := bytes.Index(b[next:], []byte("</c>"))
		if e < 0 {
			break
		}
		body = b[next : next+e]
		pos = next + e + 4
		// parse ref
		k := 0
		for k < len(ref) && ref[k] >= 'A' && ref[k] <= 'Z' {
			k++
		}
		c := colIdx(ref[:k])
		rr, _ := strconv.Atoi(ref[k:])
		rc := rawCell{r: rr - 1, c: c}
		// formula
		if fi := bytes.Index(body, []byte("<f")); fi >= 0 {
			fgt := bytes.IndexByte(body[fi:], '>')
			if fgt >= 0 && body[fi+fgt-1] != '/' {
				fe := bytes.Index(body[fi+fgt:], []byte("</f>"))
				if fe >= 0 {
					rc.f = unesc(body[fi+fgt+1 : fi+fgt+fe])
				}
			}
		}
		// value
		var vtxt []byte
		hasV := false
		if t == "inlineStr" {
			if a := bytes.Index(body, []byte("<is>")); a >= 0 {
				var sb strings.Builder
				seg := body[a:]
				p := 0
				for {
					x := bytes.Index(seg[p:], []byte("<t"))
					if x < 0 {
						break
					}
					x += p
					g := bytes.IndexByte(seg[x:], '>')
					if g < 0 {
						break
					}
					if seg[x+g-1] == '/' {
						p = x + g
						continue
					}
					y := bytes.Index(seg[x+g:], []byte("</t>"))
					if y < 0 {
						break
					}
					sb.WriteString(unesc(seg[x+g+1 : x+g+y]))
					p = x + g + y
				}
				rc.v = mkStr(sb.String())
			}
		} else {
			if a := bytes.Index(body, []byte("<v>")); a >= 0 {
				y := bytes.Index(body[a:], []byte("</v>"))
				if y >= 0 {
					vtxt = body[a+3 : a+y]
					hasV = true
				}
			}
			if hasV && len(vtxt) > 0 {
				switch t {
				case "s":
					n, _ := strconv.Atoi(string(vtxt))
					if n < len(shared) {
						rc.v = mkStr(shared[n])
					}
				case "str":
					rc.v = mkStr(unesc(vtxt))
				case "b":
					rc.v = mkBool(string(vtxt) == "1")
				case "e":
					s := string(vtxt)
					rc.v = mkErr(eVal)
					for ei, nm := range errNames {
						if nm == s {
							rc.v = mkErr(ei)
						}
					}
				default:
					f, err := strconv.ParseFloat(string(vtxt), 64)
					if err == nil {
						rc.v = mkNum(f)
					}
				}
			}
		}
		if rc.f != "" || rc.v.k != KEmpty {
			raw = append(raw, rc)
		}
	}
	return raw
}

func loadWb(path string) (*Wb, error) {
	zr, err := zip.OpenReader(path)
	if err != nil {
		return nil, err
	}
	defer zr.Close()
	wbx := readZip(&zr.Reader, "xl/workbook.xml")
	rels := readZip(&zr.Reader, "xl/_rels/workbook.xml.rels")
	relMap := map[string]string{}
	p := 0
	for {
		i := bytes.Index(rels[p:], []byte("<Relationship "))
		if i < 0 {
			break
		}
		i += p
		g := bytes.IndexByte(rels[i:], '>')
		tag := rels[i : i+g]
		relMap[attr(tag, "Id")] = attr(tag, "Target")
		p = i + g
	}
	wb := &Wb{byName: map[string]*Sheet{}}
	var shared []string
	if sb := readZip(&zr.Reader, "xl/sharedStrings.xml"); sb != nil {
		shared = parseShared(sb)
	}
	type sf struct {
		sh   *Sheet
		path string
	}
	var list []sf
	p = 0
	for {
		i := bytes.Index(wbx[p:], []byte("<sheet "))
		if i < 0 {
			break
		}
		i += p
		g := bytes.IndexByte(wbx[i:], '>')
		tag := wbx[i : i+g]
		name := attr(tag, "name")
		rid := attr(tag, "r:id")
		tgt := relMap[rid]
		if strings.HasPrefix(tgt, "/") {
			tgt = tgt[1:]
		} else {
			tgt = "xl/" + tgt
		}
		sh := &Sheet{name: name}
		wb.sheets = append(wb.sheets, sh)
		wb.byName[strings.ToLower(name)] = sh
		list = append(list, sf{sh, tgt})
		p = i + g
	}
	tl0 := time.Now()
	var wgs sync.WaitGroup
	for _, s := range list {
		wgs.Add(1)
		go func(s sf) {
			defer wgs.Done()
			b := readZip(&zr.Reader, s.path)
			parseSheetXML(b, s.sh, shared)
		}(s)
	}
	wgs.Wait()
	tl1 := time.Now()
	// allocate grids
	for _, sh := range wb.sheets {
		for _, rc := range sh.raw {
			if rc.r+1 > sh.R {
				sh.R = rc.r + 1
			}
			if rc.c+1 > sh.C {
				sh.C = rc.c + 1
			}
		}
		n := sh.R * sh.C
		sh.v = make([]Val, n)
		sh.fi = make([]int32, n)
		sh.st = make([]uint8, n)
		for i := range sh.fi {
			sh.fi[i] = -1
		}
	}
	for _, sh := range wb.sheets {
		for _, rc := range sh.raw {
			ix := rc.r*sh.C + rc.c
			if rc.f != "" {
				wb.formulas = append(wb.formulas, &Formula{sh: sh, r: rc.r, c: rc.c})
				sh.fi[ix] = int32(len(wb.formulas) - 1)
			} else {
				sh.v[ix] = rc.v
				sh.st[ix] = 2
			}
		}
	}
	tl2 := time.Now()
	// parse formulas
	type fsrc struct {
		f   *Formula
		sh  *Sheet
		src string
	}
	srcs := make([]fsrc, 0, len(wb.formulas))
	k := 0
	for _, sh := range wb.sheets {
		for _, rc := range sh.raw {
			if rc.f != "" {
				srcs = append(srcs, fsrc{wb.formulas[k], sh, rc.f})
				k++
			}
		}
		sh.raw = nil
	}
	nw := runtime.NumCPU()
	if nw > 8 {
		nw = 8
	}
	if len(srcs) < 2000 {
		nw = 1
	}
	var wgp sync.WaitGroup
	for w := 0; w < nw; w++ {
		wgp.Add(1)
		go func(w int) {
			defer wgp.Done()
			lo, hi := len(srcs)*w/nw, len(srcs)*(w+1)/nw
			for _, s := range srcs[lo:hi] {
				s.f.root = parseFormula(wb, s.sh, s.src)
			}
		}(w)
	}
	wgp.Wait()
	if os.Getenv("CALC_TIME") != "" {
		fmt.Fprintf(os.Stderr, "sheets %v alloc %v formulas %v\n", tl1.Sub(tl0), tl2.Sub(tl1), time.Since(tl2))
	}
	return wb, nil
}

func fmtJSONNum(buf []byte, f float64) []byte {
	if f == math.Trunc(f) && math.Abs(f) < 1e15 {
		if f == 0 && math.Signbit(f) {
			return append(buf, '0')
		}
		return strconv.AppendInt(buf, int64(f), 10)
	}
	return strconv.AppendFloat(buf, f, 'g', -1, 64)
}

func jsonStr(buf []byte, s string) []byte {
	buf = append(buf, '"')
	for _, r := range s {
		switch {
		case r == '"':
			buf = append(buf, '\\', '"')
		case r == '\\':
			buf = append(buf, '\\', '\\')
		case r < 0x20:
			buf = append(buf, []byte(fmt.Sprintf("\\u%04x", r))...)
		default:
			buf = utf8Append(buf, r)
		}
	}
	return append(buf, '"')
}

func utf8Append(buf []byte, r rune) []byte {
	return append(buf, string(r)...)
}

func main() {
	debug.SetGCPercent(-1)
	debug.SetMemoryLimit(3 << 30)
	args := os.Args[1:]
	if len(args) < 1 || args[0] != "eval" {
		fmt.Fprintln(os.Stderr, "usage: calc eval --in book.xlsx --out values.json")
		os.Exit(2)
	}
	in, out := "", ""
	for i := 1; i < len(args); i++ {
		if args[i] == "--in" && i+1 < len(args) {
			in = args[i+1]
			i++
		} else if args[i] == "--out" && i+1 < len(args) {
			out = args[i+1]
			i++
		}
	}
	t0 := time.Now()
	wb, err := loadWb(in)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	t1 := time.Now()
	wb.calcAll()
	t2 := time.Now()
	if os.Getenv("CALC_TIME") != "" {
		defer func() { fmt.Fprintf(os.Stderr, "load %v calc %v write %v\n", t1.Sub(t0), t2.Sub(t1), time.Since(t2)) }()
	}
	f, err := os.Create(out)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	w := bufio.NewWriterSize(f, 1<<20)
	buf := make([]byte, 0, 256)
	w.WriteByte('{')
	firstSheet := true
	for _, sh := range wb.sheets {
		if !firstSheet {
			w.WriteByte(',')
		}
		firstSheet = false
		buf = jsonStr(buf[:0], sh.name)
		buf = append(buf, ':', '{')
		w.Write(buf)
		first := true
		for r := 0; r < sh.R; r++ {
			for c := 0; c < sh.C; c++ {
				ix := r*sh.C + c
				if sh.fi[ix] < 0 {
					continue
				}
				v := sh.v[ix]
				buf = buf[:0]
				if !first {
					buf = append(buf, ',')
				}
				first = false
				buf = append(buf, '"')
				buf = append(buf, colName(c)...)
				buf = strconv.AppendInt(buf, int64(r+1), 10)
				buf = append(buf, '"', ':', '{', '"', 't', '"', ':')
				switch v.k {
				case KNum, KEmpty:
					buf = append(buf, `"n","v":`...)
					buf = fmtJSONNum(buf, v.n)
				case KStr:
					buf = append(buf, `"s","v":`...)
					buf = jsonStr(buf, v.s)
				case KBool:
					buf = append(buf, `"b","v":`...)
					if v.n != 0 {
						buf = append(buf, "true"...)
					} else {
						buf = append(buf, "false"...)
					}
				case KErr:
					buf = append(buf, `"e","v":`...)
					buf = jsonStr(buf, errNames[int(v.n)])
				}
				buf = append(buf, '}')
				w.Write(buf)
			}
		}
		w.WriteByte('}')
	}
	w.WriteByte('}')
	w.Flush()
	f.Close()
}
