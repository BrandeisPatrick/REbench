package main

import (
	"math"
	"strconv"
	"strings"
)

var monthNames = []string{"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"}
var dayNames = []string{"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"}

func fText(cx *Ctx, a []*Node) Val {
	if len(a) < 2 {
		return mkErr(eVal)
	}
	v := cx.eval(a[0])
	if v.k == KErr {
		return v
	}
	f, e := cx.scalarStr(a[1])
	if e.k == KErr {
		return e
	}
	if v.k == KStr {
		if nv, e := toNum(v); e.k != KErr && strings.TrimSpace(v.s) != "" {
			v = mkNum(nv)
		}
	}
	switch v.k {
	case KStr:
		// text with a number format: only '@' sections apply
		secs := splitSections(f)
		sec := secs[0]
		if len(secs) >= 4 {
			sec = secs[3]
		}
		if strings.Contains(sec, "@") {
			return mkStr(strings.ReplaceAll(stripQuotes(sec), "@", v.s))
		}
		return v
	case KBool:
		if f == "" {
			return mkStr("")
		}
		return mkStr(formatNumber(v.n, f))
	}
	return mkStr(formatNumber(v.n, f))
}

func stripQuotes(s string) string {
	var sb strings.Builder
	in := false
	for i := 0; i < len(s); i++ {
		if s[i] == '"' {
			in = !in
			continue
		}
		if s[i] == '\\' && i+1 < len(s) {
			i++
		}
		sb.WriteByte(s[i])
	}
	return sb.String()
}

func splitSections(f string) []string {
	var out []string
	var cur strings.Builder
	in := false
	for i := 0; i < len(f); i++ {
		c := f[i]
		if c == '"' {
			in = !in
		}
		if c == '\\' && i+1 < len(f) {
			cur.WriteByte(c)
			i++
			cur.WriteByte(f[i])
			continue
		}
		if c == ';' && !in {
			out = append(out, cur.String())
			cur.Reset()
			continue
		}
		cur.WriteByte(c)
	}
	out = append(out, cur.String())
	return out
}

func isDateFmt(f string) bool {
	in := false
	for i := 0; i < len(f); i++ {
		c := f[i]
		if c == '"' {
			in = !in
			continue
		}
		if in {
			continue
		}
		if c == '\\' || c == '_' || c == '*' {
			i++
			continue
		}
		if c == '[' {
			for i < len(f) && f[i] != ']' {
				i++
			}
			continue
		}
		switch c {
		case 'y', 'Y', 'd', 'D', 'h', 'H', 's', 'S', 'm', 'M':
			return true
		case 'a', 'A':
			if len(f) >= i+3 && strings.EqualFold(f[i:i+3], "a/p") || len(f) >= i+5 && strings.EqualFold(f[i:i+5], "am/pm") {
				return true
			}
		}
	}
	return false
}

func formatNumber(x float64, f string) string {
	secs := splitSections(f)
	sec := secs[0]
	neg := x < 0
	ax := math.Abs(x)
	auto := true
	if len(secs) >= 2 {
		if neg {
			sec = secs[1]
			auto = false
		}
	}
	if len(secs) >= 3 && x == 0 {
		sec = secs[2]
	}
	if strings.TrimSpace(sec) == "" && len(secs) > 1 {
		return ""
	}
	if strings.EqualFold(strings.TrimSpace(sec), "general") || sec == "" {
		if sec == "" && f != "" && len(secs) == 1 {
			return ""
		}
		return fmtNum(x)
	}
	if strings.Contains(stripQuotesKeepAt(sec), "@") {
		return strings.ReplaceAll(stripQuotes(sec), "@", fmtNum(x))
	}
	if isDateFmt(sec) {
		return formatDate(x, sec)
	}
	out := formatNumSection(ax, sec)
	if neg && auto && out != "" {
		// negative zero result check
		allZero := true
		for _, c := range out {
			if c >= '1' && c <= '9' {
				allZero = false
			}
		}
		if !allZero {
			out = "-" + out
		}
	}
	return out
}

