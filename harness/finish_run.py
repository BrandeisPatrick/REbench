"""Finish a subagent-driven run: collect, audit, (optionally) grade on an evaluation set.

python harness/finish_run.py <run_id> --transcript <subagent jsonl> [--evalset NAME] [--result-file F]
python harness/finish_run.py <run_id> --grade-only --evalset NAME
"""
import argparse, fcntl, json, os, subprocess, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from run import RB, sh, collect, process_metrics


import re
HEREDOC = re.compile(r"<<-?\s*(['\"]?)(\w+)\1[^\n]*\n.*?\n\s*\2\s*(?=\n|$)", re.S)


def segments(cmd):
    """Split a shell command into simple commands at unquoted ; & | && || and newlines, ignoring heredoc bodies."""
    body = HEREDOC.sub("<<HEREDOC", cmd)
    segs, cur, q, i = [], [], None, 0
    while i < len(body):
        ch = body[i]
        if q:
            cur.append(ch)
            if ch == "\\" and q == '"' and i + 1 < len(body):
                cur.append(body[i + 1]); i += 1
            elif ch == q:
                q = None
        elif ch in "'\"":
            q = ch; cur.append(ch)
        elif ch == "\\" and i + 1 < len(body):
            cur.append(ch); cur.append(body[i + 1]); i += 1
        elif ch in ";&|\n":
            segs.append("".join(cur)); cur = []
        else:
            cur.append(ch)
        i += 1
    segs.append("".join(cur))
    return [x.strip() for x in segs if x.strip()]


def ok_segment(seg, sbx, aliases=(), rel_ok=False):
    seg = seg.lstrip("( ").rstrip(") ")
    words = seg.split()
    if not words:
        return True
    first = words[0].strip("'\"")
    if re.fullmatch(r"\w+=\S+", first):
        if first.split("=", 1)[1].strip("'\"") == sbx and len(words) == 1:
            return True  # SBX=/path/to/sbx
        words = words[1:]; first = words[0].strip("'\"") if words else ""
    if first in (sbx, "sbx", "$SBX", "${SBX}") or first in aliases or first in ("echo", "printf", "true") or first.startswith("<<"):
        return True
    rd = os.path.dirname(sbx)
    if first == "./sbx" and rel_ok:
        return True  # ./sbx after cd into the agent's own run folder
    if first == "cd" and len(words) == 2 and words[1].strip("'\"").rstrip("/") == rd:
        return True  # cd into the agent's own run folder (contains only sbx and its logs)
    if re.fullmatch(r"cat\s*>\s*/dev/null\s+<<HEREDOC\s*", " ".join(words)):
        return True  # no-op
    return first == "cat" and re.fullmatch(r"cat\s+(-\s+)?<<HEREDOC\s*", " ".join(words)) is not None


def audit(rd, transcript):
    sbx = f"{rd}/sbx"; shots = f"{rd}/screenshots/"
    counts, violations, usage = {}, [], {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}
    model = None
    for line in open(transcript):
        try:
            e = json.loads(line)
        except Exception:
            continue
        msg = e.get("message") or {}
        if e.get("type") == "assistant":
            model = msg.get("model", model)
            u = msg.get("usage") or {}
            usage["input"] += u.get("input_tokens", 0); usage["output"] += u.get("output_tokens", 0)
            usage["cache_read"] += u.get("cache_read_input_tokens", 0); usage["cache_write"] += u.get("cache_creation_input_tokens", 0)
            for b in msg.get("content") or []:
                if b.get("type") != "tool_use":
                    continue
                name, inp = b.get("name"), b.get("input") or {}
                counts[name] = counts.get(name, 0) + 1
                if name == "SubagentHandback":
                    continue
                if name == "Bash":
                    c = (inp.get("command") or "").strip()
                    names = re.findall(r"(?:^|[\s;])(\w+)=['\"]?" + re.escape(sbx) + r"['\"]?(?=\s|;|$)", c)
                    aliases = tuple(x for nm in names for x in (f"${nm}", "${" + nm + "}", f'"${nm}"'))
                    rel_ok = re.search(r"(?:^|[;&\s])cd\s+['\"]?" + re.escape(os.path.dirname(sbx)) + r"/?['\"]?(?=\s|;|&|$)", c) is not None
                    bad = [seg for seg in segments(c) if not ok_segment(seg, sbx, aliases, rel_ok)]
                    if bad:
                        violations.append({"tool": name, "command": c[:300], "bad_segments": bad[:3]})
                elif name == "Read":
                    if not (inp.get("file_path") or "").startswith(shots):
                        violations.append({"tool": name, "file_path": inp.get("file_path")})
                else:
                    violations.append({"tool": name, "input": json.dumps(inp)[:300]})
    a = {"model": model, "tool_uses": counts, "violations": violations, "violation_count": len(violations), "tokens": usage}
    json.dump(a, open(f"{rd}/audit.json", "w"), indent=1)
    return a


