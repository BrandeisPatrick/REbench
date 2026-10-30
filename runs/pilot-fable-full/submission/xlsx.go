package main

import (
	"archive/zip"
	"strconv"
	"strings"
)

type Cell struct {
	k     Kind // KEmpty, KNum, KStr, KBool, KErr, or KFormula via f != nil
	n     float64
	s     string
	f     *Node
	state uint8
	val   Value
	ftxt  string
}

type Sheet struct {
	name  string
	nrows int
	ncols int
	cells []Cell // nrows*ncols
	forms []int  // indices of formula cells in file order
	fref  []string
}

type Book struct {
	sheets []*Sheet
	byName map[string]int
	idx    map[rangeKey]*colIndex
	cols   map[rangeKey]*colCache
}

func (s *Sheet) at(r, c int) *Cell {
	if r < 0 || c < 0 || r >= s.nrows || c >= s.ncols {
		return nil
	}
	return &s.cells[r*s.ncols+c]
}

func unescape(s string) string {
	if strings.IndexByte(s, '&') < 0 {
		return s
	}
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] != '&' {
			b.WriteByte(s[i])
			continue
		}
		j := strings.IndexByte(s[i:], ';')
		if j < 0 {
			b.WriteByte(s[i])
			continue
		}
		ent := s[i+1 : i+j]
		switch ent {
		case "lt":
			b.WriteByte('<')
		case "gt":
			b.WriteByte('>')
		case "amp":
			b.WriteByte('&')
		case "quot":
			b.WriteByte('"')
		case "apos":
			b.WriteByte('\'')
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
					b.WriteRune(rune(n))
				}
			} else {
				b.WriteString(s[i : i+j+1])
			}
		}
		i += j
	}
	return b.String()
}

// attribute value lookup within a tag string
func attr(tag, name string) (string, bool) {
	i := 0
	for {
		j := strings.Index(tag[i:], name+"=\"")
		if j < 0 {
			return "", false
		}
		p := i + j
		if p > 0 && (tag[p-1] == ' ' || tag[p-1] == '\t' || tag[p-1] == '\n') {
			st := p + len(name) + 2
			e := strings.IndexByte(tag[st:], '"')
			if e < 0 {
				return "", false
			}
			return tag[st : st+e], true
		}
		i = p + 1
	}
}

func colToIdx(s string) int {
	c := 0
	for i := 0; i < len(s); i++ {
		ch := s[i]
		if ch >= 'a' && ch <= 'z' {
			ch -= 32
		}
		c = c*26 + int(ch-'A'+1)
	}
	return c - 1
}

func parseRef(s string) (r, c int, ok bool) {
	i := 0
	for i < len(s) && ((s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= 'a' && s[i] <= 'z')) {
		i++
	}
	if i == 0 || i > 3 || i >= len(s) {
		return 0, 0, false
	}
	c = colToIdx(s[:i])
	n, err := strconv.Atoi(s[i:])
	if err != nil || n < 1 {
		return 0, 0, false
	}
	return n - 1, c, true
}

type rawCell struct {
	r, c int
	k    Kind
	n    float64
	s    string
	ftxt string
	ref  string
}

func parseSheetXML(data string, shared []string) []rawCell {
	var out []rawCell
	pos := 0
	sd := strings.Index(data, "<sheetData")
	if sd >= 0 {
		pos = sd
	}
	for {
		i := strings.Index(data[pos:], "<c ")
		if i < 0 {
			break
		}
		p := pos + i
		te := strings.IndexByte(data[p:], '>')
		if te < 0 {
			break
		}
		tag := data[p : p+te+1]
		selfClose := strings.HasSuffix(tag, "/>")
		ref, _ := attr(tag, "r")
		t, _ := attr(tag, "t")
		r, c, ok := parseRef(ref)
		pos = p + te + 1
		if selfClose {
			continue
		}
		end := strings.Index(data[pos:], "</c>")
		if end < 0 {
			break
		}
		body := data[pos : pos+end]
		pos = pos + end + 4
		if !ok {
			continue
		}
		rc := rawCell{r: r, c: c, ref: ref}
		// formula
		if fi := strings.Index(body, "<f"); fi >= 0 {
			fe := strings.IndexByte(body[fi:], '>')
			if fe >= 0 {
				ftag := body[fi : fi+fe+1]
				if !strings.HasSuffix(ftag, "/>") {
					fend := strings.Index(body[fi+fe+1:], "</f>")
					if fend >= 0 {
						rc.ftxt = unescape(body[fi+fe+1 : fi+fe+1+fend])
					}
				}
				if rc.ftxt == "" {
					rc.ftxt = "0"
				}
				rc.k = KFormula
				out = append(out, rc)
				continue
			}
		}
		var v string
		hasV := false
		if vi := strings.Index(body, "<v>"); vi >= 0 {
			ve := strings.Index(body[vi:], "</v>")
			if ve >= 0 {
				v = body[vi+3 : vi+ve]
				hasV = true
			}
		} else if strings.Contains(body, "<v/>") || strings.Contains(body, "<v></v>") {
			hasV = false
		}
		switch t {
		case "s":
			if hasV {
				idx, err := strconv.Atoi(v)
				if err == nil && idx >= 0 && idx < len(shared) {
					rc.k = KStr
					rc.s = shared[idx]
				}
			}
		case "str":
			rc.k = KStr
			rc.s = unescape(v)
		case "inlineStr":
			// concatenate all <t> contents
			var b strings.Builder
			q := 0
			for {
				ti := strings.Index(body[q:], "<t")
				if ti < 0 {
					break
				}
				tp := q + ti
				tte := strings.IndexByte(body[tp:], '>')
				if tte < 0 {
					break
				}
				if strings.HasSuffix(body[tp:tp+tte+1], "/>") {
					q = tp + tte + 1
					continue
				}
				tend := strings.Index(body[tp+tte+1:], "</t>")
				if tend < 0 {
					break
				}
				b.WriteString(unescape(body[tp+tte+1 : tp+tte+1+tend]))
				q = tp + tte + 1 + tend + 4
			}
			rc.k = KStr
			rc.s = b.String()
		case "b":
			rc.k = KBool
			if v == "1" || v == "true" {
				rc.n = 1
			}
		case "e":
			rc.k = KErr
			rc.s = v
		default:
			if hasV && v != "" {
				f, err := strconv.ParseFloat(v, 64)
				if err == nil {
					rc.k = KNum
					rc.n = f
				}
			}
		}
		if rc.k != KEmpty {
			out = append(out, rc)
		}
	}
	return out
}

