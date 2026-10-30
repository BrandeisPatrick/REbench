# calc — LibreOffice-compatible formula engine (Rust)

`./calc eval --in book.xlsx --out values.json`

## Structure
- `src/xlsx.rs`  – zip + quick-xml streaming reader: workbook.xml / rels / sharedStrings / sheets.
  Sheets are stored as dense `Vec<Val>` grids plus a parallel `fidx` array pointing at formulas.
- `src/parse.rs` – tokenizer + precedence-climbing parser to an `Expr` AST (Excel precedence:
  unary minus binds tighter than `^`, `%` postfix, `&` below arithmetic, comparisons lowest).
  `_xlfn.` prefixes are stripped; sheet-qualified A1 references and ranges resolved at parse time.
  Parsing runs in parallel with rayon.
- `src/eval.rs`  – value model (`N/S/B/E/Empty`), LibreOffice coercions (text→number incl. `%`,
  thousands separators, `$`, ISO / US / d-mmm-yy dates, TRUE/FALSE), comparison rules
  (numbers < text, case-insensitive text), criteria parsing/matching for *IF functions
  (wildcards, numeric vs text semantics as observed in LO), Kahan/Neumaier summation,
  lazy memoized formula evaluation with cycle guard, cached lookup hash indexes, cached criteria
  bitsets and cached range snapshots.
- `src/funcs.rs` – the 87 functions.
- `src/fmt.rs`   – LO-style number→string (15 significant digits, `E+0xx` beyond 1e16),
  TEXT() number/date format codes, date serial arithmetic (1899-12-30 epoch).

## Observed LibreOffice behaviours reproduced
- Booleans are numbers everywhere (SUM/COUNT/MIN/criteria); `&` converts them to "1"/"0".
- ISNUMBER(bool)=TRUE; IS* functions never propagate errors.
- Empty cell results → 0; `#NUM!` for inf/NaN; LOG/LN/SQRT domain errors → `#VALUE!`.
- Error precedence (verified against 350 LO probes): for binary operators a *computed* error in
  the left operand wins, otherwise the right operand's error; errors that merely come from
  referenced cells (incl. IF/IFS/SWITCH/CHOOSE/INDEX returning a reference) are weaker than any
  computed error. For function arguments the first computed error wins; among reference errors
  the last wins (first for CONCATENATE/CONCAT/TEXTJOIN).
- XOR quirk: `ISLOGICAL("text")` as a non-first XOR argument evaluates TRUE in LO.
- MIN/MAX over ranges whose first value is boolean return a boolean when the extreme is boolean.

## Performance
Evaluation is single-threaded (memoized DAG walk on a 4 GB-stack thread for deep chains);
SUMIFS-style functions use per-(range,criterion) bitsets, exact lookups use hash indexes.
