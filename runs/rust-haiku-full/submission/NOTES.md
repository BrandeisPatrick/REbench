# calc: XLSX formula engine in Rust

Usage: `./calc eval --in book.xlsx --out values.json` (build with `./build.sh`, cargo offline,
deps: zip (deflate only), roxmltree).

## Layout
- `src/main.rs`  – XLSX reader: workbook/sheets/rels, shared strings, cells (values and formulas)
  into dense per-sheet grids; formula parse; evaluation of every formula; JSON writer.
  Evaluation runs on a 256 MiB stack thread (deep dependency chains are recursive).
- `src/parse.rs` – tokenizer and precedence parser (sheet-qualified and quoted refs, ranges,
  `_xlfn.` prefixes stripped, postfix %, unary minus binding tighter than `^`).
- `src/eval.rs`  – values, coercion, comparisons, lazy memoized evaluation (formula cells are
  computed on demand, cycle guard, per-formula panic guard -> #VALUE!), function library
  (~90 functions incl. criteria functions with wildcards, lookups, dates, TEXT formats).

## Behaviour matched to LibreOffice 25.2 (learned from public examples and lo-recalc probes)
- Dates: serial n = days since 1899-12-30 for all n (LibreOffice XLSX base), so 1 = 1899-12-31.
- Booleans are numbers for comparison/arithmetic/COUNT/MIN/MAX/AVERAGE over ranges; boolean
  to string gives "1"/"0". ISNUMBER(TRUE) is TRUE. Criteria TRUE match boolean cells.
- Text "TRUE"/"FALSE" coerce to 1/0 in arithmetic and TEXT(); other date-like strings
  (m/d/y, y-m-d, d-Mon-yy) coerce to date serials.
- Arithmetic results are snapped to 15 significant digits (0.1*3 = 0.3); TRUNC/ROUNDDOWN
  use the snapped product, ROUNDUP the raw one.
- Number to text: 15 significant digits from the 17-digit representation (half-even on ties);
  exponent has 3 digits; values >= 1e16 use scientific notation.
- TEXT(): number formats with 0 # , . % and E+00; date formats yyyy yy mmm mmmm dd ddd dddd
  hh mm ss (mm after hh is minutes); negative numbers use the second section if present;
  out-of-range dates give #FMT.
- Domain errors of LN/LOG/LOG10/SQRT are #VALUE! (Err:502); 0^-1 is #NUM!; odd roots of negative
  bases are real (-8)^(1/3) = -2.
- MOD uses floor(x/d) with snapping of tiny residues to 0.
- DATEDIF "yd" aligns the start date to the end's year (previous year if after the end).
- Error precedence in binary operators is operator-dependent (approximation of LibreOffice):
  `/` prefers #VALUE! then the right operand, `+`/comparisons prefer #VALUE! then the left,
  `-`/`*` take the left. CONCAT/CONCATENATE/TEXTJOIN prefer #VALUE! among error arguments.
- SUM/SUMIF(S)/SUMPRODUCT use Kahan (compensated) summation; VAR/STDEV use two-pass plain sums.
- Direct text arguments to SUM-family functions give #VALUE!.
- IF without else returns FALSE; empty references evaluate to 0 at the top level.

## Known gaps
- Array expressions (e.g. SUMPRODUCT((range>2)*range)) are not implemented.
- Shared formulas (`t="shared"` children without text) are not expanded.
- Some error-precedence cases (e.g. nested IFS/CONCAT chains) still differ from LibreOffice.
- Performance: roughly 3-5x faster than LibreOffice on aggregate/lookup/mixed workloads, about
  parity on the 60k-formula chain (parsing and XML overhead dominate there).
