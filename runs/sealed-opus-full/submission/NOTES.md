# Calc formula engine (Rust)
- `src/io.rs`: minimal zip reader (flate2) + hand-rolled XLSX sheet/sharedStrings parser.
- `src/parse.rs`: formula tokenizer/recursive-descent parser to AST.
- `src/main.rs`: dense per-sheet grids, lazy recursive evaluation with memoization (big-stack thread), JSON output.
- `src/funcs.rs`: the 87 functions, criteria matching (cached bitsets for *IFS), hashed exact lookups.
- `src/fmt.rs`: number->text (LibreOffice style), TEXT() formats, date serial helpers.
LibreOffice quirks emulated: boolean cells behave as numbers, Kahan summation, error precedence order, ISLOGICAL format quirk.

Performance: mimalloc, zlib-rs inflate, sheet XML and formula parsing split across 4 threads,
memoized evaluation, *IFS criteria evaluated once per (range, criterion) into cached bitsets,
exact lookups via per-range hash maps, process exits without freeing.
