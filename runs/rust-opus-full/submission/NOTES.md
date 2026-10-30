# Calc engine (Rust)
- `src/main.rs`: XLSX load (zip + hand-rolled XML scan), formula parser (precedence-climbing to AST),
  memoized recursive evaluator (big-stack thread for deep chains), ~87 functions, JSON writer.
- `src/fmt.rs`: LibreOffice-style number->text (15 significant digits, half-up on shortest repr) and TEXT() formats.
- LibreOffice semantics reproduced: booleans are numbers (ISNUMBER(TRUE)=TRUE, "1"/"0" in text),
  error precedence rules for binary ops, illegal-argument -> #VALUE!, formula-empty results,
  ISLOGICAL format-type quirk, criteria matching (wildcards, numeric strings, empty operands).
- Perf: SUMIFS/COUNTIFS criteria masks cached as bitsets per (range, criterion); exact lookups use a
  lazily built hash index per range.
- Loader splits large sheet XML at `<row` boundaries and scans/parses chunks in parallel (rayon).
- Each top-level formula evaluation is wrapped in catch_unwind so a bug yields #VALUE! instead of losing the file.
