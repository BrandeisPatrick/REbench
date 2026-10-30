"""Seeded workbook generator for the calc task (v0).

Usage:
  python3 gen.py split  --out DIR --seed S --count N      # random books, 30-90 formula cells each
  python3 gen.py perf   --out DIR --seed S                # large performance workloads

Every formula cell is written WITHOUT a cached value, so a rebuild cannot read answers
from the file. Ground truth comes from LibreOffice (truth.py).
"""
import argparse
import os
import random

from openpyxl import Workbook

XLFN = {"CONCAT", "TEXTJOIN", "IFS", "SWITCH", "XLOOKUP", "XOR", "IFNA", "DAYS"}
WORDS = ["apple", "Banana", "cherry", "date", "Elder", "fig", "grape", "kiwi", "lemon", "mango",
         "olive", "peach", "Pear", "plum", "quince", "berry pie", " lime ", "Melon", "a-b", "x"]
CATS = ["Fruit", "Veg", "Dairy", "Grain", "Meat"]
REGIONS = ["North", "South", "East", "West"]
TEXT_FMTS = ["0", "0.00", "#,##0", "#,##0.00", "0%", "0.0%", "000", "0.000E+00",
             "yyyy-mm-dd", "mm/dd/yyyy", "d-mmm-yy", "dddd"]


def fn(name, *args):
    pre = "_xlfn." if name in XLFN else ""
    return f"{pre}{name}({','.join(args)})"


def q(s):
    return '"' + s.replace('"', '""') + '"'


def col(c):
    s = ""
    while c:
        c, r = divmod(c - 1, 26)
        s = chr(65 + r) + s
    return s


