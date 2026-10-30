package main

import (
	"math"
	"strconv"
	"strings"
)

var monthNames = []string{"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"}
var dayNames = []string{"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"}

func isDateFormat(f string) bool {
	inQ := false
	hasM := false
	for i := 0; i < len(f); i++ {
		c := f[i]
		if c == '"' {
			inQ = !inQ
			continue
		}
		if inQ {
			continue
		}
		switch c {
		case 'y', 'Y', 'd', 'D', 'h', 'H', 's', 'S':
			return true
		case 'm', 'M':
			hasM = true
		case '0', '#', '?':
			return false
		}
	}
	return hasM
}

func formatText(v float64, f string) string {
	if f == "" || f == "@" || strings.EqualFold(f, "General") {
		return fmtNum(v)
	}
	sections := splitSections(f)
	sec := sections[0]
	if v < 0 && len(sections) >= 2 {
		sec = sections[1]
		v = -v
	} else if v == 0 && len(sections) >= 3 {
		sec = sections[2]
	}
	if isDateFormat(sec) {
		return formatDate(v, sec)
	}
	return formatNumber(v, sec)
}

func splitSections(f string) []string {
	var out []string
	inQ := false
	st := 0
	for i := 0; i < len(f); i++ {
		if f[i] == '"' {
			inQ = !inQ
		} else if f[i] == ';' && !inQ {
			out = append(out, f[st:i])
			st = i + 1
		}
	}
	out = append(out, f[st:])
	return out
}

func pad2(n int) string {
	if n < 10 {
		return "0" + strconv.Itoa(n)
	}
	return strconv.Itoa(n)
}

func formatDate(v float64, f string) string {
	ser := math.Floor(v)
	frac := v - ser
	secs := int(math.Floor(frac*86400 + 0.5))
	if secs >= 86400 {
		secs = 0
		ser++
	}
	y, m, d := civilFromDays(int(ser))
	hh := secs / 3600
	mi := (secs % 3600) / 60
	ss := secs % 60
	dow := ((int(ser)+6)%7 + 7) % 7
	hasAMPM := strings.Contains(strings.ToUpper(f), "AM/PM")
	var b strings.Builder
	lf := strings.ToLower(f)
	i := 0
	lastWasHour := false
	for i < len(lf) {
		c := lf[i]
		if c == '"' {
			j := strings.IndexByte(f[i+1:], '"')
			if j < 0 {
				b.WriteString(f[i+1:])
				break
			}
			b.WriteString(f[i+1 : i+1+j])
			i += j + 2
			continue
		}
		if c == '\\' && i+1 < len(f) {
			b.WriteByte(f[i+1])
			i += 2
			continue
		}
		if strings.HasPrefix(lf[i:], "am/pm") {
			if hh >= 12 {
				b.WriteString("PM")
			} else {
				b.WriteString("AM")
			}
			i += 5
			continue
		}
		run := 1
		for i+run < len(lf) && lf[i+run] == c {
			run++
		}
		switch c {
		case 'y':
			if run <= 2 {
				b.WriteString(pad2(y % 100))
			} else {
				b.WriteString(strconv.Itoa(y))
			}
			lastWasHour = false
		case 'm':
			// minute if preceded by h or followed by s
			isMin := lastWasHour
			if !isMin {
				j := i + run
				for j < len(lf) && (lf[j] == ':' || lf[j] == ' ') {
					j++
				}
				if j < len(lf) && lf[j] == 's' {
					isMin = true
				}
			}
			if isMin {
				if run == 1 {
					b.WriteString(strconv.Itoa(mi))
				} else {
					b.WriteString(pad2(mi))
				}
			} else {
				switch {
				case run == 1:
					b.WriteString(strconv.Itoa(m))
				case run == 2:
					b.WriteString(pad2(m))
				case run == 3:
					b.WriteString(monthNames[m-1][:3])
				case run == 4:
					b.WriteString(monthNames[m-1])
				default:
					b.WriteString(monthNames[m-1][:1])
				}
			}
			lastWasHour = false
		case 'd':
			switch {
			case run == 1:
				b.WriteString(strconv.Itoa(d))
			case run == 2:
				b.WriteString(pad2(d))
			case run == 3:
				b.WriteString(dayNames[dow][:3])
			default:
				b.WriteString(dayNames[dow])
			}
			lastWasHour = false
		case 'h':
			h := hh
			if hasAMPM {
				h = hh % 12
				if h == 0 {
					h = 12
				}
			}
			if run == 1 {
				b.WriteString(strconv.Itoa(h))
			} else {
				b.WriteString(pad2(h))
			}
			lastWasHour = true
		case 's':
			if run == 1 {
				b.WriteString(strconv.Itoa(ss))
			} else {
				b.WriteString(pad2(ss))
			}
			lastWasHour = false
		default:
			b.WriteString(f[i : i+run])
			if c != ':' && c != ' ' {
				lastWasHour = false
			}
		}
		i += run
	}
	return b.String()
}

