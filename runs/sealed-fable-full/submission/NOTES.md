# Calc rebuild – notes

Go (standard library only). `build.sh` produces `./calc`.

* `xlsx.go` – minimal XLSX reader (zip + hand-rolled scanning of sheetData, shared strings, inline strings).
* `parse.go` – tokenizer + recursive-descent parser to an AST (Excel precedence, `_xlfn.` prefix stripped).
* `eval.go` – lazy, memoised evaluation of formula cells, LibreOffice-compatible coercions,
  comparison rules (numbers < text, case-insensitive text), 15-significant-digit number→text
  conversion (emulating rtl::math scaling/rounding), error-priority rules (computed errors win
  first in RPN order; references raise errors only when popped, right operand first; IFS/SWITCH
  clear the pending error), dates (serial 1899-12-30 base).
* `funcs.go` – the 87 functions. Kahan–Neumaier summation like LibreOffice's KahanSum, criteria
  parsing with wildcards, LibreOffice quirks (booleans stringify as 1/0, ISLOGICAL on literals
  depends on the previously evaluated function's type, MIN/MAX keep boolean type, etc.).
* `format.go` – TEXT() number/date format codes.
* `index.go` – hash indexes for exact-match lookups (VLOOKUP/HLOOKUP/MATCH/XLOOKUP).
* `main.go` – parallel parse, parallel evaluation of formula cells that only depend on
  non-formula cells, sequential evaluation of the rest, JSON output. GC disabled for speed.

## Verified LibreOffice behaviours (via probes in this sandbox)
* Text→number conversion: numeric-looking strings (incl. " 7 ", "5%", "1,234", ISO/US dates, h:mm) convert;
  others give #VALUE!. Direct string args to SUM/MAX/MEDIAN… give #VALUE!, but INT("3.7") works.
* Numeric `=` uses rtl::math::approxEqual (relative 2^-48); text compare is case-insensitive.
* Error precedence: computed errors are raised immediately (first in RPN order wins); reference errors
  are raised when popped (right operand first, the right operand's conversion happens before the left is
  popped); IFS/SWITCH clear a pending error; PRODUCT overflow yields Inf that only becomes #NUM! on use.
* ISLOGICAL(literal) returns whether the previously executed function/operator was of logical type;
  IF/IFS/SWITCH/CHOOSE/IFERROR jumps reset this.
* MIN keeps the type (boolean/number) of the first value of its first argument; MAX the type of the strict
  winner; VLOOKUP/HLOOKUP return plain numbers for boolean cells, XLOOKUP/INDEX keep the boolean type.
* Empty-string cells in the file are treated as blank cells.