type fmtTok struct {
	kind byte // 'd' digit placeholder (0,#,?), 'l' literal, '.' decimal, ',' comma, '%' , 'e' exponent
	ch   byte
	lit  string
}

func tokenizeNum(sec string) []fmtTok {
	var toks []fmtTok
	for i := 0; i < len(sec); i++ {
		c := sec[i]
		switch c {
		case '"':
			j := i + 1
			for j < len(sec) && sec[j] != '"' {
				j++
			}
			toks = append(toks, fmtTok{kind: 'l', lit: sec[i+1 : j]})
			i = j
		case '\\':
			if i+1 < len(sec) {
				i++
				toks = append(toks, fmtTok{kind: 'l', lit: string(sec[i])})
			}
		case '_':
			if i+1 < len(sec) {
				i++
				toks = append(toks, fmtTok{kind: 'l', lit: " "})
			}
		case '*':
			if i+1 < len(sec) {
				i++
			}
		case '[':
			j := i
			for j < len(sec) && sec[j] != ']' {
				j++
			}
			i = j
		case '0', '#', '?':
			toks = append(toks, fmtTok{kind: 'd', ch: c})
		case '.':
			toks = append(toks, fmtTok{kind: '.'})
		case ',':
			toks = append(toks, fmtTok{kind: ','})
		case '%':
			toks = append(toks, fmtTok{kind: '%'})
		case 'E', 'e':
			if i+1 < len(sec) && (sec[i+1] == '+' || sec[i+1] == '-') {
				j := i + 2
				for j < len(sec) && sec[j] == '0' {
					j++
				}
				toks = append(toks, fmtTok{kind: 'e', ch: sec[i+1], lit: strconv.Itoa(j - i - 2)})
				i = j - 1
			} else {
				toks = append(toks, fmtTok{kind: 'l', lit: string(c)})
			}
		default:
			toks = append(toks, fmtTok{kind: 'l', lit: string(c)})
		}
	}
	return toks
}

// decimal digits of ax rounded to nd decimals (half-up on shortest repr)
func decDigits(ax float64, nd int) (string, string) {
	s := strconv.FormatFloat(ax, 'f', -1, 64)
	ip, fp := s, ""
	if i := strings.IndexByte(s, '.'); i >= 0 {
		ip, fp = s[:i], s[i+1:]
	}
	if len(fp) > nd {
		round := fp[nd] >= '5'
		fp = fp[:nd]
		if round {
			b := []byte(ip + fp)
			i := len(b) - 1
			for i >= 0 {
				if b[i] == '9' {
					b[i] = '0'
					i--
				} else {
					b[i]++
					break
				}
			}
			if i < 0 {
				b = append([]byte{'1'}, b...)
			}
			ip = string(b[:len(b)-nd])
			fp = string(b[len(b)-nd:])
		}
	} else {
		fp += strings.Repeat("0", nd-len(fp))
	}
	return ip, fp
}