func parseSharedStrings(data string) []string {
	var out []string
	pos := 0
	for {
		i := strings.Index(data[pos:], "<si>")
		if i < 0 {
			break
		}
		p := pos + i + 4
		e := strings.Index(data[p:], "</si>")
		if e < 0 {
			break
		}
		body := data[p : p+e]
		pos = p + e + 5
		var b strings.Builder
		q := 0
		for {
			ti := strings.Index(body[q:], "<t")
			if ti < 0 {
				break
			}
			tp := q + ti
			tte := strings.IndexByte(body[tp:], '>')
			if tte < 0 {
				break
			}
			if strings.HasSuffix(body[tp:tp+tte+1], "/>") {
				q = tp + tte + 1
				continue
			}
			tend := strings.Index(body[tp+tte+1:], "</t>")
			if tend < 0 {
				break
			}
			b.WriteString(unescape(body[tp+tte+1 : tp+tte+1+tend]))
			q = tp + tte + 1 + tend + 4
		}
		out = append(out, b.String())
	}
	return out
}

func readZipFile(z *zip.ReadCloser, name string) string {
	name = strings.TrimPrefix(name, "/")
	for _, f := range z.File {
		if f.Name == name {
			rc, err := f.Open()
			if err != nil {
				return ""
			}
			var sb strings.Builder
			buf := make([]byte, 1<<16)
			for {
				n, err := rc.Read(buf)
				sb.Write(buf[:n])
				if err != nil {
					break
				}
			}
			rc.Close()
			return sb.String()
		}
	}
	return ""
}

func loadBook(path string) (*Book, error) {
	z, err := zip.OpenReader(path)
	if err != nil {
		return nil, err
	}
	defer z.Close()
	wb := readZipFile(z, "xl/workbook.xml")
	rels := readZipFile(z, "xl/_rels/workbook.xml.rels")
	shared := parseSharedStrings(readZipFile(z, "xl/sharedStrings.xml"))
	// rels: Id -> Target
	relmap := map[string]string{}
	pos := 0
	for {
		i := strings.Index(rels[pos:], "<Relationship ")
		if i < 0 {
			break
		}
		p := pos + i
		e := strings.IndexByte(rels[p:], '>')
		if e < 0 {
			break
		}
		tag := rels[p : p+e+1]
		id, _ := attr(tag, "Id")
		tg, _ := attr(tag, "Target")
		relmap[id] = tg
		pos = p + e + 1
	}
	book := &Book{byName: map[string]int{}}
	pos = 0
	for {
		i := strings.Index(wb[pos:], "<sheet ")
		if i < 0 {
			break
		}
		p := pos + i
		e := strings.IndexByte(wb[p:], '>')
		if e < 0 {
			break
		}
		tag := wb[p : p+e+1]
		name, _ := attr(tag, "name")
		rid, _ := attr(tag, "r:id")
		if rid == "" {
			rid, _ = attr(tag, "id")
		}
		pos = p + e + 1
		target := relmap[rid]
		if target == "" {
			continue
		}
		if !strings.HasPrefix(target, "/") && !strings.HasPrefix(target, "xl/") {
			target = "xl/" + target
		}
		data := readZipFile(z, target)
		raw := parseSheetXML(data, shared)
		sh := &Sheet{name: unescape(name)}
		maxr, maxc := 0, 0
		for _, rc := range raw {
			if rc.r+1 > maxr {
				maxr = rc.r + 1
			}
			if rc.c+1 > maxc {
				maxc = rc.c + 1
			}
		}
		sh.nrows, sh.ncols = maxr, maxc
		sh.cells = make([]Cell, maxr*maxc)
		for _, rc := range raw {
			cl := &sh.cells[rc.r*maxc+rc.c]
			cl.k = rc.k
			cl.n = rc.n
			cl.s = rc.s
			if rc.k == KFormula {
				cl.ftxt = rc.ftxt
				sh.forms = append(sh.forms, rc.r*maxc+rc.c)
				sh.fref = append(sh.fref, rc.ref)
			}
		}
		book.byName[strings.ToLower(sh.name)] = len(book.sheets)
		book.sheets = append(book.sheets, sh)
	}
	return book, nil
}