def grade(rd, evalset, skip_perf=False, lang=""):
    E = f"{RB}/data/calc/evalsets/{evalset}"
    out = f"{rd}/grade-{evalset}"; os.makedirs(out, exist_ok=True)
    lock = open(f"{RB}/runs/.grade.lock", "w"); fcntl.flock(lock, fcntl.LOCK_EX)
    cmd = ["docker", "run", "--rm", "--network", "none", "--cpus", "4", "--cpuset-cpus", "0-3", "--memory", "6g",
           "--memory-swap", "6g", "--pids-limit", "4096", "-v", f"{rd}/submission:/sub:ro",
           "-v", f"{E}/hidden_books:/data/hidden_books:ro", "-v", f"{E}/perf_books:/data/perf_books:ro",
           "-v", f"{E}/hidden_truth:/root/secret/hidden_truth:ro", "-v", f"{E}/perf_truth:/root/secret/perf_truth:ro",
           "-v", f"{E}/baseline_4core.json:/root/secret/baseline_4core.json:ro",
           "-v", f"{E}/baseline_1core.json:/root/secret/baseline_1core.json:ro", "-v", f"{out}:/out"] + \
          (["-e", "SKIP_PERF=1"] if skip_perf else []) + (["-e", f"REQUIRE_LANG={lang}"] if lang else []) + \
          ["rb-grader:latest", "python3", "/grader/grade.py"]
    p = sh(*cmd)
    open(f"{out}/grader_stdout.txt", "w").write(p.stdout + p.stderr)
    fcntl.flock(lock, fcntl.LOCK_UN)
    return json.load(open(f"{out}/reward.json"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_id"); ap.add_argument("--transcript"); ap.add_argument("--evalset")
    ap.add_argument("--grade-only", action="store_true"); ap.add_argument("--result-file")
    ap.add_argument("--skip-perf", action="store_true")
    a = ap.parse_args()
    rd = f"{RB}/runs/{a.run_id}"; meta = json.load(open(f"{rd}/meta.json"))
    if not a.grade_only:
        c = meta["container"]
        collect(c, rd); sh("docker", "rm", "-f", c)
        meta["finished"] = time.time(); meta["wall_minutes"] = round((meta["finished"] - meta["started"]) / 60, 1)
        if a.transcript:
            meta["audit"] = audit(rd, a.transcript)
        if a.result_file and os.path.exists(a.result_file):
            meta["final_message"] = open(a.result_file).read()[-3000:]
        meta["process"] = process_metrics(rd)
        json.dump(meta, open(f"{rd}/meta.json", "w"), indent=1)
    if a.evalset:
        r = grade(rd, a.evalset, a.skip_perf, "" if meta.get("lang_rule", "any") == "any" else meta["lang_rule"])
        meta.setdefault("grades", {})[a.evalset] = r.get("score")
        json.dump(meta, open(f"{rd}/meta.json", "w"), indent=1)
        print(json.dumps({"score": r.get("score"), "hard_fail": r.get("hard_fail"), "build": r.get("build", {}).get("status")}, indent=1))
    else:
        print(json.dumps({k: meta.get(k) for k in ("wall_minutes", "process", "audit")}, indent=1)[:3000])


if __name__ == "__main__":
    main()
