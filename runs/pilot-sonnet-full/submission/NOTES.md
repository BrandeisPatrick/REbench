# Calc formula engine rebuild (Go, stdlib only)

`./calc eval --in book.xlsx --out values.json`

* `main.go`   - XLSX loading (hand-rolled zip/XML scanner, parallel per sheet and per chunk), JSON writer.
* `parse.go`  - formula tokenizer / precedence-climbing parser to a small AST.
* `eval.go`   - lazy memoised cell evaluation, coercions, operators, number-to-text (LibreOffice General rules).
* `funcs.go`, `funcs2.go` - the 87 functions (math, text, logic, stats, criteria functions, lookups, dates).
* `format.go` - TEXT() number/date format engine.
* `fast.go`   - bitmap cache for SUMIFS/COUNTIFS-style criteria and hash index for exact lookups.

LibreOffice behaviours reproduced (found by probing the reference with lo-recalc):
booleans are numbers with a format flag (ISNUMBER(TRUE)=TRUE, "1"/"0" in string conversion, MIN/MAX
keep the flag), text->number conversion accepts "TRUE", thousands separators, dates/times, direct text
args to SUM/MAX/... give #VALUE!, NaN/Inf -> #NUM!, LOG(0)/SQRT(-1) -> #VALUE!, number->text uses
15 significant digits with LO's scaling/rounding and E+nnn exponents, error precedence emulation
(first raised error wins, lookup #N/A tokens are "latent"), DATE year limits, DATEDIF quirks, etc.
