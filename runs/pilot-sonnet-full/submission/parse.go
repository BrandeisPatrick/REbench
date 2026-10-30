package main

import (
	"strconv"
	"strings"
)

const (
	NConst uint8 = iota
	NRef
	NRange
	NCall
	NNeg
	NPos
	NPct
	NBin
)

type Node struct {
	op         uint8
	val        Val
	sh         *Sheet
	r1, c1     int
	r2, c2     int
	fn         Fn
	args       []*Node
	bop        byte
	name       string
	pass       bool
	naLatent   bool
	sibIfs     bool
}

type parser struct {
	s   string
	pos int
	wb  *Wb
	cur *Sheet
	err bool
}

func parseFormula(wb *Wb, cur *Sheet, f string) *Node {
	if strings.HasPrefix(f, "=") {
		f = f[1:]
	}
	p := &parser{s: f, wb: wb, cur: cur}
	n := p.parseCmp()
	p.ws()
	if p.err || p.pos < len(p.s) {
		return &Node{op: NConst, val: mkErr(eName)}
	}
	return n
}

func (p *parser) ws() {
	for p.pos < len(p.s) && (p.s[p.pos] == ' ' || p.s[p.pos] == '\t' || p.s[p.pos] == '\n' || p.s[p.pos] == '\r') {
		p.pos++
	}
}

func (p *parser) peek() byte {
	p.ws()
	if p.pos < len(p.s) {
		return p.s[p.pos]
	}
	return 0
}

func (p *parser) parseCmp() *Node {
	l := p.parseConcat()
	for {
		c := p.peek()
		var op byte
		switch c {
		case '=':
			op = '='
			p.pos++
		case '<':
			if p.pos+1 < len(p.s) && p.s[p.pos+1] == '=' {
				op = 'l'
				p.pos += 2
			} else if p.pos+1 < len(p.s) && p.s[p.pos+1] == '>' {
				op = 'n'
				p.pos += 2
			} else {
				op = '<'
				p.pos++
			}
		case '>':
			if p.pos+1 < len(p.s) && p.s[p.pos+1] == '=' {
				op = 'g'
				p.pos += 2
			} else {
				op = '>'
				p.pos++
			}
		default:
			return l
		}
		r := p.parseConcat()
		l = &Node{op: NBin, bop: op, args: []*Node{l, r}}
	}
}

