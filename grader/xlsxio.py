"""Minimal, dependency-free XLSX cell reader shared by oracle, grader and agent self-check.

read_xlsx(path) -> {sheet_name: {"A1": {"f": formula_or_None, "t": "n|s|b|e|z", "v": value}}}
  t = n number (float), s string, b boolean, e error code string ("#DIV/0!"), z no value.
"""
import posixpath
import xml.etree.ElementTree as ET
import zipfile

M = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
R_ID = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"


def _si_text(si):
    t = si.find(M + "t")
    if t is not None:
        return t.text or ""
    return "".join((r.findtext(M + "t") or "") for r in si.findall(M + "r"))


def read_xlsx(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    relmap = {r.get("Id"): r.get("Target") for r in rels}
    ss = []
    if "xl/sharedStrings.xml" in names:
        ss = [_si_text(si) for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall(M + "si")]
    out = {}
    for s in wb.find(M + "sheets"):
        target = relmap[s.get(R_ID)]
        p = target.lstrip("/") if target.startswith("/") else posixpath.normpath(posixpath.join("xl", target))
        root = ET.fromstring(z.read(p))
        cells = {}
        for c in root.iter(M + "c"):
            t = c.get("t", "n")
            f = c.find(M + "f")
            v = c.find(M + "v")
            vt = v.text if v is not None else None
            if t == "s":
                cell = {"t": "s", "v": ss[int(vt)]} if vt is not None else {"t": "z", "v": None}
            elif t in ("str", "inlineStr"):
                if t == "inlineStr":
                    is_ = c.find(M + "is")
                    vt = _si_text(is_) if is_ is not None else vt
                cell = {"t": "s", "v": vt or ""}
            elif t == "b":
                cell = {"t": "b", "v": vt == "1"} if vt is not None else {"t": "z", "v": None}
            elif t == "e":
                cell = {"t": "e", "v": vt}
            elif vt is None or vt == "":
                cell = {"t": "z", "v": None}
            else:
                cell = {"t": "n", "v": float(vt)}
            cell["f"] = f.text if f is not None else None
            cells[c.get("r")] = cell
        out[s.get("name")] = cells
    return out
