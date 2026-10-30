# Calc rebuild (Rust)
- `src/main.rs`: xlsx (zip + hand-rolled XML scanner) loading, JSON output. Formula parsing runs on 4 threads.
- `src/parse.rs`: recursive-descent formula parser -> AST.
- `src/eval.rs`: lazy memoized evaluation (recursion on a 3 GB stack thread handles 100k-deep chains), operators, coercions, LO error-priority emulation.
- `src/funcs.rs`: all 87 functions; criteria matching (COUNTIF family) with per-(range,criterion) bitset cache; Kahan summation like LO.
- `src/fmt.rs`: number->text (15 significant digits, LO style exponent), TEXT() formats, date helpers.
Behaviour was matched empirically against LibreOffice with probe workbooks and shuffled-data fuzzing of the public books.
