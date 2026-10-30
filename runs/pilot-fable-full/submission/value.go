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
	KFormula
	KRange
	KArray
)

type Value struct {
	k  Kind
	n  float64
	s  string // string or error code
	rg *Range
	ar *Array
}

type Range struct {
	sh             *Sheet
	r1, c1, r2, c2 int
}

type Array struct {
	rows, cols int
	v          []Value
}

var (
	ErrDiv0  = Value{k: KErr, s: "#DIV/0!"}
	ErrValue = Value{k: KErr, s: "#VALUE!"}
	ErrNA    = Value{k: KErr, s: "#N/A"}
	ErrNum   = Value{k: KErr, s: "#NUM!"}
	ErrRef   = Value{k: KErr, s: "#REF!"}
	ErrName  = Value{k: KErr, s: "#NAME?"}
	ErrNull  = Value{k: KErr, s: "#NULL!"}
)

func Num(f float64) Value  { return Value{k: KNum, n: f} }
func Str(s string) Value   { return Value{k: KStr, s: s} }
func Bool(b bool) Value {
	if b {
		return Value{k: KBool, n: 1}
	}
	return Value{k: KBool, n: 0}
}
func mkErr(code string) Value { return Value{k: KErr, s: code} }

func approxEqual(a, b float64) bool {
	if a == b {
		return true
	}
	d := a - b
	if d < 0 {
		d = -d
	}
	aa := a
	if aa < 0 {
		aa = -aa
	}
	return d < aa*(1.0/(16777216.0*16777216.0))
}

func approxFloor(x float64) float64 {
	f := math.Floor(x)
	if f != x && approxEqual(f+1, x) {
		return f + 1
	}
	return f
}
func approxCeil(x float64) float64 {
	f := math.Ceil(x)
	if f != x && approxEqual(f-1, x) {
		return f - 1
	}
	return f
}

// number -> string, LibreOffice "automatic" format with 15 significant digits
func fmtNum(v float64) string {
	if v == 0 {
		return "0"
	}
	if math.IsInf(v, 0) || math.IsNaN(v) {
		return "#NUM!"
	}
	neg := v < 0
	if neg {
		v = -v
	}
	s := strconv.FormatFloat(v, 'e', -1, 64) // shortest round-trip, then round half away from zero
	ep := strings.IndexByte(s, 'e')
	mant := s[:ep]
	exp, _ := strconv.Atoi(s[ep+1:])
	digits := strings.Replace(mant, ".", "", 1)
	if len(digits) > 15 {
		d := []byte(digits[:15])
		rest := digits[15:]
		up := rest[0] >= '5'
		if up {
			i := 14
			for i >= 0 {
				if d[i] == '9' {
					d[i] = '0'
					i--
				} else {
					d[i]++
					break
				}
			}
			if i < 0 {
				d = append([]byte{'1'}, d...)
				d = d[:15]
				exp++
			}
		}
		digits = string(d)
	}
	digits = strings.TrimRight(digits, "0")
	if digits == "" {
		digits = "0"
	}
	var out string
	if exp <= -15 || exp >= 15 {
		m := digits[:1]
		if len(digits) > 1 {
			m += "." + digits[1:]
		}
		es := "+"
		if exp < 0 {
			es = "-"
			exp = -exp
		}
		ex := strconv.Itoa(exp)
		for len(ex) < 3 {
			ex = "0" + ex
		}
		out = m + "E" + es + ex
	} else if exp < 0 {
		out = "0." + strings.Repeat("0", -exp-1) + digits
	} else {
		if len(digits) <= exp+1 {
			out = digits + strings.Repeat("0", exp+1-len(digits))
		} else {
			out = digits[:exp+1] + "." + digits[exp+1:]
		}
	}
	if neg {
		out = "-" + out
	}
	return out
}

