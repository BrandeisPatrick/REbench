# Calc formula engine (Python, stdlib only)

- `build.sh` writes a `calc` shell wrapper that runs `engine.py` with `python3 -I`.
- `engine.py`: XLSX reader (zipfile + ElementTree), tokenizer, recursive-descent parser
  compiling each formula into Python closures, lazy memoized evaluation of formula cells
  (cycle guard), then JSON output for every formula cell.
- LibreOffice semantics encoded: booleans are numbers (1/0) except when they are the final
  result of a formula; text->number coercion (incl. "TRUE"/"FALSE", dates like 1-Jan-06);
  left-operand error precedence for operators; criteria parsing with operators and
  wildcards (boolean cells match text patterns as "TRUE"/"FALSE"); General number-to-text
  with 15 significant digits and 3-digit exponents; TEXT() format subset; ROUND via
  decimal half-up on 15-significant-digit value.
- Performance: range value lists and criteria masks are cached; exact-match lookups use
  hash indexes built once per key range; GC disabled.

## Status (end of session)
- Public examples: 2381/2382 formula cells (99.96%), 39/40 workbooks fully correct.
- Perf (public workloads): aggregate ~5.7x faster than LibreOffice, lookup ~9x faster,
  mixed ~parity (99.8% correct), chain ~8x slower (parse-bound: 60k closures built in Python).
- Known gaps: approximate MATCH/VLOOKUP on unsorted data (LibreOffice uses its own binary
  search order), some error-precedence cases involving references, ISLOGICAL on literals
  inside multi-argument calls, [h] elapsed-time formats, "?" padding in TEXT, last-digit
  differences in VAR/large sums.
