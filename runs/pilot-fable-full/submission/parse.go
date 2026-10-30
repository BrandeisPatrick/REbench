package main

import (
	"strconv"
	"strings"
)

type NodeOp uint8

const (
	NNum NodeOp = iota
	NStr
	NBool
	NErr
	NRef
	NFunc
	NBin
	NUnm
	NPct
)

type Node struct {
	op    NodeOp
	num   float64
	str   string
	bop   byte // for NBin: + - * / ^ & = < > l(<=) g(>=) n(<>)
	fn    string
	args  []*Node
	sheet int // -1 = current
	r1, c1, r2, c2 int
	isRange bool
	cv      Value
	cached  bool
	errAware, isJump, isLogical bool
}

type lexer struct {
	s   string
	pos int
	book *Book
}

type token struct {
	kind byte // n number, s string, i ident, o operator, ( ) , r ref, e error, E end
	val  string
	num  float64
	node *Node
}

func isAlpha(c byte) bool { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') }
func isDigit(c byte) bool { return c >= '0' && c <= '9' }

// try to parse a cell ref like $A$1 at position i; returns row, col, end, ok
func scanCell(s string, i int) (r, c, end int, ok bool) {
	j := i
	if j < len(s) && s[j] == '$' {
		j++
	}
	cs := j
	for j < len(s) && isAlpha(s[j]) {
		j++
	}
	if j == cs || j-cs > 3 {
		return 0, 0, 0, false
	}
	col := s[cs:j]
	if j < len(s) && s[j] == '$' {
		j++
	}
	rs := j
	for j < len(s) && isDigit(s[j]) {
		j++
	}
	if j == rs || j-rs > 7 {
		return 0, 0, 0, false
	}
	// must not be followed by alnum or '(' (function name)
	if j < len(s) && (isAlpha(s[j]) || isDigit(s[j]) || s[j] == '(' || s[j] == '_' || s[j] == '.') {
		return 0, 0, 0, false
	}
	n, _ := strconv.Atoi(s[rs:j])
	return n - 1, colToIdx(strings.ToUpper(col)), j, true
}

func (lx *lexer) next() token {
	s := lx.s
	for lx.pos < len(s) && (s[lx.pos] == ' ' || s[lx.pos] == '\n' || s[lx.pos] == '\t') {
		lx.pos++
	}
	if lx.pos >= len(s) {
		return token{kind: 'E'}
	}
	c := s[lx.pos]
	// string
	if c == '"' {
		var b strings.Builder
		i := lx.pos + 1
		for i < len(s) {
			if s[i] == '"' {
				if i+1 < len(s) && s[i+1] == '"' {
					b.WriteByte('"')
					i += 2
					continue
				}
				break
			}
			b.WriteByte(s[i])
			i++
		}
		lx.pos = i + 1
		return token{kind: 's', val: b.String()}
	}
	// error literal
	if c == '#' {
		for _, e := range []string{"#N/A", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#NULL!"} {
			if strings.HasPrefix(s[lx.pos:], e) {
				lx.pos += len(e)
				return token{kind: 'e', val: e}
			}
		}
	}
	// number
	if isDigit(c) || (c == '.' && lx.pos+1 < len(s) && isDigit(s[lx.pos+1])) {
		i := lx.pos
		for i < len(s) && (isDigit(s[i]) || s[i] == '.') {
			i++
		}
		if i < len(s) && (s[i] == 'E' || s[i] == 'e') {
			j := i + 1
			if j < len(s) && (s[j] == '+' || s[j] == '-') {
				j++
			}
			if j < len(s) && isDigit(s[j]) {
				for j < len(s) && isDigit(s[j]) {
					j++
				}
				i = j
			}
		}
		f, _ := strconv.ParseFloat(s[lx.pos:i], 64)
		lx.pos = i
		return token{kind: 'n', num: f}
	}
	// sheet-qualified ref: 'name'!ref or name!ref
	if c == '\'' {
		i := lx.pos + 1
		var b strings.Builder
		for i < len(s) {
			if s[i] == '\'' {
				if i+1 < len(s) && s[i+1] == '\'' {
					b.WriteByte('\'')
					i += 2
					continue
				}
				break
			}
			b.WriteByte(s[i])
			i++
		}
		i++ // closing quote
		if i < len(s) && s[i] == '!' {
			nd := lx.refAfter(i+1, b.String())
			if nd != nil {
				return token{kind: 'r', node: nd}
			}
		}
		lx.pos = i
		return token{kind: 'E'}
	}
	if isAlpha(c) || c == '$' || c == '_' {
		// identifier: letters digits _ .
		i := lx.pos
		for i < len(s) && (isAlpha(s[i]) || isDigit(s[i]) || s[i] == '_' || s[i] == '.' || s[i] == '$') {
			i++
		}
		if i < len(s) && s[i] == '!' {
			name := s[lx.pos:i]
			nd := lx.refAfter(i+1, name)
			if nd != nil {
				return token{kind: 'r', node: nd}
			}
		}
		// cell ref?
		if nd := lx.refAfter(lx.pos, ""); nd != nil {
			return token{kind: 'r', node: nd}
		}
		ident := s[lx.pos:i]
		lx.pos = i
		return token{kind: 'i', val: ident}
	}
	lx.pos++
	switch c {
	case '(', ')', ',', ';':
		if c == ';' {
			c = ','
		}
		return token{kind: c}
	case '<':
		if lx.pos < len(s) && s[lx.pos] == '=' {
			lx.pos++
			return token{kind: 'o', val: "l"}
		}
		if lx.pos < len(s) && s[lx.pos] == '>' {
			lx.pos++
			return token{kind: 'o', val: "n"}
		}
		return token{kind: 'o', val: "<"}
	case '>':
		if lx.pos < len(s) && s[lx.pos] == '=' {
			lx.pos++
			return token{kind: 'o', val: "g"}
		}
		return token{kind: 'o', val: ">"}
	}
	return token{kind: 'o', val: string(c)}
}

// parse ref at position i with sheet name (may be ""); sets lx.pos on success
func (lx *lexer) refAfter(i int, sheet string) *Node {
	s := lx.s
	r1, c1, e1, ok := scanCell(s, i)
	if !ok {
		return nil
	}
	nd := &Node{op: NRef, r1: r1, c1: c1, r2: r1, c2: c1, sheet: -1}
	if sheet != "" {
		idx, found := lx.book.byName[strings.ToLower(sheet)]
		if !found {
			nd.sheet = -2 // invalid sheet -> #REF!
		} else {
			nd.sheet = idx
		}
	}
	if e1 < len(s) && s[e1] == ':' {
		r2, c2, e2, ok2 := scanCell(s, e1+1)
		if ok2 {
			nd.r2, nd.c2 = r2, c2
			nd.isRange = true
			if nd.r1 > nd.r2 {
				nd.r1, nd.r2 = nd.r2, nd.r1
			}
			if nd.c1 > nd.c2 {
				nd.c1, nd.c2 = nd.c2, nd.c1
			}
			e1 = e2
		}
	}
	lx.pos = e1
	return nd
}

type parser struct {
	lx  *lexer
	tok token
	err bool
}

func (p *parser) advance() { p.tok = p.lx.next() }

func parseFormula(s string, book *Book) *Node {
	p := &parser{lx: &lexer{s: s, book: book}}
	p.advance()
	n := p.parseCmp()
	if p.tok.kind != 'E' {
		p.err = true
	}
	if p.err || n == nil {
		return &Node{op: NErr, str: "#NAME?"}
	}
	return n
}

func (p *parser) parseCmp() *Node {
	l := p.parseConcat()
	for p.tok.kind == 'o' && (p.tok.val == "=" || p.tok.val == "<" || p.tok.val == ">" || p.tok.val == "l" || p.tok.val == "g" || p.tok.val == "n") {
		op := p.tok.val[0]
		p.advance()
		r := p.parseConcat()
		l = &Node{op: NBin, bop: op, args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseConcat() *Node {
	l := p.parseAdd()
	for p.tok.kind == 'o' && p.tok.val == "&" {
		p.advance()
		r := p.parseAdd()
		l = &Node{op: NBin, bop: '&', args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseAdd() *Node {
	l := p.parseMul()
	for p.tok.kind == 'o' && (p.tok.val == "+" || p.tok.val == "-") {
		op := p.tok.val[0]
		p.advance()
		r := p.parseMul()
		l = &Node{op: NBin, bop: op, args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseMul() *Node {
	l := p.parsePow()
	for p.tok.kind == 'o' && (p.tok.val == "*" || p.tok.val == "/") {
		op := p.tok.val[0]
		p.advance()
		r := p.parsePow()
		l = &Node{op: NBin, bop: op, args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parsePow() *Node {
	l := p.parseUnary()
	for p.tok.kind == 'o' && p.tok.val == "^" {
		p.advance()
		r := p.parseUnary()
		l = &Node{op: NBin, bop: '^', args: []*Node{l, r}}
	}
	return l
}

func (p *parser) parseUnary() *Node {
	if p.tok.kind == 'o' && p.tok.val == "-" {
		p.advance()
		n := p.parseUnary()
		return &Node{op: NUnm, args: []*Node{n}}
	}
	if p.tok.kind == 'o' && p.tok.val == "+" {
		p.advance()
		return p.parseUnary()
	}
	return p.parsePostfix()
}

func (p *parser) parsePostfix() *Node {
	n := p.parsePrimary()
	for p.tok.kind == 'o' && p.tok.val == "%" {
		p.advance()
		n = &Node{op: NPct, args: []*Node{n}}
	}
	return n
}

func (p *parser) parsePrimary() *Node {
	t := p.tok
	switch t.kind {
	case 'n':
		p.advance()
		return &Node{op: NNum, num: t.num}
	case 's':
		p.advance()
		return &Node{op: NStr, str: t.val}
	case 'e':
		p.advance()
		return &Node{op: NErr, str: t.val}
	case 'r':
		p.advance()
		return t.node
	case '(':
		p.advance()
		n := p.parseCmp()
		if p.tok.kind != ')' {
			p.err = true
			return nil
		}
		p.advance()
		return n
	case 'i':
		name := strings.ToUpper(t.val)
		name = strings.TrimPrefix(name, "_XLFN.")
		name = strings.TrimPrefix(name, "_XLWS.")
		p.advance()
		if p.tok.kind == '(' {
			p.advance()
			var args []*Node
			if p.tok.kind == ')' {
				p.advance()
				return &Node{op: NFunc, fn: name, args: args, errAware: errAware[name], isJump: jumpFn[name], isLogical: logicalFn[name]}
			}
			for {
				if p.tok.kind == ',' {
					args = append(args, nil) // missing arg
					p.advance()
					continue
				}
				if p.tok.kind == ')' {
					args = append(args, nil)
					p.advance()
					break
				}
				a := p.parseCmp()
				args = append(args, a)
				if p.tok.kind == ',' {
					p.advance()
					if p.tok.kind == ')' {
						args = append(args, nil)
						p.advance()
						break
					}
					continue
				}
				if p.tok.kind == ')' {
					p.advance()
					break
				}
				p.err = true
				return nil
			}
			return &Node{op: NFunc, fn: name, args: args, errAware: errAware[name], isJump: jumpFn[name], isLogical: logicalFn[name]}
		}
		if name == "TRUE" {
			return &Node{op: NBool, num: 1}
		}
		if name == "FALSE" {
			return &Node{op: NBool, num: 0}
		}
		return &Node{op: NErr, str: "#NAME?"}
	}
	p.err = true
	return nil
}
