# Calc rebuild: contract

You are rebuilding the formula engine of a spreadsheet application from scratch. The reference
application is **LibreOffice Calc 25.2**, installed in this sandbox. Your program reads an `.xlsx`
workbook, computes every formula, and writes the results. It must match LibreOffice's results
exactly and run as fast as possible.

## The command your submission must provide

```
./calc eval --in book.xlsx --out values.json
```

Write one entry for **every cell that contains a formula** (cells with an `<f>` element), on every
sheet. Formula cells in the input files carry **no cached values**, so the answers are not in the
file. Extra entries are ignored.

```json
{ "Calc": { "E1": {"t": "n", "v": 3.5}, "E2": {"t": "s", "v": "abc"},
            "E3": {"t": "b", "v": true}, "E4": {"t": "e", "v": "#DIV/0!"} } }
```

| `t` | meaning | `v` |
|---|---|---|
| `n` | number | finite JSON number; compared with relative tolerance 1e-9, absolute 1e-12 |
| `s` | text | exact string |
| `b` | boolean | `true` / `false` |
| `e` | error | exactly one of `#DIV/0!` `#VALUE!` `#N/A` `#NUM!` `#REF!` `#NAME?` `#NULL!` |

A formula whose result is a reference to an empty cell evaluates to the number 0, as in LibreOffice.
Error values are the ones LibreOffice writes when it saves XLSX. Its GUI shows some errors as internal
codes such as `Err:502`; those are saved as one of the standard codes above, so check with `lo-recalc`.
Where Excel and LibreOffice differ, **LibreOffice 25.2 is the reference**.

## Scope

Workbooks have up to three sheets (`Calc`, `Data`, `Lookup`), plain values (numbers, text,
booleans, blanks), and formulas using:

- **Operators:** `+ - * / ^ & = <> < > <= >=`, unary minus, postfix `%`, parentheses.
- **References:** A1 cells and ranges, absolute and relative (`$A$2:$H$61`), cross-sheet (`Data!E2:E40`).
- **Functions (87):** ABS AND AVERAGE AVERAGEIF AVERAGEIFS CHOOSE CONCAT CONCATENATE COUNT COUNTA
  COUNTBLANK COUNTIF COUNTIFS DATE DATEDIF DAY DAYS EDATE EOMONTH EXACT EXP FIND HLOOKUP IF IFERROR
  IFNA IFS INDEX INT ISBLANK ISERROR ISLOGICAL ISNA ISNUMBER ISTEXT LARGE LEFT LEN LN LOG LOG10 LOWER
  MATCH MAX MEDIAN MID MIN MOD MONTH N NOT OR PI POWER PRODUCT PROPER REPLACE REPT RIGHT ROUND
  ROUNDDOWN ROUNDUP SEARCH SIGN SMALL SQRT STDEV SUBSTITUTE SUM SUMIF SUMIFS SUMPRODUCT SUMSQ SWITCH
  T TEXT TEXTJOIN TRIM TRUNC UPPER VALUE VAR VLOOKUP WEEKDAY XLOOKUP XOR YEAR, plus TRUE/FALSE literals.
  Newer functions appear with the OOXML prefix, e.g. `_xlfn.XLOOKUP(...)`.
- Dates are serial numbers (1900 date system, as LibreOffice reads XLSX).

Behavior you must discover yourself: type coercion between text, numbers and booleans, error
propagation, criteria syntax and wildcards in the *IF/*IFS functions, number-to-text conversion,
TEXT() format codes, date arithmetic edge cases, lookup match modes, and so on.

## How you are scored

The hidden test set has 160 workbooks drawn from **the same generator** as the 40 public examples
in `/task/public/books` (expected values in `/task/public/expected`).

1. **Correctness gate:** at least 99.9% of hidden formula cells correct **and** at least 95% of
   hidden workbooks fully correct.
2. **Speed:** if the gate passes, the score is your speed relative to LibreOffice (warm process:
   load + full recalculation, startup excluded) on four large hidden workloads shaped like the
   public ones in `/task/public/perf`: a 60k-formula dependency chain 20k deep; 1,000
   SUMIFS/COUNTIFS/AVERAGEIFS over 50k rows; 5,000 exact lookups over 20k rows; and 10,000 mixed
   formulas. Your time includes process start, parsing, calculation and writing JSON.
   Parity scores 0.5, 8x faster scores 1.0, 8x slower scores 0. A workload counts only if at
   least 99.9% of its cells are correct. LibreOffice's times on the public workloads are in
   `/task/public/perf/libreoffice_baseline.json` (seconds, `total_s`).
3. Correct-cell rate and speed are both reported even when the gate fails.

## Grading environment

A clean Debian container with **no network**, **no LibreOffice**, and **no spreadsheet
libraries**, 4 CPU cores, 6 GB RAM, as an unprivileged user. 30 s per workbook, 120 s per
performance workload. Same toolchains as this sandbox:

- C / C++ (gcc 14, clang): system libraries zlib, expat, pugixml, libzip only.
- Rust 1.85 + Cargo, offline: only the vendored crates listed in `/task/RUST_CRATES.txt`
  (zip, flate2, quick-xml, roxmltree, serde, serde_json, rayon, hashbrown, ahash, smallvec, memchr,
  itoa, ryu, mimalloc and their dependencies).
- Go 1.24: standard library only.  Python 3.13 and Node 20: standard library only.

No spreadsheet or formula libraries of any kind, no copying LibreOffice code, no calling external
programs at runtime.

## Deliverable

Everything in `/workspace/submission/`:

- `build.sh`: builds offline from source in the grading container (30-minute limit) and leaves an
  executable `./calc` at the submission root.
- Your source code. No prebuilt binaries, archives or build outputs (they are rejected).
- Optional `NOTES.md` describing your approach.

## Tools in this sandbox

- `lo-recalc a.xlsx [b.xlsx ...] [-o outdir]`: recalculates workbooks with LibreOffice and saves
  copies with computed values (default outdir `/tmp/lo-recalc-out`).
- `xlsx-values file.xlsx`: prints every cell's formula, type and value as JSON (same reader as the
  grader). Combine with `lo-recalc` to see what LibreOffice computes for any formula you write.
- `check-public [--perf] [--show N] [--no-build]`: builds your submission and scores it on the
  public set with the grader's exact comparison rules; `--perf` also times the public workloads.
- `python3` with `openpyxl` for creating probe workbooks (openpyxl is NOT in the grader).
- When the display is enabled: LibreOffice Calc is open on the screen, and the screenshot, click,
  type and key tools operate it.
