"""Grader for the calc task. Runs inside rb-grader with --network none.

Mounts: /sub (submission, ro)  /data (hidden + perf sets, ro)  /out (results, rw)
"""
import json, math, os, random, re, shutil, statistics, subprocess, sys, time, glob
sys.path.insert(0, os.path.dirname(__file__))
from compare import compare_book

SUB, DATA, OUT, WORK = "/sub", "/data", "/out", "/work/sub"
SECRET = "/root/secret"  # hidden truth + baselines; /root is mode 700, the submission runs as "runner"
AS_RUNNER = ("setpriv", "--reuid=runner", "--regid=runner", "--init-groups", "--reset-env")
RUNNER_ENV = {"PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": "/home/runner", "CARGO_HOME": "/opt/cargo",
              "CARGO_NET_OFFLINE": "true", "GOFLAGS": "-mod=mod", "GOPROXY": "off", "GOTOOLCHAIN": "local", "LANG": "C.UTF-8"}
BOOK_TIMEOUT = 30
CORRECTNESS_BUDGET = 1200
DENY = re.compile(r"\b(soffice|libreoffice|unoconv|openpyxl|xlrd|pandas|pycel|xlcalculator|formulas|ironcalc|hyperformula|calamine|umya)\b", re.I)
BINARY_MAGIC = (b"\x7fELF", b"\xcf\xfa\xed\xfe", b"MZ", b"PK\x03\x04", b"!<arch>")


