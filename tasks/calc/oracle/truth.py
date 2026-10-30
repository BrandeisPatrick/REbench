"""Ground truth: recalculate every book with LibreOffice and record each formula cell's value.

python3 truth.py --books DIR --out DIR [--check]
  --check  convert twice and report any cell that differs between the two runs.
"""
import argparse, glob, json, os, shutil, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(__file__))
from xlsxio import read_xlsx

PROFILE_TEMPLATE = "/opt/lo-profile-template"


def convert(books, outdir):
    prof = tempfile.mkdtemp(prefix="lo-prof-")
    shutil.copytree(PROFILE_TEMPLATE, prof, dirs_exist_ok=True)
    os.makedirs(outdir, exist_ok=True)
    for i in range(0, len(books), 40):
        chunk = books[i:i + 40]
        cmd = ["soffice", "--headless", "--norestore", "--nologo", f"-env:UserInstallation=file://{prof}",
               "--convert-to", "xlsx:Calc MS Excel 2007 XML", "--outdir", outdir] + chunk
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3600)
    shutil.rmtree(prof, ignore_errors=True)


def extract(book, converted):
    src, got = read_xlsx(book), read_xlsx(converted)
    truth = {}
    for sheet, cells in src.items():
        for ref, c in cells.items():
            if c["f"] is None:
                continue
            g = got.get(sheet, {}).get(ref, {"t": "z", "v": None})
            truth.setdefault(sheet, {})[ref] = {"f": c["f"], "t": g["t"], "v": g["v"]}
    return truth


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--books", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    books = sorted(glob.glob(os.path.join(a.books, "*.xlsx")))
    os.makedirs(a.out, exist_ok=True)
    t0 = time.time()
    tmp1 = tempfile.mkdtemp(prefix="conv1-")
    convert(books, tmp1)
    stats = {"books": 0, "cells": 0, "types": {}, "errors": {}, "missing": 0, "nondeterministic": 0}
    tmp2 = None
    if a.check:
        tmp2 = tempfile.mkdtemp(prefix="conv2-")
        convert(books, tmp2)
    for b in books:
        name = os.path.basename(b)
        conv = os.path.join(tmp1, name)
        if not os.path.exists(conv):
            print("CONVERSION FAILED", name, file=sys.stderr); continue
        t = extract(b, conv)
        if tmp2:
            t2 = extract(b, os.path.join(tmp2, name))
            for sh in t:
                for ref in t[sh]:
                    if t[sh][ref] != t2.get(sh, {}).get(ref):
                        stats["nondeterministic"] += 1
        with open(os.path.join(a.out, name[:-5] + ".json"), "w") as f:
            json.dump(t, f, sort_keys=True)
        stats["books"] += 1
        for sh in t.values():
            for c in sh.values():
                stats["cells"] += 1
                stats["types"][c["t"]] = stats["types"].get(c["t"], 0) + 1
                if c["t"] == "e":
                    stats["errors"][c["v"]] = stats["errors"].get(c["v"], 0) + 1
                if c["t"] == "z":
                    stats["missing"] += 1
    stats["seconds"] = round(time.time() - t0, 1)
    print(json.dumps(stats, indent=1))


if __name__ == "__main__":
    main()
