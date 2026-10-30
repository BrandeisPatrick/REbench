package main

import (
	"math"
	"strconv"
	"strings"
)

type Kind uint8

const (
	KEmpty Kind = iota
	KNum
	KStr
	KBool
	KErr
	KRef
)

const (
	ErrNull  = 1
	ErrDiv0  = 2
	ErrValue = 3
	ErrRef   = 4
	ErrName  = 5
	ErrNum   = 6
	ErrNA    = 7
)

var errNames = []string{"", "#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"}

func errFromString(s string) int {
	for i, n := range errNames {
		if n == s {
			return i
		}
	}
	return ErrValue
}

type Ref struct {
	sheet          int
	r1, c1, r2, c2 int
}

type Val struct {
	k   Kind
	n   float64
	s   string
	ref Ref
}

func numVal(f float64) Val {
	if math.IsInf(f, 0) || math.IsNaN(f) {
		return Val{k: KErr, n: ErrNum}
	}
	return Val{k: KNum, n: f}
}

type ValNode struct{ v Val }

func strVal(s string) Val { return Val{k: KStr, s: s} }
func boolVal(b bool) Val {
	if b {
		return Val{k: KBool, n: 1}
	}
	return Val{k: KBool, n: 0}
}
func errVal(e int) Val { return Val{k: KErr, n: float64(e)} }

var emptyVal = Val{k: KEmpty}

// ---------- tokenizer ----------

type tokKind uint8

const (
	tNum tokKind = iota
	tStr
	tIdent
	tRef
	tOp
	tLParen
	tRParen
	tComma
	tEOF
)

type token struct {
	k tokKind
	s string
	n float64
}