func formatNumSection(ax float64, sec string) string {
	toks := tokenizeNum(sec)
	hasPct := 0
	hasExp := false
	dotSeen := false
	nInt, nDec := 0, 0
	for _, t := range toks {
		switch t.kind {
		case '%':
			hasPct++
		case 'e':
			hasExp = true
		case '.':
			dotSeen = true
		case 'd':
			if dotSeen {
				nDec++
			} else {
				nInt++
			}
		}
	}
	for i := 0; i < hasPct; i++ {
		ax *= 100
	}
	// thousands: comma between digit placeholders; trailing commas scale
	thousands := false
	scale := 0
	lastD := -1
	firstD := -1
	for i, t := range toks {
		if t.kind == 'd' {
			if firstD < 0 {
				firstD = i
			}
			lastD = i
		}
	}
	for i, t := range toks {
		if t.kind == ',' && firstD >= 0 {
			if i > firstD && i < lastD {
				thousands = true
			} else if i > lastD && !dotSeen {
				scale++
			}
		}
	}
	for i := 0; i < scale; i++ {
		ax /= 1000
	}
	var ipart, fpart string
	expStr := ""
	if hasExp {
		var et fmtTok
		for _, t := range toks {
			if t.kind == 'e' {
				et = t
			}
		}
		ed, _ := strconv.Atoi(et.lit)
		e10 := 0
		mant := ax
		if ax != 0 {
			e10 = int(math.Floor(math.Log10(ax)))
			// integer digits count in mantissa
			if nInt > 1 {
				e10 -= nInt - 1
			}
			mant = ax / math.Pow(10, float64(e10))
			// rounding may bump mantissa
			ip, fp := decDigits(mant, nDec)
			if len(ip) > maxInt(nInt, 1) {
				e10++
				mant = ax / math.Pow(10, float64(e10))
			}
			_ = fp
		}
		ipart, fpart = decDigits(mant, nDec)
		sign := ""
		if e10 < 0 {
			sign = "-"
		} else if et.ch == '+' {
			sign = "+"
		}
		es := strconv.Itoa(absInt(e10))
		for len(es) < ed {
			es = "0" + es
		}
		expStr = "E" + sign + es
	} else {
		ipart, fpart = decDigits(ax, nDec)
	}
	// strip leading zeros of ipart
	ipart = strings.TrimLeft(ipart, "0")
	// count mandatory int zeros
	reqInt := 0
	for _, t := range toks {
		if t.kind == '.' {
			break
		}
		if t.kind == 'd' && (t.ch == '0') {
			reqInt++
		}
	}
	for len(ipart) < reqInt {
		ipart = "0" + ipart
	}
	// trim optional decimals
	reqDec := 0
	di := 0
	dotSeen = false
	for _, t := range toks {
		if t.kind == '.' {
			dotSeen = true
			continue
		}
		if dotSeen && t.kind == 'd' {
			di++
			if t.ch == '0' {
				reqDec = di
			}
		}
	}
	for len(fpart) > reqDec && strings.HasSuffix(fpart, "0") {
		fpart = fpart[:len(fpart)-1]
	}
	// '?' placeholders pad with spaces (rare) - ignore
	if thousands {
		var sb strings.Builder
		for i := 0; i < len(ipart); i++ {
			if i > 0 && (len(ipart)-i)%3 == 0 {
				sb.WriteByte(',')
			}
			sb.WriteByte(ipart[i])
		}
		ipart = sb.String()
	}
	// assemble
	var out strings.Builder
	intDone := false
	dotSeen = false
	decUsed := false
	for _, t := range toks {
		switch t.kind {
		case 'l':
			out.WriteString(t.lit)
		case '%':
			out.WriteByte('%')
		case ',':
		case 'd':
			if !dotSeen {
				if !intDone {
					out.WriteString(ipart)
					intDone = true
				}
			} else if !decUsed {
				out.WriteString(fpart)
				decUsed = true
			}
		case '.':
			dotSeen = true
			if !intDone {
				out.WriteString(ipart)
				intDone = true
			}
			out.WriteByte('.')
		case 'e':
			out.WriteString(expStr)
		}
	}
	if nInt == 0 && nDec == 0 && !hasExp {
		// no placeholders: literal only
	}
	return out.String()
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}
func absInt(a int) int {
	if a < 0 {
		return -a
	}
	return a
}

