package main

import (
	"math"
	"strconv"
	"strings"
)

var monthNames = []string{"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"}
var dayNames = []string{"Saturday", "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday"}

func isDateFormat(f string) bool {
	lf := strings.ToLower(f)
	// strip quoted literals
	for _, c := range lf {
		switch c {
		case 'y', 'd', 'h', 's':
			return true
		}
	}
	if strings.Contains(lf, "m") && !strings.Contains(lf, "0") && !strings.Contains(lf, "#") {
		return true
	}
	return false
}

func formatNumber(x float64, f string) string {
	if isDateFormat(f) {
		return formatDate(x, f)
	}
	return formatNum(x, f)
}

func pad2(n int) string {
	if n < 10 {
		return "0" + strconv.Itoa(n)
	}
	return strconv.Itoa(n)
}

func formatDate(x float64, f string) string {
	serial := int64(math.Floor(x))
	frac := x - math.Floor(x)
	secs := int(math.Round(frac * 86400))
	hh := secs / 3600
	mi := (secs % 3600) / 60
	ss := secs % 60
	y, m, d := civilFromDays(serial)
	dow := int(((serial % 7) + 7) % 7)
	lf := strings.ToLower(f)
	var b strings.Builder
	i := 0
	lastWasHour := false
	for i < len(lf) {
		c := lf[i]
		run := 1
		for i+run < len(lf) && lf[i+run] == c {
			run++
		}
		switch c {
		case 'y':
			if run >= 4 {
				b.WriteString(strconv.Itoa(y))
			} else {
				b.WriteString(pad2(y % 100))
			}
			lastWasHour = false
		case 'm':
			if lastWasHour {
				b.WriteString(pad2(mi))
				lastWasHour = false
			} else {
				switch {
				case run >= 4:
					b.WriteString(monthNames[m-1])
				case run == 3:
					b.WriteString(monthNames[m-1][:3])
				case run == 2:
					b.WriteString(pad2(m))
				default:
					b.WriteString(strconv.Itoa(m))
				}
			}
		case 'd':
			switch {
			case run >= 4:
				b.WriteString(dayNames[dow])
			case run == 3:
				b.WriteString(dayNames[dow][:3])
			case run == 2:
				b.WriteString(pad2(d))
			default:
				b.WriteString(strconv.Itoa(d))
			}
			lastWasHour = false
		case 'h':
			if run >= 2 {
				b.WriteString(pad2(hh))
			} else {
				b.WriteString(strconv.Itoa(hh))
			}
			lastWasHour = true
		case 's':
			if run >= 2 {
				b.WriteString(pad2(ss))
			} else {
				b.WriteString(strconv.Itoa(ss))
			}
			lastWasHour = false
		case '"':
			// literal
			j := strings.IndexByte(f[i+1:], '"')
			if j < 0 {
				j = len(f) - i - 1
			}
			b.WriteString(f[i+1 : i+1+j])
			i += j + 2
			continue
		case '\\':
			if i+1 < len(f) {
				b.WriteByte(f[i+1])
			}
			i += 2
			continue
		default:
			b.WriteString(f[i : i+run])
		}
		i += run
	}
	return b.String()
}

func addThousands(s string) string {
	if len(s) <= 3 {
		return s
	}
	var b strings.Builder
	pre := len(s) % 3
	if pre > 0 {
		b.WriteString(s[:pre])
	}
	for i := pre; i < len(s); i += 3 {
		if b.Len() > 0 {
			b.WriteByte(',')
		}
		b.WriteString(s[i : i+3])
	}
	return b.String()
}

func formatNum(x float64, f string) string {
	// handle sections: positive;negative
	if i := strings.IndexByte(f, ';'); i >= 0 {
		if x < 0 {
			return formatNum(-x, f[i+1:])
		}
		f = f[:i]
	}
	// scan the pattern
	var prefix, suffix strings.Builder
	intDigits := 0 // minimum integer digits ('0')
	decDigits := 0
	hasDot := false
	thousands := false
	percent := false
	sci := false
	expDigits := 0
	expSign := "+"
	inNum := false
	afterNum := false
	i := 0
	for i < len(f) {
		c := f[i]
		switch {
		case c == '"':
			j := strings.IndexByte(f[i+1:], '"')
			if j < 0 {
				j = len(f) - i - 1
			}
			lit := f[i+1 : i+1+j]
			if afterNum {
				suffix.WriteString(lit)
			} else {
				prefix.WriteString(lit)
			}
			i += j + 2
			continue
		case c == '\\':
			if i+1 < len(f) {
				if afterNum {
					suffix.WriteByte(f[i+1])
				} else {
					prefix.WriteByte(f[i+1])
				}
			}
			i += 2
			continue
		case c == '0' || c == '#' || c == '?':
			if sci {
				if c == '0' {
					expDigits++
				}
			} else {
				inNum = true
				if hasDot {
					if c == '0' {
						decDigits++
					} else if c == '#' {
						decDigits++
					}
				} else if c == '0' {
					intDigits++
				}
			}
		case c == '.' && inNum && !hasDot:
			hasDot = true
		case c == ',' && inNum && !hasDot:
			thousands = true
		case (c == 'E' || c == 'e') && inNum && i+1 < len(f) && (f[i+1] == '+' || f[i+1] == '-'):
			sci = true
			expSign = string(f[i+1])
			i += 2
			continue
		case c == '%':
			percent = true
			if inNum {
				afterNum = true
				suffix.WriteByte('%')
			} else {
				prefix.WriteByte('%')
			}
		default:
			if inNum {
				afterNum = true
				suffix.WriteByte(c)
			} else {
				prefix.WriteByte(c)
			}
		}
		i++
	}
	if percent {
		x *= 100
	}
	neg := x < 0
	ax := math.Abs(x)
	var body string
	if sci {
		if ax == 0 {
			body = "0" + dotDec(0, decDigits) + "E" + expSign + strings.Repeat("0", max(expDigits, 1))
		} else {
			exp := int(math.Floor(math.Log10(ax)))
			mant := ax / math.Pow(10, float64(exp))
			mant = roundTo(mant, decDigits)
			if mant >= 10 {
				mant /= 10
				exp++
			}
			ms := strconv.FormatFloat(mant, 'f', decDigits, 64)
			es := strconv.Itoa(absInt(exp))
			for len(es) < expDigits {
				es = "0" + es
			}
			sign := "+"
			if exp < 0 {
				sign = "-"
			} else if expSign == "-" {
				sign = ""
			}
			body = ms + "E" + sign + es
		}
	} else {
		r := roundTo(ax, decDigits)
		s := strconv.FormatFloat(r, 'f', decDigits, 64)
		ip := s
		fp := ""
		if j := strings.IndexByte(s, '.'); j >= 0 {
			ip = s[:j]
			fp = s[j+1:]
		}
		if ip == "0" && intDigits == 0 {
			ip = ""
		}
		for len(ip) < intDigits {
			ip = "0" + ip
		}
		if thousands {
			ip = addThousands(ip)
		}
		body = ip
		if hasDot && decDigits > 0 {
			body += "." + fp
		} else if hasDot && decDigits == 0 {
			body += "."
		}
		if body == "" {
			body = "0"
		}
		if neg && r == 0 {
			neg = false
		}
	}
	res := prefix.String() + body + suffix.String()
	if neg {
		res = "-" + res
	}
	return res
}

func dotDec(x float64, d int) string {
	if d == 0 {
		return ""
	}
	return "." + strings.Repeat("0", d)
}

func absInt(x int) int {
	if x < 0 {
		return -x
	}
	return x
}
