# Calc formula engine rebuild (Go, standard library only)

`./calc eval --in book.xlsx --out values.json`

* `xlsx.go`   - minimal zip/XML reader (hand-rolled scanner for sheetData, shared/inline strings).
* `parse.go`  - tokenizer + precedence-climbing parser (LibreOffice precedence: unary minus binds
                tighter than `^`, `^` left-assoc, `&` below `+ -`, comparisons lowest).
* `eval.go`   - lazy, memoised cell evaluation (recursive, cycle-guarded), LibreOffice coercions
                (text->number "unambiguous" conversion, bool->"1"/"0" in text context, approxEqual
                comparisons, numbers < text ordering, LibreOffice error-precedence emulation).
* `funcs.go`  - the 87 functions; criteria engine for *IF/*IFS (wildcards, numeric/text rules,
                number cells matched by their display string), lookups with LO binary-search
                semantics, DATEDIF quirks, Kahan (Neumaier) summation as in LibreOffice.
* `format.go` - TEXT() number/date format codes.
* `value.go`  - value model, number->string (shortest round-trip then rounded half-away to 15
                significant digits, `E+0nn` exponent style), date serials (epoch 1899-12-30).
* `index.go`  - per-range hash index for exact-match lookups (VLOOKUP/MATCH/XLOOKUP).