func formatNumber(v float64, f string) string {
	// analyse pattern
	intDigits := 0 // count of 0 in integer part
	intPlaceholders := 0
	decDigits := 0
	decMin := 0
	thousands := false
	percent := false
	expo := false
	expDigits := 0
	expSign := "+"
	inQ := false
	seenDot := false
	seenE := false
	for i := 0; i < len(f); i++ {
		c := f[i]
		if c == '"' {
			inQ = !inQ
			continue
		}
		if inQ {
			continue
		}
		switch c {
		case '0', '#', '?':
			if seenE {
				expDigits++
			} else if seenDot {
				decDigits++
				if c == '0' {
					decMin++
				}
			} else {
				intPlaceholders++
				if c == '0' {
					intDigits++
				}
			}
		case '.':
			if !seenE {
				seenDot = true
			}
		case ',':
			if !seenDot && !seenE {
				thousands = true
			}
		case '%':
			percent = true
		case 'E', 'e':
			if i+1 < len(f) && (f[i+1] == '+' || f[i+1] == '-') {
				expo = true
				seenE = true
				expSign = string(f[i+1])
				i++
			}
		}
	}
	if percent {
		v *= 100
	}
	neg := v < 0
	av := math.Abs(v)
	var intStr, decStr, expStr string
	if expo {
		e := 0
		if av != 0 {
			e = int(math.Floor(math.Log10(av)))
		}
		mant := av / math.Pow(10, float64(e))
		// round mantissa to decDigits
		ms := strconv.FormatFloat(mant, 'f', decDigits, 64)
		if strings.HasPrefix(ms, "10") {
			e++
			mant = av / math.Pow(10, float64(e))
			ms = strconv.FormatFloat(mant, 'f', decDigits, 64)
		}
		parts := strings.SplitN(ms, ".", 2)
		intStr = parts[0]
		if len(parts) > 1 {
			decStr = parts[1]
		}
		es := strconv.Itoa(absInt(e))
		for len(es) < expDigits {
			es = "0" + es
		}
		if e < 0 {
			expStr = "E-" + es
		} else if expSign == "+" {
			expStr = "E+" + es
		} else {
			expStr = "E" + es
		}
	} else {
		// round half away from zero to decDigits
		p := math.Pow(10, float64(decDigits))
		r := approxFloor(av*p+0.5) / p
		if r == 0 {
			neg = false
		}
		s := strconv.FormatFloat(r, 'f', decDigits, 64)
		if av >= 1e15 {
			es := strconv.FormatFloat(av, 'e', 14, 64)
			epos := strings.IndexByte(es, 'e')
			dg := strings.Replace(es[:epos], ".", "", 1)
			ex, _ := strconv.Atoi(es[epos+1:])
			if ex+1 >= len(dg) {
				s = dg + strings.Repeat("0", ex+1-len(dg))
				if decDigits > 0 {
					s += "." + strings.Repeat("0", decDigits)
				}
			}
		}
		parts := strings.SplitN(s, ".", 2)
		intStr = parts[0]
		if len(parts) > 1 {
			decStr = parts[1]
		}
		if intStr == "0" && intDigits == 0 {
			intStr = ""
		}
	}
	// trim optional decimals
	for len(decStr) > decMin && strings.HasSuffix(decStr, "0") {
		decStr = decStr[:len(decStr)-1]
	}
	for len(intStr) < intDigits {
		intStr = "0" + intStr
	}
	if thousands && len(intStr) > 3 {
		var b strings.Builder
		for i, c := range intStr {
			if i > 0 && (len(intStr)-i)%3 == 0 {
				b.WriteByte(',')
			}
			b.WriteRune(c)
		}
		intStr = b.String()
	}
	// now emit: walk the format, replacing the numeric block with the formatted number
	var b strings.Builder
	if neg && !(intStr == "" && (decStr == "" || strings.Trim(decStr, "0") == "")) {
		b.WriteByte('-')
	} else if neg && expo {
		b.WriteByte('-')
	}
	inQ = false
	numDone := false
	i := 0
	for i < len(f) {
		c := f[i]
		if c == '"' {
			j := strings.IndexByte(f[i+1:], '"')
			if j < 0 {
				b.WriteString(f[i+1:])
				break
			}
			b.WriteString(f[i+1 : i+1+j])
			i += j + 2
			continue
		}
		if c == '\\' && i+1 < len(f) {
			b.WriteByte(f[i+1])
			i += 2
			continue
		}
		if c == '0' || c == '#' || c == '?' || c == '.' || c == ',' || c == 'E' || c == 'e' {
			if !numDone {
				b.WriteString(intStr)
				if decDigits > 0 && decStr != "" {
					b.WriteByte('.')
					b.WriteString(decStr)
				}
				b.WriteString(expStr)
				numDone = true
			}
			// skip the whole numeric block
			for i < len(f) && (f[i] == '0' || f[i] == '#' || f[i] == '?' || f[i] == '.' || f[i] == ',' || f[i] == 'E' || f[i] == 'e' || f[i] == '+' || f[i] == '-') {
				i++
			}
			continue
		}
		if c == '%' {
			b.WriteByte('%')
			i++
			continue
		}
		if c == '_' && i+1 < len(f) {
			b.WriteByte(' ')
			i += 2
			continue
		}
		if c == '*' && i+1 < len(f) {
			i += 2
			continue
		}
		b.WriteByte(c)
		i++
	}
	return b.String()
}

func absInt(x int) int {
	if x < 0 {
		return -x
	}
	return x
}