class Book:
    def __init__(self, rng, n_rows, n_formulas):
        self.r = rng
        self.n = n_rows
        self.nf = n_formulas
        self.inputs = {"n": [], "s": [], "b": [], "z": [], "d": []}
        self.formulas = {"n": [], "s": [], "b": []}
        self.cells = {}  # (sheet, ref) -> value or formula

    # ---------- data ----------
    def build_data(self):
        r = self.r
        hdr = ["ID", "Name", "Category", "Region", "Amount", "Qty", "Date", "Flag"]
        for i, h in enumerate(hdr):
            self.cells[("Data", f"{col(i + 1)}1")] = h
        idv = r.randint(1, 5)
        self.ids = []
        for row in range(2, self.n + 2):
            self.ids.append(idv)
            vals = [idv, r.choice(WORDS) + (str(r.randint(1, 9)) if r.random() < 0.3 else ""),
                    r.choice(CATS), r.choice(REGIONS),
                    round(r.uniform(-100, 1000), r.choice([0, 1, 2])), r.randint(0, 50),
                    r.randint(40000, 46000), r.random() < 0.5]
            for i, v in enumerate(vals):
                if i in (4, 5) and r.random() < 0.06:
                    continue  # blank
                self.cells[("Data", f"{col(i + 1)}{row}")] = v
            idv += r.randint(1, 3)
        # horizontal table for HLOOKUP
        for c in range(1, 13):
            self.cells[("Lookup", f"{col(c)}1")] = f"K{c}"
            self.cells[("Lookup", f"{col(c)}2")] = round(r.uniform(0, 100), 2)
            self.cells[("Lookup", f"{col(c)}3")] = r.choice(WORDS)

    def build_inputs(self):
        r = self.r
        for row in range(1, 16):
            for c in range(1, 4):
                ref = f"{col(c)}{row}"
                x = r.random()
                if x < 0.45:
                    v = r.choice([r.randint(-20, 200), round(r.uniform(-50, 500), r.randint(1, 4)), 0])
                    self.inputs["n"].append(ref)
                elif x < 0.65:
                    v = r.choice(WORDS + ["12", "3.5", "  7 ", "TRUE", "", "1e3", "abc def"])
                    if v == "":
                        continue
                    self.inputs["s"].append(ref)
                elif x < 0.75:
                    v = r.random() < 0.5
                    self.inputs["b"].append(ref)
                elif x < 0.85:
                    self.inputs["z"].append(ref)
                    continue
                else:
                    v = r.randint(36000, 47000)
                    self.inputs["d"].append(ref)
                    self.inputs["n"].append(ref)
                self.cells[("Calc", ref)] = v

    # ---------- ranges ----------
    def drange(self, letter, absolute=None):
        a = self.r.random() < 0.5 if absolute is None else absolute
        d = "$" if a else ""
        return f"Data!{d}{letter}{d}2:{d}{letter}{d}{self.n + 1}"

    def table(self):
        return f"Data!$A$2:$H${self.n + 1}"

    def in_range(self):
        c = self.r.choice("ABC")
        a, b = sorted(self.r.sample(range(1, 16), 2))
        return f"{c}{a}:{c}{b}"

    # ---------- leaves ----------
    def num_lit(self):
        r = self.r
        return str(r.choice([r.randint(-10, 100), round(r.uniform(-20, 200), r.randint(1, 3)), 0, 1, 2, 0.5]))

    def leaf(self, t):
        r = self.r
        x = r.random()
        if t == "n":
            if x < 0.30:
                return self.num_lit()
            if x < 0.65 and self.inputs["n"]:
                return r.choice(self.inputs["n"])
            if x < 0.85 and self.formulas["n"]:
                return r.choice(self.formulas["n"])
            if x < 0.93 and (self.inputs["s"] or self.inputs["b"]):
                return r.choice(self.inputs["s"] + self.inputs["b"])  # coercion / #VALUE!
            if self.inputs["z"]:
                return r.choice(self.inputs["z"])  # blank -> 0
            return self.num_lit()
        if t == "s":
            if x < 0.35:
                return q(r.choice(WORDS + ["", "Hello World", "a,b,c", "42"]))
            if x < 0.65 and self.inputs["s"]:
                return r.choice(self.inputs["s"])
            if x < 0.9 and self.formulas["s"]:
                return r.choice(self.formulas["s"])
            return q(r.choice(WORDS))
        # bool
        if x < 0.25:
            return r.choice(["TRUE", "FALSE"])
        if x < 0.5 and self.inputs["b"]:
            return r.choice(self.inputs["b"])
        if x < 0.7 and self.formulas["b"]:
            return r.choice(self.formulas["b"])
        return f"{self.gen('n', 0)}{r.choice(['>', '<', '>=', '<=', '=', '<>'])}{self.gen('n', 0)}"

    def date(self, d):
        r = self.r
        if r.random() < 0.5 and self.inputs["d"]:
            return r.choice(self.inputs["d"])
        if r.random() < 0.5:
            return f"Data!G{r.randint(2, self.n + 1)}"
        return fn("DATE", str(r.randint(1990, 2030)), str(r.randint(-2, 14)), str(r.randint(-5, 40)))

    def crit(self):
        r = self.r
        kind = r.random()
        if kind < 0.3:
            return q(r.choice(CATS + REGIONS))
        if kind < 0.55:
            return q(r.choice([">", "<", ">=", "<=", "<>", "="]) + str(r.randint(-50, 600)))
        if kind < 0.7:
            return q(r.choice(["F*", "*a*", "?eg", "N*", "<>West", "*t"]))
        if kind < 0.85:
            return q(r.choice([">", "<=", "<>"])) + "&" + self.gen("n", 0)
        return r.choice(self.inputs["s"] or ['"Fruit"'])

    # ---------- productions ----------
    def gen(self, t, depth, top=False):
        r = self.r
        if depth <= 0 or (r.random() < 0.22 and not top):
            return self.leaf(t)
        d = depth - 1
        g = self.gen
        if t == "n":
            p = r.randrange(36)
            if p < 6:
                op = r.choice(["+", "-", "*", "/", "+", "*"])
                return f"{g('n', d)}{op}{g('n', d)}"
            if p == 6:
                return f"{g('n', d)}^{r.choice(['2', '3', '0.5', '0', '-1'])}"
            if p == 7:
                return f"-({g('n', d)})" if r.random() < 0.5 else f"{g('n', d)}%"
            if p == 8:
                name = r.choice(["SUM", "PRODUCT", "MIN", "MAX", "AVERAGE", "COUNT", "COUNTA", "COUNTBLANK"])
                if name == "COUNTBLANK" or r.random() < 0.6:
                    a = self.in_range() if r.random() < 0.5 else self.drange(r.choice("EFAH"))
                    return fn(name, a) if name == "COUNTBLANK" else fn(name, a, *( [g('n', d)] if r.random() < 0.3 else []))
                return fn(name, *[g("n", d) for _ in range(r.randint(1, 4))])
            if p == 9:
                name = r.choice(["ROUND", "ROUNDUP", "ROUNDDOWN", "TRUNC"])
                return fn(name, g("n", d), str(r.randint(-1, 3)))
            if p == 10:
                name = r.choice(["INT", "ABS", "SIGN", "SQRT", "EXP", "LN", "LOG10"])
                inner = g("n", d)
                if name == "EXP":
                    inner = f"{inner}/100"
                if name in ("SQRT", "LN", "LOG10") and r.random() < 0.7:
                    inner = fn("ABS", inner) + "+1"
                return fn(name, inner)
            if p == 11:
                return fn(r.choice(["MOD", "POWER", "LOG"]), g("n", d), r.choice(["2", "3", "10", "7", "-3", "0.5", g("n", 0)]))
            if p == 12:
                return fn(r.choice(["MEDIAN", "STDEV", "VAR", "SUMSQ"]), self.drange(r.choice("EF")))
            if p == 13:
                return fn(r.choice(["LARGE", "SMALL"]), self.drange(r.choice("EF")), str(r.randint(1, min(8, self.n + 2))))
            if p == 14:
                return fn("SUMPRODUCT", self.drange("E"), self.drange("F"))
            if p == 15:
                return fn("SUMIF", self.drange(r.choice("CDF")), self.crit(), self.drange("E"))
            if p == 16:
                return fn("SUMIFS", self.drange("E"), self.drange("C"), self.crit(), self.drange(r.choice("DF")), self.crit())
            if p == 17:
                return fn(r.choice(["COUNTIF", "AVERAGEIF"]), self.drange(r.choice("CDEF")), self.crit())
            if p == 18:
                pairs = []
                for _ in range(r.randint(1, 2)):
                    pairs += [self.drange(r.choice("CDEF")), self.crit()]
                if r.random() < 0.5:
                    return fn("COUNTIFS", *pairs)
                return fn("AVERAGEIFS", self.drange("E"), *pairs)
            if p == 19:
                key = str(r.choice(self.ids)) if r.random() < 0.8 else str(r.randint(0, self.ids[-1] + 5))
                return fn("VLOOKUP", key, self.table(), str(r.choice([1, 5, 6, 7])), r.choice(["FALSE", "0", "TRUE"]))
            if p == 20:
                return fn("HLOOKUP", q(f"K{r.randint(1, 14)}"), "Lookup!$A$1:$L$3", "2", "FALSE")
            if p == 21:
                if r.random() < 0.5:
                    return fn("INDEX", self.drange(r.choice("AEFG")), str(r.randint(1, self.n + 1)))
                return fn("INDEX", self.table(), str(r.randint(1, self.n)), str(r.choice([1, 5, 6, 7])))
            if p == 22:
                key = str(r.choice(self.ids)) if r.random() < 0.8 else str(r.randint(0, self.ids[-1] + 5))
                return fn("MATCH", key, self.drange("A"), r.choice(["0", "1", "0"]))
            if p == 23:
                key = str(r.choice(self.ids)) if r.random() < 0.8 else str(r.randint(0, self.ids[-1] + 5))
                return fn("XLOOKUP", key, self.drange("A"), self.drange(r.choice("EF")), *(["-1"] if r.random() < 0.5 else []))
            if p == 24:
                return fn("IF", g("b", d), g("n", d), *([g("n", d)] if r.random() < 0.85 else []))
            if p == 25:
                return fn(r.choice(["IFERROR", "IFNA"]), g("n", d), g("n", 0))
            if p == 26:
                return fn("IFS", g("b", d), g("n", d), g("b", d), g("n", d), *(["TRUE", g("n", 0)] if r.random() < 0.7 else []))
            if p == 27:
                return fn("SWITCH", fn("MOD", fn("INT", g("n", d)), "4"), "0", g("n", d), "1", g("n", d), *([g("n", 0)] if r.random() < 0.7 else []))
            if p == 28:
                return fn("CHOOSE", str(r.randint(1, 3)) if r.random() < 0.7 else fn("MOD", fn("INT", g("n", 0)), "4") + "+1",
                          g("n", d), g("n", d), g("n", d))
            if p == 29:
                return fn(r.choice(["LEN", "VALUE", "N"]), g("s", d))
            if p == 30:
                return fn(r.choice(["FIND", "SEARCH"]), q(r.choice(["a", "e", "an", "A", " ", "x"])), g("s", d),
                          *([str(r.randint(1, 4))] if r.random() < 0.3 else []))
            if p == 31:
                return fn(r.choice(["YEAR", "MONTH", "DAY"]), self.date(d))
            if p == 32:
                return fn("WEEKDAY", self.date(d), *([r.choice(["1", "2", "3"])] if r.random() < 0.6 else []))
            if p == 33:
                return fn(r.choice(["EDATE", "EOMONTH"]), self.date(d), str(r.randint(-14, 14)))
            if p == 34:
                a, b = self.date(d), self.date(d)
                if r.random() < 0.5:
                    return fn("DAYS", a, b)
                return fn("DATEDIF", fn("MIN", a, b), fn("MAX", a, b), q(r.choice(["d", "m", "y", "md", "ym", "yd"])))
            return fn("PI") + "*" + g("n", d)
        if t == "s":
            p = r.randrange(16)
            if p < 3:
                return f"{g(r.choice('ssn'), d)}&{g(r.choice('ssn'), d)}"
            if p == 3:
                return fn("CONCATENATE", *[g(r.choice("ssn"), d) for _ in range(r.randint(2, 3))])
            if p == 4:
                return fn("CONCAT", self.in_range() if r.random() < 0.4 else g("s", d), g("s", d))
            if p == 5:
                return fn("TEXTJOIN", q(r.choice([",", "-", " ", ""])), r.choice(["TRUE", "FALSE"]),
                          self.in_range() if r.random() < 0.5 else g("s", d), g(r.choice("sn"), d))
            if p == 6:
                return fn(r.choice(["LEFT", "RIGHT"]), g("s", d), *([str(r.randint(0, 6))] if r.random() < 0.8 else []))
            if p == 7:
                return fn("MID", g("s", d), str(r.randint(1, 5)), str(r.randint(0, 6)))
            if p == 8:
                return fn(r.choice(["UPPER", "LOWER", "PROPER", "TRIM", "T"]), g("s", d))
            if p == 9:
                return fn("SUBSTITUTE", g("s", d), q(r.choice(["a", "e", " ", "an"])), q(r.choice(["", "_", "XY"])),
                          *([str(r.randint(1, 3))] if r.random() < 0.3 else []))
            if p == 10:
                return fn("REPLACE", g("s", d), str(r.randint(1, 4)), str(r.randint(0, 3)), q(r.choice(["*", "zz", ""])))
            if p == 11:
                return fn("REPT", g("s", d), str(r.randint(0, 3)))
            if p == 12:
                fmt = r.choice(TEXT_FMTS)
                v = self.date(d) if fmt[0] in "ymd" else g("n", d)
                return fn("TEXT", v, q(fmt))
            if p == 13:
                return fn("IF", g("b", d), g("s", d), g("s", d))
            if p == 14:
                key = str(r.choice(self.ids)) if r.random() < 0.85 else "0"
                if r.random() < 0.5:
                    return fn("VLOOKUP", key, self.table(), str(r.choice([2, 3, 4])), "FALSE")
                return fn("XLOOKUP", key, self.drange("A"), self.drange(r.choice("BCD")), q("none"))
            return fn("IFERROR", g("s", d), q("err"))
        # bool
        p = r.randrange(9)
        if p < 2:
            op = r.choice([">", "<", ">=", "<=", "=", "<>"])
            if r.random() < 0.7:
                return f"{g('n', d)}{op}{g('n', d)}"
            return f"{g('s', d)}{op}{g('s', d)}"
        if p == 2:
            return fn(r.choice(["AND", "OR", "XOR"]), *[g("b", d) for _ in range(r.randint(1, 3))])
        if p == 3:
            return fn("NOT", g("b", d))
        if p == 4:
            ref = r.choice(self.inputs["z"] + self.inputs["n"] + self.inputs["s"] or ["A1"])
            return fn("ISBLANK", ref)
        if p == 5:
            return fn(r.choice(["ISNUMBER", "ISTEXT", "ISLOGICAL"]), g(r.choice("nsb"), d))
        if p == 6:
            return fn(r.choice(["ISERROR", "ISNA"]), g("n", d))
        if p == 7:
            return fn("EXACT", g("s", d), g("s", d))
        return fn("IF", g("b", d), g("b", d), g("b", d))

    def build_formulas(self):
        r = self.r
        slots = [f"{c}{row}" for c in "EFGH" for row in range(1, 41)]
        for ref in slots[: self.nf]:
            t = r.choices("nsb", weights=[6, 3, 2])[0]
            self.cells[("Calc", ref)] = "=" + self.gen(t, r.randint(1, 3), top=True)
            self.formulas[t].append(ref)

    def save(self, path):
        wb = Workbook()
        ws = {"Calc": wb.active}
        ws["Calc"].title = "Calc"
        ws["Data"] = wb.create_sheet("Data")
        ws["Lookup"] = wb.create_sheet("Lookup")
        for (sh, ref), v in self.cells.items():
            ws[sh][ref] = v
        wb.save(path)