func isLetter(c byte) bool { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') }
func isDigit(c byte) bool  { return c >= '0' && c <= '9' }

func tokenize(f string) []token {
	toks := make([]token, 0, len(f)/3+4)
	i := 0
	n := len(f)
	for i < n {
		c := f[i]
		switch {
		case c == ' ' || c == '\n' || c == '\r' || c == '\t':
			i++
		case isDigit(c) || (c == '.' && i+1 < n && isDigit(f[i+1])):
			j := i
			for j < n && (isDigit(f[j]) || f[j] == '.') {
				j++
			}
			if j < n && (f[j] == 'E' || f[j] == 'e') {
				k := j + 1
				if k < n && (f[k] == '+' || f[k] == '-') {
					k++
				}
				if k < n && isDigit(f[k]) {
					for k < n && isDigit(f[k]) {
						k++
					}
					j = k
				}
			}
			v, _ := strconv.ParseFloat(f[i:j], 64)
			toks = append(toks, token{k: tNum, n: v})
			i = j
		case c == '"':
			var b strings.Builder
			j := i + 1
			for j < n {
				if f[j] == '"' {
					if j+1 < n && f[j+1] == '"' {
						b.WriteByte('"')
						j += 2
						continue
					}
					break
				}
				b.WriteByte(f[j])
				j++
			}
			toks = append(toks, token{k: tStr, s: b.String()})
			i = j + 1
		case c == '\'':
			// quoted sheet name
			j := i + 1
			var b strings.Builder
			for j < n {
				if f[j] == '\'' {
					if j+1 < n && f[j+1] == '\'' {
						b.WriteByte('\'')
						j += 2
						continue
					}
					break
				}
				b.WriteByte(f[j])
				j++
			}
			j++ // skip closing quote
			// expect '!'
			if j < n && f[j] == '!' {
				j++
			}
			k := j
			for k < n && (isLetter(f[k]) || isDigit(f[k]) || f[k] == '$' || f[k] == ':') {
				k++
			}
			toks = append(toks, token{k: tRef, s: b.String() + "!" + f[j:k]})
			i = k
		case isLetter(c) || c == '$' || c == '_':
			j := i
			for j < n && (isLetter(f[j]) || isDigit(f[j]) || f[j] == '$' || f[j] == '_' || f[j] == '.') {
				j++
			}
			if j < n && f[j] == '!' {
				// sheet-qualified
				k := j + 1
				for k < n && (isLetter(f[k]) || isDigit(f[k]) || f[k] == '$' || f[k] == ':') {
					k++
				}
				toks = append(toks, token{k: tRef, s: f[i:k]})
				i = k
			} else if j < n && f[j] == '(' {
				toks = append(toks, token{k: tIdent, s: strings.ToUpper(f[i:j])})
				i = j
			} else {
				word := f[i:j]
				// is it a cell ref? maybe with ':'
				if j < n && f[j] == ':' {
					k := j + 1
					for k < n && (isLetter(f[k]) || isDigit(f[k]) || f[k] == '$') {
						k++
					}
					toks = append(toks, token{k: tRef, s: f[i:k]})
					i = k
				} else if looksLikeRef(word) {
					toks = append(toks, token{k: tRef, s: word})
					i = j
				} else {
					toks = append(toks, token{k: tIdent, s: strings.ToUpper(word)})
					i = j
				}
			}
		case c == '(':
			toks = append(toks, token{k: tLParen})
			i++
		case c == ')':
			toks = append(toks, token{k: tRParen})
			i++
		case c == ',' || c == ';':
			toks = append(toks, token{k: tComma})
			i++
		case c == '<' || c == '>':
			if i+1 < n && (f[i+1] == '=' || (c == '<' && f[i+1] == '>')) {
				toks = append(toks, token{k: tOp, s: f[i : i+2]})
				i += 2
			} else {
				toks = append(toks, token{k: tOp, s: f[i : i+1]})
				i++
			}
		default:
			toks = append(toks, token{k: tOp, s: f[i : i+1]})
			i++
		}
	}
	toks = append(toks, token{k: tEOF})
	return toks
}

func looksLikeRef(w string) bool {
	i := 0
	if i < len(w) && w[i] == '$' {
		i++
	}
	j := i
	for j < len(w) && isLetter(w[j]) {
		j++
	}
	if j == i || j-i > 3 {
		return false
	}
	if j < len(w) && w[j] == '$' {
		j++
	}
	k := j
	for k < len(w) && isDigit(w[k]) {
		k++
	}
	return k > j && k == len(w)
}

// ---------- AST ----------

type Node interface{}

type NumNode struct{ v float64 }
type StrNode struct{ s string }
type BoolNode struct{ b bool }
type ErrNode struct{ e int }
type RefNode struct{ ref Ref }
type BinNode struct {
	op   string
	l, r Node
}
type UnNode struct {
	op string
	x  Node
}
type FuncNode struct {
	name string
	fn   fnImpl
	args []Node
}

type parser struct {
	toks []token
	pos  int
	book *Book
	sh   int
}

func (p *parser) peek() token { return p.toks[p.pos] }
func (p *parser) next() token { t := p.toks[p.pos]; p.pos++; return t }

func parseFormula(f string, book *Book, sh int) (n Node) {
	defer func() {
		if r := recover(); r != nil {
			n = &ErrNode{e: ErrName}
		}
	}()
	p := &parser{toks: tokenize(f), book: book, sh: sh}
	n = p.parseCmp()
	return n
}

func (p *parser) parseCmp() Node {
	l := p.parseConcat()
	for {
		t := p.peek()
		if t.k == tOp && (t.s == "=" || t.s == "<>" || t.s == "<" || t.s == ">" || t.s == "<=" || t.s == ">=") {
			p.next()
			r := p.parseConcat()
			l = &BinNode{op: t.s, l: l, r: r}
		} else {
			return l
		}
	}
}

func (p *parser) parseConcat() Node {
	l := p.parseAdd()
	for {
		t := p.peek()
		if t.k == tOp && t.s == "&" {
			p.next()
			r := p.parseAdd()
			l = &BinNode{op: "&", l: l, r: r}
		} else {
			return l
		}
	}
}

func (p *parser) parseAdd() Node {
	l := p.parseMul()
	for {
		t := p.peek()
		if t.k == tOp && (t.s == "+" || t.s == "-") {
			p.next()
			r := p.parseMul()
			l = &BinNode{op: t.s, l: l, r: r}
		} else {
			return l
		}
	}
}

func (p *parser) parseMul() Node {
	l := p.parsePow()
	for {
		t := p.peek()
		if t.k == tOp && (t.s == "*" || t.s == "/") {
			p.next()
			r := p.parsePow()
			l = &BinNode{op: t.s, l: l, r: r}
		} else {
			return l
		}
	}
}

func (p *parser) parsePow() Node {
	l := p.parseUnary()
	for {
		t := p.peek()
		if t.k == tOp && t.s == "^" {
			p.next()
			r := p.parseUnary()
			l = &BinNode{op: "^", l: l, r: r}
		} else {
			return l
		}
	}
}

func (p *parser) parseUnary() Node {
	t := p.peek()
	if t.k == tOp && t.s == "-" {
		p.next()
		x := p.parseUnary()
		return &UnNode{op: "-", x: x}
	}
	if t.k == tOp && t.s == "+" {
		p.next()
		return p.parseUnary()
	}
	return p.parsePostfix()
}

func (p *parser) parsePostfix() Node {
	x := p.parsePrimary()
	for {
		t := p.peek()
		if t.k == tOp && t.s == "%" {
			p.next()
			x = &UnNode{op: "%", x: x}
		} else {
			return x
		}
	}
}

func (p *parser) parsePrimary() Node {
	t := p.next()
	switch t.k {
	case tNum:
		return &NumNode{v: t.n}
	case tStr:
		return &StrNode{s: t.s}
	case tLParen:
		x := p.parseCmp()
		if p.peek().k == tRParen {
			p.next()
		}
		return x
	case tRef:
		return p.makeRef(t.s)
	case tIdent:
		name := t.s
		if strings.HasPrefix(name, "_XLFN.") {
			name = name[6:]
		}
		if p.peek().k == tLParen {
			p.next()
			var args []Node
			if p.peek().k == tRParen {
				p.next()
			} else {
				for {
					if p.peek().k == tComma {
						args = append(args, nil)
						p.next()
						continue
					}
					if p.peek().k == tRParen {
						args = append(args, nil)
						p.next()
						break
					}
					args = append(args, p.parseCmp())
					if p.peek().k == tComma {
						p.next()
						if p.peek().k == tRParen {
							args = append(args, nil)
							p.next()
							break
						}
						continue
					}
					if p.peek().k == tRParen {
						p.next()
					}
					break
				}
			}
			fn, ok := funcs[name]
			if !ok {
				return &ErrNode{e: ErrName}
			}
			return &FuncNode{name: name, fn: fn, args: args}
		}
		switch name {
		case "TRUE":
			return &BoolNode{b: true}
		case "FALSE":
			return &BoolNode{b: false}
		}
		return &ErrNode{e: ErrName}
	}
	panic("bad token")
}

func (p *parser) makeRef(s string) Node {
	sheet := p.sh
	if i := strings.LastIndexByte(s, '!'); i >= 0 {
		name := s[:i]
		idx, ok := p.book.byName[name]
		if !ok {
			return &ErrNode{e: ErrRef}
		}
		sheet = idx
		s = s[i+1:]
	}
	var r Ref
	r.sheet = sheet
	if i := strings.IndexByte(s, ':'); i >= 0 {
		a, b := s[:i], s[i+1:]
		r.r1, r.c1 = parseA1(a)
		r.r2, r.c2 = parseA1(b)
		if r.r1 > r.r2 {
			r.r1, r.r2 = r.r2, r.r1
		}
		if r.c1 > r.c2 {
			r.c1, r.c2 = r.c2, r.c1
		}
	} else {
		r.r1, r.c1 = parseA1(s)
		r.r2, r.c2 = r.r1, r.c1
	}
	if r.r1 < 0 || r.c1 < 0 {
		return &ErrNode{e: ErrRef}
	}
	return &RefNode{ref: r}
}

func parseA1(s string) (int, int) {
	s = strings.ReplaceAll(s, "$", "")
	return parseCellRef(strings.ToUpper(s))
}

var _ = math.Abs
