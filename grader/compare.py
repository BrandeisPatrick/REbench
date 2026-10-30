"""Cell comparison rules, shared by the grader and the agent's public self-check."""
import math
import re

REL_TOL = 1e-9
ABS_TOL = 1e-12
FUNC_RE = re.compile(r"(?:_xlfn\.)?([A-Z][A-Z0-9.]*)\(")


def cell_equal(exp, act):
    if not isinstance(act, dict):
        return False
    te = exp["t"]
    if act.get("t") != te:
        return False
    ve, va = exp["v"], act.get("v")
    if te == "n":
        if isinstance(va, bool) or not isinstance(va, (int, float)):
            return False
        va = float(va)
        if math.isnan(va) or math.isinf(va):
            return False
        return abs(va - ve) <= max(REL_TOL * abs(ve), ABS_TOL)
    return ve == va


def functions_in(formula):
    return sorted(set(FUNC_RE.findall(formula or "")))


def compare_book(expected, actual):
    """expected: {sheet: {ref: {t, v, f}}}; actual: same shape from the rebuild (f optional)."""
    total = match = 0
    mismatches = []
    per_func = {}
    if not isinstance(actual, dict):
        actual = {}
    for sheet, cells in expected.items():
        got = actual.get(sheet)
        got = got if isinstance(got, dict) else {}
        for ref, e in cells.items():
            total += 1
            ok = cell_equal(e, got.get(ref))
            match += ok
            for fn in functions_in(e.get("f")):
                a = per_func.setdefault(fn, [0, 0])
                a[0] += 1
                a[1] += ok
            if not ok:
                mismatches.append({"sheet": sheet, "ref": ref, "formula": e.get("f"),
                                   "expected": {"t": e["t"], "v": e["v"]}, "got": got.get(ref)})
    return {"total": total, "match": match, "mismatches": mismatches, "per_func": per_func}