func formatDate(x float64, sec string) string {
	serial := int(math.Floor(x))
	frac := x - float64(serial)
	totalSec := int(math.Round(frac * 86400))
	if totalSec >= 86400 {
		totalSec -= 86400
		serial++
	}
	y, m, d := serialCivil(serial)
	wd := weekday(serial, 1) - 1
	hh, mi, ss := totalSec/3600, (totalSec/60)%60, totalSec%60
	ampm := false
	lower := strings.ToLower(sec)
	if strings.Contains(lower, "am/pm") || strings.Contains(lower, "a/p") {
		ampm = true
	}
	var out strings.Builder
	n := len(sec)
	for i := 0; i < n; i++ {
		c := sec[i]
		lc := c | 0x20
		switch {
		case c == '"':
			j := i + 1
			for j < n && sec[j] != '"' {
				j++
			}
			out.WriteString(sec[i+1 : j])
			i = j
		case c == '\\':
			if i+1 < n {
				i++
				out.WriteByte(sec[i])
			}
		case c == '_':
			i++
			out.WriteByte(' ')
		case c == '*':
			i++
		case c == '[':
			for i < n && sec[i] != ']' {
				i++
			}
		case lc == 'y':
			j := i
			for j < n && (sec[j]|0x20) == 'y' {
				j++
			}
			if j-i <= 2 {
				out.WriteString(pad2(y % 100))
			} else {
				out.WriteString(strconv.Itoa(y))
			}
			i = j - 1
		case lc == 'm':
			j := i
			for j < n && (sec[j]|0x20) == 'm' {
				j++
			}
			cnt := j - i
			// minutes if after h or before s
			isMin := false
			k := i - 1
			for k >= 0 && (sec[k] == ' ' || sec[k] == ':' || sec[k] == '"') {
				k--
			}
			if k >= 0 && (sec[k]|0x20) == 'h' {
				isMin = true
			}
			k = j
			for k < n && (sec[k] == ' ' || sec[k] == ':' || sec[k] == '"') {
				k++
			}
			if k < n && (sec[k]|0x20) == 's' {
				isMin = true
			}
			if isMin {
				if cnt == 1 {
					out.WriteString(strconv.Itoa(mi))
				} else {
					out.WriteString(pad2(mi))
				}
			} else {
				switch cnt {
				case 1:
					out.WriteString(strconv.Itoa(m))
				case 2:
					out.WriteString(pad2(m))
				case 3:
					out.WriteString(monthNames[m-1][:3])
				case 4:
					out.WriteString(monthNames[m-1])
				default:
					out.WriteString(monthNames[m-1][:1])
				}
			}
			i = j - 1
		case lc == 'd':
			j := i
			for j < n && (sec[j]|0x20) == 'd' {
				j++
			}
			switch j - i {
			case 1:
				out.WriteString(strconv.Itoa(d))
			case 2:
				out.WriteString(pad2(d))
			case 3:
				out.WriteString(dayNames[wd][:3])
			default:
				out.WriteString(dayNames[wd])
			}
			i = j - 1
		case lc == 'h':
			j := i
			for j < n && (sec[j]|0x20) == 'h' {
				j++
			}
			h := hh
			if ampm {
				h = hh % 12
				if h == 0 {
					h = 12
				}
			}
			if j-i == 1 {
				out.WriteString(strconv.Itoa(h))
			} else {
				out.WriteString(pad2(h))
			}
			i = j - 1
		case lc == 's':
			j := i
			for j < n && (sec[j]|0x20) == 's' {
				j++
			}
			if j-i == 1 {
				out.WriteString(strconv.Itoa(ss))
			} else {
				out.WriteString(pad2(ss))
			}
			i = j - 1
		case lc == 'a':
			if i+4 < n+0 && strings.EqualFold(sec[i:minInt(i+5, n)], "am/pm") {
				if hh < 12 {
					out.WriteString("AM")
				} else {
					out.WriteString("PM")
				}
				i += 4
			} else if i+2 < n+0 && strings.EqualFold(sec[i:minInt(i+3, n)], "a/p") {
				if hh < 12 {
					out.WriteString("A")
				} else {
					out.WriteString("P")
				}
				i += 2
			} else {
				out.WriteByte(c)
			}
		default:
			out.WriteByte(c)
		}
	}
	return out.String()
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func pad2(n int) string {
	if n < 10 {
		return "0" + strconv.Itoa(n)
	}
	return strconv.Itoa(n)
}

func stripQuotesKeepAt(s string) string {
	var sb strings.Builder
	in := false
	for i := 0; i < len(s); i++ {
		if s[i] == '"' {
			in = !in
			continue
		}
		if !in {
			sb.WriteByte(s[i])
		}
	}
	return sb.String()
}
