"""Speed baseline: LibreOffice Calc in a warm process via UNO.

Times loadComponentFromURL (with recalc-on-load disabled) + calculateAll(), i.e. one parse and
one full recalculation, excluding soffice startup. 1 warm-up + N measured runs, median reported.
python3 baseline.py --books DIR --runs 5 --out baseline.json
"""
import argparse, glob, json, os, shutil, statistics, subprocess, sys, tempfile, time
import uno
from com.sun.star.beans import PropertyValue


def pv(name, value):
    p = PropertyValue(); p.Name = name; p.Value = value; return p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--books", required=True)
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    prof = tempfile.mkdtemp(prefix="lo-base-")
    shutil.copytree("/opt/lo-profile-template", prof, dirs_exist_ok=True)
    x = os.path.join(prof, "user", "registrymodifications.xcu")
    s = open(x).read().replace('<prop oor:name="OOXMLRecalcMode" oor:op="fuse"><value>0</value>',
                               '<prop oor:name="OOXMLRecalcMode" oor:op="fuse"><value>1</value>')
    open(x, "w").write(s)
    port = 2002
    proc = subprocess.Popen(["soffice", "--headless", "--invisible", "--norestore", "--nologo",
                             f"-env:UserInstallation=file://{prof}",
                             f"--accept=socket,host=127.0.0.1,port={port};urp;StarOffice.ComponentContext"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
    ctx = None
    for _ in range(120):
        try:
            ctx = resolver.resolve(f"uno:socket,host=127.0.0.1,port={port};urp;StarOffice.ComponentContext")
            break
        except Exception:
            time.sleep(0.5)
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    results = {}
    for b in sorted(glob.glob(os.path.join(a.books, "*.xlsx"))):
        url = uno.systemPathToFileUrl(os.path.abspath(b))
        loads, calcs = [], []
        for i in range(a.runs + 1):
            t0 = time.perf_counter()
            doc = desktop.loadComponentFromURL(url, "_blank", 0, (pv("Hidden", True),))
            t1 = time.perf_counter()
            doc.calculateAll()
            t2 = time.perf_counter()
            doc.close(True)
            if i > 0:
                loads.append(t1 - t0); calcs.append(t2 - t1)
        tot = [l + c for l, c in zip(loads, calcs)]
        results[os.path.basename(b)] = {
            "load_s": statistics.median(loads), "calc_s": statistics.median(calcs),
            "total_s": statistics.median(tot),
            "cv": statistics.pstdev(tot) / statistics.mean(tot) if len(tot) > 1 else 0.0, "runs": tot}
        print(os.path.basename(b), json.dumps(results[os.path.basename(b)]), flush=True)
    try:
        desktop.terminate()
    except Exception:
        pass
    proc.wait(timeout=30)
    json.dump(results, open(a.out, "w"), indent=1)


if __name__ == "__main__":
    main()