def make_book(seed, path):
    r = random.Random(seed)
    b = Book(r, n_rows=r.randint(20, 60), n_formulas=r.randint(30, 90))
    b.build_data()
    b.build_inputs()
    b.build_formulas()
    b.save(path)


def make_perf(out, seed):
    r = random.Random(seed)
    os.makedirs(out, exist_ok=True)
    # 1. deep dependency chain, 60k formulas, chain depth 20k
    wb = Workbook(); ws = wb.active; ws.title = "Calc"
    n = 20000
    for i in range(1, n + 1):
        ws[f"A{i}"] = round(r.uniform(-100, 100), 3)
        ws[f"B{i}"] = f"=A{i}*1.5+B{i - 1}*0.25" if i > 1 else "=A1*1.5"
        ws[f"C{i}"] = f"=IF(B{i}>A{i},B{i}-A{i},ROUND(A{i}/3,2))"
        ws[f"D{i}"] = f"=MOD(C{i}*7,13)+SQRT(ABS(B{i}))"
    wb.save(os.path.join(out, "perf_chain.xlsx"))
    # 2. conditional aggregation: 50k data rows, 1,000 SUMIFS/COUNTIFS/AVERAGEIFS
    wb = Workbook(); ws = wb.active; ws.title = "Calc"; d = wb.create_sheet("Data")
    rows = 50000
    for i in range(2, rows + 2):
        d[f"A{i}"] = r.choice(CATS); d[f"B{i}"] = r.choice(REGIONS)
        d[f"C{i}"] = round(r.uniform(0, 1000), 2); d[f"D{i}"] = r.randint(0, 50)
    rng = lambda c: f"Data!${c}$2:${c}${rows + 1}"
    for i in range(1, 1001):
        cat, reg, thr = r.choice(CATS), r.choice(REGIONS), r.randint(0, 50)
        k = i % 3
        if k == 0:
            ws[f"A{i}"] = f'=SUMIFS({rng("C")},{rng("A")},"{cat}",{rng("B")},"{reg}",{rng("D")},">{thr}")'
        elif k == 1:
            ws[f"A{i}"] = f'=COUNTIFS({rng("A")},"{cat}",{rng("D")},"<={thr}")'
        else:
            ws[f"A{i}"] = f'=AVERAGEIFS({rng("C")},{rng("B")},"{reg}",{rng("D")},">={thr}")'
    wb.save(os.path.join(out, "perf_aggregate.xlsx"))
    # 3. lookups: 20k-row table, 2,000 VLOOKUP + 2,000 MATCH + 1,000 XLOOKUP
    wb = Workbook(); ws = wb.active; ws.title = "Calc"; d = wb.create_sheet("Data")
    rows = 20000
    keys = r.sample(range(1, 10 * rows), rows)
    for i, kv in enumerate(keys, start=2):
        d[f"A{i}"] = kv; d[f"B{i}"] = r.choice(WORDS); d[f"C{i}"] = round(r.uniform(0, 100), 2)
    tbl = f"Data!$A$2:$C${rows + 1}"
    for i in range(1, 2001):
        ws[f"A{i}"] = f"=VLOOKUP({r.choice(keys) if r.random() < 0.9 else 0},{tbl},3,FALSE)"
        ws[f"B{i}"] = f"=MATCH({r.choice(keys)},Data!$A$2:$A${rows + 1},0)"
    for i in range(1, 1001):
        ws[f"C{i}"] = f'=_xlfn.XLOOKUP({r.choice(keys)},Data!$A$2:$A${rows + 1},Data!$B$2:$B${rows + 1},"none")'
    wb.save(os.path.join(out, "perf_lookup.xlsx"))
    # 4. broad mix: the random generator at scale (~10k formulas over 12 sheets-worth of slots)
    big = Book(r, n_rows=2000, n_formulas=160)
    big.build_data(); big.build_inputs()
    # widen the formula area: 10,000 cells in columns E..AN
    slots = [f"{col(c)}{row}" for c in range(5, 45) for row in range(1, 251)]
    for ref in slots:
        t = r.choices("nsb", weights=[6, 3, 2])[0]
        big.cells[("Calc", ref)] = "=" + big.gen(t, r.randint(1, 3), top=True)
        big.formulas[t].append(ref)
    big.save(os.path.join(out, "perf_mixed.xlsx"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["split", "perf"])
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--count", type=int, default=10)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    if a.mode == "split":
        for i in range(a.count):
            make_book(a.seed * 100003 + i, os.path.join(a.out, f"book_{a.seed}_{i:04d}.xlsx"))
    else:
        make_perf(a.out, a.seed)


if __name__ == "__main__":
    main()
