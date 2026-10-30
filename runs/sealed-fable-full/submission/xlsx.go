package main

import (
	"archive/zip"
	"io"
	"regexp"
	"strconv"
	"strings"
)

type Cell struct {
	v       Val
	formula string
	ast     Node
	state   uint8 // 0 none, 1 computing, 2 done
	hasF    bool
}

type Sheet struct {
	name string
	cols [][]*Cell // [col][row]
	nrow int
}

type Book struct {
	sheets []*Sheet
	byName map[string]int
}

func (s *Sheet) get(r, c int) *Cell {
	if c < 0 || c >= len(s.cols) {
		return nil
	}
	col := s.cols[c]
	if r < 0 || r >= len(col) {
		return nil
	}
	return col[r]
}

func (s *Sheet) set(r, c int, cell *Cell) {
	for c >= len(s.cols) {
		s.cols = append(s.cols, nil)
	}
	for r >= len(s.cols[c]) {
		s.cols[c] = append(s.cols[c], nil)
	}
	s.cols[c][r] = cell
	if r+1 > s.nrow {
		s.nrow = r + 1
	}
}

func unescapeXML(s string) string {
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

func attr(tag, name string) (string, bool) {
	key := " " + name + "=\""
	i := strings.Index(tag, key)
	if i < 0 {
		return "", false
	}
	i += len(key)
	j := strings.IndexByte(tag[i:], '"')
	if j < 0 {
		return "", false
	}
	return tag[i : i+j], true
}

func parseCellRef(ref string) (row, col int) {
	col = 0
	i := 0
	for i < len(ref) && ref[i] >= 'A' && ref[i] <= 'Z' {
		col = col*26 + int(ref[i]-'A'+1)
		i++
	}
	row, _ = strconv.Atoi(ref[i:])
	return row - 1, col - 1
}

// extract text content of <t> elements within s
func extractT(s string) string {
	var b strings.Builder
	for {
		i := strings.Index(s, "<t")
		if i < 0 {
			break
		}
		// ensure it's <t> or <t ...>
		if i+2 < len(s) && s[i+2] != '>' && s[i+2] != ' ' {
			s = s[i+2:]
			continue
		}
		k := strings.IndexByte(s[i:], '>')
		if k < 0 {
			break
		}
		if s[i+k-1] == '/' {
			s = s[i+k+1:]
			continue
		}
		rest := s[i+k+1:]
		e := strings.Index(rest, "</t>")
		if e < 0 {
			break
		}
		b.WriteString(unescapeXML(rest[:e]))
		s = rest[e+4:]
	}
	return b.String()
}

var reSheet = regexp.MustCompile(`<sheet [^>]*>`)

func readBook(path string) (*Book, error) {
	zr, err := zip.OpenReader(path)
	if err != nil {
		return nil, err
	}
	defer zr.Close()
	files := map[string][]byte{}
	for _, f := range zr.File {
		if strings.HasPrefix(f.Name, "xl/worksheets/") || f.Name == "xl/workbook.xml" || f.Name == "xl/sharedStrings.xml" || f.Name == "xl/_rels/workbook.xml.rels" {
			rc, err := f.Open()
			if err != nil {
				return nil, err
			}
			data, err := io.ReadAll(rc)
			rc.Close()
			if err != nil {
				return nil, err
			}
			files[f.Name] = data
		}
	}
	// shared strings
	var sst []string
	if d, ok := files["xl/sharedStrings.xml"]; ok {
		s := string(d)
		for {
			i := strings.Index(s, "<si>")
			if i < 0 {
				break
			}
			e := strings.Index(s[i:], "</si>")
			if e < 0 {
				break
			}
			sst = append(sst, extractT(s[i:i+e]))
			s = s[i+e+5:]
		}
	}
	// rels
	rels := map[string]string{}
	if d, ok := files["xl/_rels/workbook.xml.rels"]; ok {
		s := string(d)
		for _, m := range regexp.MustCompile(`<Relationship [^>]*>`).FindAllString(s, -1) {
			id, _ := attr(m, "Id")
			tg, _ := attr(m, "Target")
			tg = strings.TrimPrefix(tg, "/xl/")
			tg = strings.TrimPrefix(tg, "xl/")
			rels[id] = tg
		}
	}
	book := &Book{byName: map[string]int{}}
	wb := string(files["xl/workbook.xml"])
	for _, m := range reSheet.FindAllString(wb, -1) {
		name, _ := attr(m, "name")
		rid, _ := attr(m, "r:id")
		target := rels[rid]
		data, ok := files["xl/"+target]
		if !ok {
			data = files["xl/worksheets/sheet"+strconv.Itoa(len(book.sheets)+1)+".xml"]
		}
		sh := &Sheet{name: unescapeXML(name)}
		parseSheet(sh, string(data), sst)
		book.byName[sh.name] = len(book.sheets)
		book.sheets = append(book.sheets, sh)
	}
	return book, nil
}

func parseSheet(sh *Sheet, s string, sst []string) {
	i := strings.Index(s, "<sheetData")
	if i < 0 {
		return
	}
	s = s[i:]
	for {
		i := strings.Index(s, "<c ")
		if i < 0 {
			break
		}
		k := strings.IndexByte(s[i:], '>')
		if k < 0 {
			break
		}
		tag := s[i : i+k+1]
		selfClose := tag[len(tag)-2] == '/'
		ref, _ := attr(tag, "r")
		t, _ := attr(tag, "t")
		row, col := parseCellRef(ref)
		if selfClose {
			s = s[i+k+1:]
			continue
		}
		rest := s[i+k+1:]
		e := strings.Index(rest, "</c>")
		if e < 0 {
			break
		}
		body := rest[:e]
		s = rest[e+4:]
		cell := &Cell{}
		// formula
		if fi := strings.Index(body, "<f"); fi >= 0 {
			fk := strings.IndexByte(body[fi:], '>')
			if fk >= 0 && body[fi+fk-1] != '/' {
				fe := strings.Index(body[fi+fk+1:], "</f>")
				if fe >= 0 {
					cell.formula = unescapeXML(body[fi+fk+1 : fi+fk+1+fe])
					cell.hasF = true
				}
			}
		}
		if !cell.hasF {
			var vtext string
			hasV := false
			if vi := strings.Index(body, "<v>"); vi >= 0 {
				ve := strings.Index(body[vi:], "</v>")
				if ve >= 0 {
					vtext = body[vi+3 : vi+ve]
					hasV = true
				}
			}
			switch t {
			case "s":
				idx, _ := strconv.Atoi(vtext)
				if idx >= 0 && idx < len(sst) {
					cell.v = strVal(sst[idx])
				} else {
					cell.v = strVal("")
				}
			case "inlineStr":
				cell.v = strVal(extractT(body))
			case "str":
				cell.v = strVal(unescapeXML(vtext))
			case "b":
				cell.v = boolVal(vtext == "1" || vtext == "true")
			case "e":
				cell.v = errVal(errFromString(unescapeXML(vtext)))
			default:
				if !hasV {
					continue
				}
				f, err := strconv.ParseFloat(strings.TrimSpace(vtext), 64)
				if err != nil {
					continue
				}
				cell.v = numVal(f)
			}
			cell.state = 2
			if cell.v.k == KStr && cell.v.s == "" {
				continue
			}
		}
		sh.set(row, col, cell)
	}
}
