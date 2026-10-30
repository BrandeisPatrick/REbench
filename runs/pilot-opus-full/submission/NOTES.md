# Calc formula engine rebuild

Single-file C++17 engine (`src/main.cpp`), built by `build.sh` (g++ -O2; links libzip, pugixml, pthread).

## Pipeline
- XLSX: libzip; worksheets scanned by a hand-written XML scanner split across 4 threads (pugixml fallback),
  formulas parsed into ASTs inside the scanner threads (thread-local arena allocation).
- Evaluation: recursive with memoization on a large-stack thread, row-major order.
- Output JSON written in parallel chunks.

## LibreOffice semantics emulated (found by probing lo-recalc)
- Booleans are numbers with a "logical" format flag; output as bool only if the value is 0/1.
  Format propagates through refs, IF/CHOOSE/IFS/SWITCH, MIN/MAX (first value), unary minus, VALUE.
- Text->number: trims spaces, accepts %, thousands separators, $, ISO / m/d/y / d-mmm-yy / "Month yyyy"
  dates, hh:mm times, TRUE/FALSE. Direct string args to SUM/PRODUCT/etc. give #VALUE!.
- Error precedence: binary ops prefer the left error unless it came from a cell reference; errors produced by
  IFERROR/IFNA/IFS/SWITCH/XLOOKUP are "strong" and win; function args: first error wins, with the
  segment rule around strong-function args.
- Neumaier (Kahan) sums, approxAdd/approxSub, 15-significant-digit number->text using shortest repr + half-up.
- *IF(S) criteria with operators, wildcards, numeric-string criteria, empty criteria; implicit intersection
  (also cross-sheet); approximate lookups scan linearly and stop on exact match or order break.
- TEXT() number/date format codes, DATEDIF units, EDATE/EOMONTH truncation, LN/SQRT/LOG errors = #VALUE!.

## Speed
- Criteria match masks cached per (range, criterion) for SUMIFS/COUNTIFS/AVERAGEIFS etc.
- Hash index for exact numeric VLOOKUP/MATCH/XLOOKUP over large ranges.
- Integer function ids, arg cache to evaluate each argument once.