// parse a string as number following LO's unambiguous conversion; ok=false if not numeric
func parseNumStr(s string) (float64, bool) {
	t := strings.TrimSpace(s)
	if t == "" {
		return 0, false
	}
	// plain number
	if f, ok := parsePlain(t); ok {
		return f, true
	}
	up := strings.ToUpper(t)
	if up == "TRUE" {
		return 1, true
	}
	if up == "FALSE" {
		return 0, true
	}
	// percent
	if strings.HasSuffix(t, "%") {
		if f, ok := parsePlain(strings.TrimSpace(t[:len(t)-1])); ok {
			return f / 100, true
		}
	}
	// currency / thousands
	t2 := strings.ReplaceAll(t, ",", "")
	t2 = strings.TrimPrefix(t2, "$")
	if t2 != t {
		if f, ok := parsePlain(t2); ok {
			return f, true
		}
	}
	// d-mmm-yy / dd-mmm-yyyy
	if parts := strings.Split(t, "-"); len(parts) == 3 && len(parts[1]) == 3 {
		d, e1 := strconv.Atoi(parts[0])
		y, e3 := strconv.Atoi(parts[2])
		mi := -1
		for i, mn := range monthNames {
			if strings.EqualFold(mn[:3], parts[1]) {
				mi = i + 1
			}
		}
		if e1 == nil && e3 == nil && mi > 0 && d >= 1 && d <= 31 {
			if len(parts[2]) <= 2 {
				if y < 30 {
					y += 2000
				} else {
					y += 1900
				}
			}
			return float64(daysFromCivil(y, mi, d)), true
		}
	}
	// mm/dd/yyyy
	if parts := strings.Split(t, "/"); len(parts) == 3 && len(parts[2]) == 4 {
		m, e1 := strconv.Atoi(parts[0])
		d, e2 := strconv.Atoi(parts[1])
		y, e3 := strconv.Atoi(parts[2])
		if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
			return float64(daysFromCivil(y, m, d)), true
		}
	}
	// ISO date yyyy-mm-dd [hh:mm[:ss]]
	if len(t) >= 10 && t[4] == '-' && t[7] == '-' {
		y, e1 := strconv.Atoi(t[0:4])
		m, e2 := strconv.Atoi(t[5:7])
		d, e3 := strconv.Atoi(t[8:10])
		if e1 == nil && e2 == nil && e3 == nil && m >= 1 && m <= 12 && d >= 1 && d <= 31 {
			ser := float64(daysFromCivil(y, m, d))
			rest := strings.TrimSpace(t[10:])
			if rest == "" {
				return ser, true
			}
			if tf, ok := parseTime(rest); ok {
				return ser + tf, true
			}
		}
	}
	if tf, ok := parseTime(t); ok {
		return tf, true
	}
	return 0, false
}

func parseTime(t string) (float64, bool) {
	parts := strings.Split(t, ":")
	if len(parts) < 2 || len(parts) > 3 {
		return 0, false
	}
	var vals [3]float64
	for i, p := range parts {
		f, ok := parsePlain(strings.TrimSpace(p))
		if !ok {
			return 0, false
		}
		vals[i] = f
	}
	return vals[0]/24 + vals[1]/1440 + vals[2]/86400, true
}

func parsePlain(t string) (float64, bool) {
	if t == "" {
		return 0, false
	}
	i := 0
	if t[0] == '+' || t[0] == '-' {
		i++
	}
	nd := 0
	for i < len(t) && isDigit(t[i]) {
		i++
		nd++
	}
	if i < len(t) && t[i] == '.' {
		i++
		for i < len(t) && isDigit(t[i]) {
			i++
			nd++
		}
	}
	if nd == 0 {
		return 0, false
	}
	if i < len(t) && (t[i] == 'e' || t[i] == 'E') {
		j := i + 1
		if j < len(t) && (t[j] == '+' || t[j] == '-') {
			j++
		}
		k := j
		for k < len(t) && isDigit(t[k]) {
			k++
		}
		if k == j {
			return 0, false
		}
		i = k
	}
	if i != len(t) {
		return 0, false
	}
	f, err := strconv.ParseFloat(t, 64)
	if err != nil {
		return 0, false
	}
	return f, true
}

// civil date helpers; serial 0 = 1899-12-30
func daysFromCivil(y, m, d int) int {
	if m <= 2 {
		y--
	}
	var era int
	if y >= 0 {
		era = y / 400
	} else {
		era = (y - 399) / 400
	}
	yoe := y - era*400
	mp := (m + 9) % 12
	doy := (153*mp+2)/5 + d - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	days := era*146097 + doe - 719468 // days since 1970-01-01
	return days + 25569
}

func civilFromDays(serial int) (y, m, d int) {
	z := serial - 25569 + 719468
	var era int
	if z >= 0 {
		era = z / 146097
	} else {
		era = (z - 146096) / 146097
	}
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	y = yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	d = doy - (153*mp+2)/5 + 1
	if mp < 10 {
		m = mp + 3
	} else {
		m = mp - 9
	}
	if m <= 2 {
		y++
	}
	return
}

func daysInMonth(y, m int) int {
	switch m {
	case 1, 3, 5, 7, 8, 10, 12:
		return 31
	case 4, 6, 9, 11:
		return 30
	}
	if (y%4 == 0 && y%100 != 0) || y%400 == 0 {
		return 29
	}
	return 28
}