def run_timed(cmd, timeout, cwd=WORK, prefix=()):
    t0 = time.perf_counter()
    p = subprocess.Popen(list(prefix) + list(AS_RUNNER) + ["env"] + [f"{k}={v}" for k, v in RUNNER_ENV.items()] + cmd,
                         cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    deadline = t0 + timeout
    status = None
    while True:
        pid, st, ru = os.wait4(p.pid, os.WNOHANG)
        if pid:
            status = st; break
        if time.perf_counter() > deadline:
            p.kill(); pid, st, ru = os.wait4(p.pid, 0)
            return {"status": "timeout", "wall": timeout, "cpu": ru.ru_utime + ru.ru_stime, "rss_mb": ru.ru_maxrss / 1024}
        time.sleep(0.002)
    wall = time.perf_counter() - t0
    err = p.stderr.read().decode(errors="replace")[-400:] if p.stderr else ""
    code = os.waitstatus_to_exitcode(status)
    return {"status": "ok" if code == 0 else f"exit {code}", "wall": wall, "cpu": ru.ru_utime + ru.ru_stime,
            "rss_mb": ru.ru_maxrss / 1024, "stderr": err}


def load_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except Exception:
        return None


FOREIGN_SRC = (".c", ".cc", ".cpp", ".cxx", ".h", ".hh", ".hpp", ".go", ".py", ".js", ".mjs", ".cjs", ".ts", ".java", ".rb", ".pl", ".lua")
FOREIGN_CMD = re.compile(r"(^|[\s;&|(`$])(gcc|g\+\+|clang|clang\+\+|cc|c\+\+|go|python3?|node|perl|ruby)(?=[\s;)]|$)", re.M)


def lang_violations(root, lang):
    """Rules for a fixed-language run (REQUIRE_LANG). Only "rust" is defined."""
    if lang != "rust":
        return [f"unknown language rule {lang}"]
    v, cargo, rs = [], 0, 0
    for r, _, files in os.walk(root):
        for fn in files:
            cargo += fn == "Cargo.toml"
            rs += fn.endswith(".rs")
            if fn.endswith(FOREIGN_SRC):
                v.append("non-Rust source " + os.path.relpath(os.path.join(r, fn), root))
    if not cargo or not rs:
        v.append("no Cargo project (Cargo.toml and .rs sources)")
    for fn in ("build.sh", "calc"):
        try:
            txt = open(os.path.join(root, fn), encoding="utf-8").read()
        except Exception:
            continue
        code = "\n".join(l.split("#", 1)[0] for l in txt.splitlines() if not l.startswith("#!"))
        v += [f"{fn} invokes {m.group(2)}" for m in FOREIGN_CMD.finditer(code)]
    return v


def precheck():
    info = {"files": 0, "bytes": 0, "binaries": [], "deny_hits": []}
    for root, _, files in os.walk(WORK):
        for fn in files:
            p = os.path.join(root, fn)
            info["files"] += 1
            info["bytes"] += os.path.getsize(p)
            with open(p, "rb") as f:
                head = f.read(4)
            if head.startswith(BINARY_MAGIC):
                info["binaries"].append(os.path.relpath(p, WORK))
                continue
            try:
                txt = open(p, encoding="utf-8").read()
            except Exception:
                continue
            for m in DENY.finditer(txt):
                info["deny_hits"].append(f"{os.path.relpath(p, WORK)}: {m.group(0)}")
    return info


def main():
    os.makedirs(OUT, exist_ok=True)
    res = {"task": "calc", "version": "v0", "started": time.strftime("%Y-%m-%dT%H:%M:%S")}
    if os.path.exists(WORK):
        shutil.rmtree(WORK)
    shutil.copytree(SUB, WORK, symlinks=False, ignore=shutil.ignore_patterns("target", "node_modules", "__pycache__", "*.o"))
    subprocess.run(["chown", "-R", "runner:runner", "/work"], check=True)
    res["precheck"] = pre = precheck()
    hard_fail = []
    if pre["binaries"]:
        hard_fail.append("prebuilt binaries or archives in submission: " + ", ".join(pre["binaries"][:5]))
    if os.environ.get("REQUIRE_LANG"):
        lang = os.environ["REQUIRE_LANG"]
        res["lang_rule"] = {"lang": lang, "violations": lang_violations(WORK, lang)}
        if res["lang_rule"]["violations"]:
            hard_fail.append(f"language rule ({lang}): " + "; ".join(res["lang_rule"]["violations"][:5]))
    # deny_hits are informational: the grader has no LibreOffice or spreadsheet libraries, so calling them fails anyway.
    # build
    b = {"status": "missing build.sh"}
    if os.path.exists(os.path.join(WORK, "build.sh")):
        t0 = time.time()
        p = subprocess.run(list(AS_RUNNER) + ["env"] + [f"{k}={v}" for k, v in RUNNER_ENV.items()] + ["bash", "build.sh"],
                           cwd=WORK, capture_output=True, text=True, timeout=1800)
        b = {"status": "ok" if p.returncode == 0 else f"exit {p.returncode}", "seconds": round(time.time() - t0, 1),
             "log_tail": (p.stdout + p.stderr)[-3000:]}
    res["build"] = b
    exe = os.path.join(WORK, "calc")
    if b["status"] != "ok" or not os.access(exe, os.X_OK):
        hard_fail.append(f"build failed or ./calc missing ({b['status']})")
    res["hard_fail"] = hard_fail
    if hard_fail:
        return finish(res)

    # correctness on the hidden set
    books = sorted(glob.glob(f"{DATA}/hidden_books/*.xlsx"))
    os.makedirs("/tmp/o", exist_ok=True)
    os.chmod("/tmp/o", 0o777)
    tot = match = perfect = 0
    statuses, per_func, mism = {}, {}, []
    kinds = {"wrong_type": 0, "wrong_value": 0, "missing": 0}
    t_start = time.time()
    outputs = {}
    for bk in books:
        name = os.path.basename(bk)[:-5]
        truth = load_json(f"{SECRET}/hidden_truth/{name}.json")
        outp = f"/tmp/o/{name}.json"
        if os.path.exists(outp):
            os.remove(outp)
        if time.time() - t_start > CORRECTNESS_BUDGET:
            r = {"status": "skipped (budget)"}
        else:
            r = run_timed(["./calc", "eval", "--in", bk, "--out", outp], BOOK_TIMEOUT)
        got = load_json(outp) if r["status"] == "ok" else None
        st = r["status"] if got is not None or r["status"] != "ok" else "bad json"
        statuses[st] = statuses.get(st, 0) + 1
        c = compare_book(truth, got)
        outputs[name] = got
        tot += c["total"]; match += c["match"]; perfect += c["match"] == c["total"]
        for fn_, (n, ok) in c["per_func"].items():
            a = per_func.setdefault(fn_, [0, 0]); a[0] += n; a[1] += ok
        for m in c["mismatches"]:
            g = m["got"]
            if not isinstance(g, dict):
                kinds["missing"] += 1
            elif g.get("t") != m["expected"]["t"]:
                kinds["wrong_type"] += 1
            else:
                kinds["wrong_value"] += 1
            if len(mism) < 300:
                mism.append(dict(m, book=name))
    res["correctness"] = {
        "books": len(books), "cells": tot, "cells_matched": match,
        "cell_match_rate": match / tot if tot else 0.0,
        "books_perfect": perfect, "books_perfect_rate": perfect / len(books) if books else 0.0,
        "run_status": statuses, "mismatch_kinds": kinds, "seconds": round(time.time() - t_start, 1),
        "per_function": {k: {"cells": v[0], "match_rate": round(v[1] / v[0], 4)} for k, v in sorted(per_func.items())},
    }
    json.dump(mism, open(f"{OUT}/mismatch_sample.json", "w"), indent=1)

    # determinism: re-run a seeded 10% sample
    rnd = random.Random(0)
    sample = rnd.sample(books, max(1, len(books) // 10))
    same = 0
    for bk in sample:
        name = os.path.basename(bk)[:-5]
        outp = f"/tmp/o/{name}.again.json"
        r = run_timed(["./calc", "eval", "--in", bk, "--out", outp], BOOK_TIMEOUT)
        same += r["status"] == "ok" and load_json(outp) == outputs.get(name)
    res["determinism"] = {"sampled": len(sample), "identical": same}

    if os.environ.get("SKIP_PERF"):
        res["performance_skipped"] = True
        return finish(res)
    # performance
    base4 = load_json(f"{SECRET}/baseline_4core.json")
    base1 = load_json(f"{SECRET}/baseline_1core.json")
    perf = {}
    for bk in sorted(glob.glob(f"{DATA}/perf_books/*.xlsx")):
        fname = os.path.basename(bk); name = fname[:-5]
        truth = load_json(f"{SECRET}/perf_truth/{name}.json")
        outp = f"/tmp/o/perf_{name}.json"
        warm = run_timed(["./calc", "eval", "--in", bk, "--out", outp], 120)
        got = load_json(outp) if warm["status"] == "ok" else None
        c = compare_book(truth, got)
        rate = c["match"] / c["total"] if c["total"] else 0.0
        entry = {"warmup": warm["status"], "match_rate": rate, "baseline_4core_s": base4[fname]["total_s"],
                 "baseline_1core_s": base1[fname]["total_s"]}
        if warm["status"] == "ok":
            n = 5 if warm["wall"] < 20 else 1
            runs = [run_timed(["./calc", "eval", "--in", bk, "--out", outp], 120) for _ in range(n)]
            walls = [x["wall"] for x in runs if x["status"] == "ok"]
            if walls:
                t4 = statistics.median(walls)
                entry.update({"wall_4core_s": t4, "cv": statistics.pstdev(walls) / statistics.mean(walls),
                              "cpu_s": statistics.median([x["cpu"] for x in runs]),
                              "peak_rss_mb": max(x["rss_mb"] for x in runs),
                              "speedup_vs_lo": base4[fname]["total_s"] / t4})
                one = [run_timed(["./calc", "eval", "--in", bk, "--out", outp], 240, prefix=("taskset", "-c", "0"))
                       for _ in range(3 if t4 < 10 else 1)]
                w1 = [x["wall"] for x in one if x["status"] == "ok"]
                if w1:
                    entry["wall_1core_s"] = statistics.median(w1)
                    entry["parallel_speedup"] = entry["wall_1core_s"] / t4
        perf[name] = entry
    res["performance"] = perf
    return finish(res)


def finish(res):
    subprocess.run(["pkill", "-9", "-u", "runner"], capture_output=True)
    c = res.get("correctness")
    gate = bool(c) and c["cell_match_rate"] >= 0.999 and c["books_perfect_rate"] >= 0.95
    ratios, raw = [], []
    for e in res.get("performance", {}).values():
        s = e.get("speedup_vs_lo")
        if s and e["match_rate"] >= 0.999:
            raw.append(s)
        ratios.append(s if (s and e["match_rate"] >= 0.999) else 1 / 8)
    E = math.exp(sum(map(math.log, ratios)) / len(ratios)) if ratios else 0.0
    E_raw = math.exp(sum(map(math.log, raw)) / len(raw)) if raw else 0.0
    eff = max(0.0, min(1.0, 0.5 + math.log2(E) / 6)) if E > 0 else 0.0
    res["score"] = {
        "resolved": gate, "E": E, "E_correct_workloads": E_raw, "correct_workloads": len(raw), "efficiency": eff,
        "task_score": eff if gate else 0.0,
        "cell_match_rate": c["cell_match_rate"] if c else 0.0,
        "books_perfect_rate": c["books_perfect_rate"] if c else 0.0,
    }
    res["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    json.dump(res, open(f"{OUT}/reward.json", "w"), indent=1)
    open(f"{OUT}/reward.txt", "w").write(f"{res['score']['task_score']:.4f}\n")
    print(json.dumps(res["score"], indent=1))


if __name__ == "__main__":
    main()
