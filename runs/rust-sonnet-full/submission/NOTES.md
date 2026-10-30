# Calc rebuild (Rust)
- `src/load.rs`: XLSX reader (zip + fast byte scanner for sheet XML).
- `src/parse.rs`: formula tokenizer/recursive-descent parser -> AST.
- `src/eval.rs`: lazy memoised evaluator (recursion on a big-stack thread), LibreOffice semantics
  (bool==number, text->number coercion, criteria syntax, Kahan sums, error-priority emulation via a
  per-formula "global error"), cached rect/index/criteria-bitmask for fast SUMIFS/lookup workloads.
- `src/fmt.rs`: number->text (15 sig digits), TEXT() formats, date helpers.