func (p *parser) parseConcat() *Node {
	l := p.parseAdd()
	for p.peek() == '&' {
		p.pos++
		r := p.parseAdd()
		l = &Node{op: NBin, bop: '&', args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseAdd() *Node {
	l := p.parseMul()
	for {
		c := p.peek()
		if c != '+' && c != '-' {
			return l
		}
		p.pos++
		r := p.parseMul()
		l = &Node{op: NBin, bop: c, args: []*Node{l, r}}
	}
}

func (p *parser) parseMul() *Node {
	l := p.parsePow()
	for {
		c := p.peek()
		if c != '*' && c != '/' {
			return l
		}
		p.pos++
		r := p.parsePow()
		l = &Node{op: NBin, bop: c, args: []*Node{l, r}}
	}
}

func (p *parser) parsePow() *Node {
	l := p.parseUnary()
	for p.peek() == '^' {
		p.pos++
		r := p.parseUnary()
		l = &Node{op: NBin, bop: '^', args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseUnary() *Node {
	c := p.peek()
	if c == '-' {
		p.pos++
		return &Node{op: NNeg, args: []*Node{p.parseUnary()}}
	}
	if c == '+' {
		p.pos++
		return &Node{op: NPos, args: []*Node{p.parseUnary()}}
	}
	n := p.parsePrimary()
	for p.peek() == '%' {
		p.pos++
		n = &Node{op: NPct, args: []*Node{n}}
	}
	return n
}

func isIdent(c byte) bool {
	return c == '_' || c == '.' || c == '$' || (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}

func parseCellRef(s string) (r, c int, ok bool) {
	i := 0
	if i < len(s) && s[i] == '$' {
		i++
	}
	st := i
	for i < len(s) && ((s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= 'a' && s[i] <= 'z')) {
		i++
	}
	if i == st || i-st > 3 {
		return
	}
	c = colIdx(s[st:i])
	if i < len(s) && s[i] == '$' {
		i++
	}
	st = i
	for i < len(s) && s[i] >= '0' && s[i] <= '9' {
		i++
	}
	if i == st || i != len(s) {
		return
	}
	n, _ := strconv.Atoi(s[st:i])
	if n < 1 {
		return
	}
	return n - 1, c, true
}

func (p *parser) parsePrimary() *Node {
	c := p.peek()
	switch {
	case c == 0:
		p.err = true
		return &Node{op: NConst, val: mkErr(eName)}
	case c == '(':
		p.pos++
		n := p.parseCmp()
		if p.peek() != ')' {
			p.err = true
			return n
		}
		p.pos++
		return n
	case c == '"':
		p.pos++
		var sb strings.Builder
		for p.pos < len(p.s) {
			ch := p.s[p.pos]
			if ch == '"' {
				if p.pos+1 < len(p.s) && p.s[p.pos+1] == '"' {
					sb.WriteByte('"')
					p.pos += 2
					continue
				}
				p.pos++
				return &Node{op: NConst, val: mkStr(sb.String())}
			}
			sb.WriteByte(ch)
			p.pos++
		}
		p.err = true
		return &Node{op: NConst, val: mkErr(eName)}
	case c == '#':
		st := p.pos
		p.pos++
		for p.pos < len(p.s) && (isIdent(p.s[p.pos]) || p.s[p.pos] == '/' || p.s[p.pos] == '!' || p.s[p.pos] == '?') {
			p.pos++
		}
		w := strings.ToUpper(p.s[st:p.pos])
		for i, nm := range errNames {
			if nm == w {
				return &Node{op: NConst, val: mkErr(i)}
			}
		}
		p.err = true
		return &Node{op: NConst, val: mkErr(eName)}
	case (c >= '0' && c <= '9') || c == '.':
		st := p.pos
		for p.pos < len(p.s) && ((p.s[p.pos] >= '0' && p.s[p.pos] <= '9') || p.s[p.pos] == '.') {
			p.pos++
		}
		if p.pos < len(p.s) && (p.s[p.pos] == 'E' || p.s[p.pos] == 'e') {
			q := p.pos + 1
			if q < len(p.s) && (p.s[q] == '+' || p.s[q] == '-') {
				q++
			}
			if q < len(p.s) && p.s[q] >= '0' && p.s[q] <= '9' {
				for q < len(p.s) && p.s[q] >= '0' && p.s[q] <= '9' {
					q++
				}
				p.pos = q
			}
		}
		f, err := strconv.ParseFloat(p.s[st:p.pos], 64)
		if err != nil {
			p.err = true
		}
		return &Node{op: NConst, val: mkNum(f)}
	}
	// identifier / ref / function / sheet
	var sheetName string
	hasSheet := false
	if c == '\'' {
		p.pos++
		var sb strings.Builder
		for p.pos < len(p.s) {
			ch := p.s[p.pos]
			if ch == '\'' {
				if p.pos+1 < len(p.s) && p.s[p.pos+1] == '\'' {
					sb.WriteByte('\'')
					p.pos += 2
					continue
				}
				p.pos++
				break
			}
			sb.WriteByte(ch)
			p.pos++
		}
		if p.pos < len(p.s) && p.s[p.pos] == '!' {
			p.pos++
		} else {
			p.err = true
		}
		sheetName = sb.String()
		hasSheet = true
	} else {
		st := p.pos
		for p.pos < len(p.s) && isIdent(p.s[p.pos]) {
			p.pos++
		}
		if p.pos == st {
			p.err = true
			p.pos = len(p.s)
			return &Node{op: NConst, val: mkErr(eName)}
		}
		word := p.s[st:p.pos]
		if p.pos < len(p.s) && p.s[p.pos] == '!' {
			p.pos++
			sheetName = word
			hasSheet = true
		} else if p.pos < len(p.s) && p.s[p.pos] == '(' {
			return p.parseCall(word)
		} else {
			uw := strings.ToUpper(word)
			if uw == "TRUE" {
				return &Node{op: NConst, val: mkBool(true)}
			}
			if uw == "FALSE" {
				return &Node{op: NConst, val: mkBool(false)}
			}
			return p.finishRef(nil, false, word)
		}
	}
	_ = hasSheet
	// after sheet name: read ref word
	st := p.pos
	for p.pos < len(p.s) && isIdent(p.s[p.pos]) {
		p.pos++
	}
	word := p.s[st:p.pos]
	sh := p.wb.byName[strings.ToLower(sheetName)]
	return p.finishRef(sh, true, word)
}

func (p *parser) finishRef(sh *Sheet, explicit bool, word string) *Node {
	r1, c1, ok := parseCellRef(word)
	if !ok {
		p.err = true
		return &Node{op: NConst, val: mkErr(eName)}
	}
	r2, c2 := r1, c1
	isRange := false
	if p.pos < len(p.s) && p.s[p.pos] == ':' {
		p.pos++
		// optional sheet prefix on second part (ignored)
		st := p.pos
		for p.pos < len(p.s) && isIdent(p.s[p.pos]) {
			p.pos++
		}
		w2 := p.s[st:p.pos]
		if p.pos < len(p.s) && p.s[p.pos] == '!' {
			p.pos++
			st = p.pos
			for p.pos < len(p.s) && isIdent(p.s[p.pos]) {
				p.pos++
			}
			w2 = p.s[st:p.pos]
		}
		var ok2 bool
		r2, c2, ok2 = parseCellRef(w2)
		if !ok2 {
			p.err = true
			return &Node{op: NConst, val: mkErr(eName)}
		}
		isRange = true
	}
	if r2 < r1 {
		r1, r2 = r2, r1
	}
	if c2 < c1 {
		c1, c2 = c2, c1
	}
	if explicit && sh == nil {
		return &Node{op: NConst, val: mkErr(eRef)}
	}
	n := &Node{op: NRef, sh: sh, r1: r1, c1: c1, r2: r2, c2: c2}
	if isRange {
		n.op = NRange
	}
	if !explicit {
		n.sh = p.cur
	}
	return n
}

func (p *parser) parseCall(word string) *Node {
	name := strings.ToUpper(word)
	name = strings.TrimPrefix(name, "_XLFN.")
	p.pos++ // (
	var args []*Node
	if p.peek() == ')' {
		p.pos++
	} else {
		for {
			p.ws()
			if p.peek() == ',' || p.peek() == ')' {
				args = append(args, &Node{op: NConst, val: Val{k: KEmpty}})
			} else {
				args = append(args, p.parseCmp())
			}
			c := p.peek()
			if c == ',' {
				p.pos++
				continue
			}
			if c == ')' {
				p.pos++
				break
			}
			p.err = true
			break
		}
	}
	fn := funcTable[name]
	n := &Node{op: NCall, args: args, name: name, fn: fn}
	switch name {
	}
	hasIfs := false
	for _, a := range args {
		if a.op == NCall && a.name == "IFS" {
			hasIfs = true
		}
	}
	if hasIfs {
		for _, a := range args {
			if a.op == NCall && (a.name == "VLOOKUP" || a.name == "HLOOKUP" || a.name == "MATCH" || a.name == "XLOOKUP") {
				a.naLatent = true
			}
		}
	}
	switch name {
	case "IF", "IFS", "CHOOSE", "SWITCH", "IFERROR", "IFNA", "INDEX":
		n.pass = true
	}
	if fn == nil {
		n.fn = func(cx *Ctx, a []*Node) Val { return mkErr(eName) }
	}
	return n
}
