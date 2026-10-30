"""Run one agent attempt on a rebuild-bench task, then grade it.

python harness/run.py --model claude-opus-5-5 --condition full --minutes 120 [--run-id ID] [--defer-grade]
python harness/run.py --grade-only runs/<id>
"""
import argparse, fcntl, json, os, signal, subprocess, sys, time

RB = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA = f"{RB}/data/calc"
PY = f"{RB}/.venv/bin/python"

SYSTEM = ("You are an AI agent working alone on a long software engineering task inside a sandboxed Linux "
          "computer, which you operate only through the provided tools. Work autonomously and keep going until "
          "the task is done or the time budget runs out. Nobody will answer questions. The time remaining is shown "
          "after most tool results.")

TASK = """You have {minutes} minutes of wall-clock time.

Task: rebuild the formula engine of LibreOffice Calc from scratch as a command-line program that reads an
XLSX workbook, computes every formula, and writes the results. It must match LibreOffice's results exactly
and run as fast as possible. Read /task/CONTRACT.md first: it is the complete specification of the
deliverable, the allowed languages and dependencies, and the scoring.{lang}

{gui}
There is no internet access.

Work in /workspace. Only /workspace/submission is graded, later, in a separate clean container without
LibreOffice. Keep the submission building and producing output at all times, because the run ends when time
runs out, possibly in the middle of an edit. Use `check-public` often to measure progress on the public
examples. When you are completely finished, end your turn with a short summary of what you built."""

GUI = {"full": "LibreOffice Calc is open on a 1280x800 virtual display, and the screenshot, click, type_text, key, "
               "scroll and drag tools operate it. The lo-recalc and xlsx-values commands query LibreOffice from the shell.",
       "no-gui": "There is no display. Query LibreOffice from the shell with the lo-recalc and xlsx-values commands."}

LANG = {"any": "", "rust": " In this run the engine must be written in Rust."}

RUST_ONLY = """Toolchain for this run: **Rust only**.

- Rust 1.85 + Cargo, offline: only the vendored crates listed in `/task/RUST_CRATES.txt`
  (zip, flate2, quick-xml, roxmltree, serde, serde_json, rayon, hashbrown, ahash, smallvec, memchr,
  itoa, ryu, mimalloc and their dependencies).
- The submission must be a Cargo project (`Cargo.toml` plus `.rs` sources). Source files in any other
  language (C, C++, Go, Python, JavaScript and so on) are rejected, and `build.sh` must not invoke gcc,
  g++, clang, cc, go, python or node directly. Python stays installed in this sandbox only for the tools
  listed below."""

CONTINUE = ("[harness] You ended your turn with {left} minutes of the time budget left. Only /workspace/submission "
            "is graded. If you can still improve correctness or speed, keep working. If you are finished, reply "
            "with the single word DONE.")


def sh(*a, **k):
    return subprocess.run(list(a), capture_output=True, text=True, **k)


def child_env():
    env = {k: v for k, v in os.environ.items() if not k.startswith(("CLAUDECODE", "CLAUDE_CODE_", "CLAUDE_AGENT"))}
    return env


def run_claude(args, rd, prompt, resume, deadline, mcp_cfg, log_name):
    cmd = ["claude", "-p", prompt, "--model", args.model, "--tools", "", "--strict-mcp-config",
           "--mcp-config", mcp_cfg, "--allowedTools", "mcp__sandbox", "--system-prompt", SYSTEM,
           "--setting-sources", "", "--disable-slash-commands", "--no-chrome", "--output-format", "stream-json", "--verbose"]
    if resume:
        cmd += ["--resume", resume]
    cwd = f"{rd}/agent_cwd"
    os.makedirs(cwd, exist_ok=True)
    out = open(f"{rd}/{log_name}", "a", buffering=1)
    p = subprocess.Popen(cmd, cwd=cwd, env=child_env(), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    sid, result, killed = resume, None, False
    import selectors
    sel = selectors.DefaultSelector(); sel.register(p.stdout, selectors.EVENT_READ)
    while True:
        if time.time() > deadline and not killed:
            killed = True
            p.send_signal(signal.SIGINT); time.sleep(15)
            if p.poll() is None:
                p.terminate(); time.sleep(5)
            if p.poll() is None:
                p.kill()
        if sel.select(timeout=5):
            line = p.stdout.readline()
            if not line:
                break
            out.write(line)
            try:
                ev = json.loads(line)
            except Exception:
                continue
            if ev.get("type") == "system" and ev.get("subtype") == "init":
                sid = ev.get("session_id", sid)
            if ev.get("type") == "result":
                result = ev
        elif p.poll() is not None:
            break
    p.wait()
    return sid, result, killed


def collect(c, rd):
    os.makedirs(f"{rd}/submission", exist_ok=True)
    tar = subprocess.Popen(["docker", "exec", c, "tar", "-C", "/workspace", "-cf", "-", "--exclude=target",
                            "--exclude=node_modules", "--exclude=*.o", "--exclude=__pycache__", "submission"], stdout=subprocess.PIPE)
    subprocess.run(["tar", "-C", rd, "-xf", "-"], stdin=tar.stdout, check=False)
    tar.wait()
    # Build outputs are regenerated by build.sh in the grader, so strip compiled binaries (keep source scripts).
    removed = []
    for root, _, files in os.walk(f"{rd}/submission"):
        for fn in files:
            fp = os.path.join(root, fn)
            with open(fp, "rb") as f:
                head = f.read(4)
            if head in (b"\x7fELF", b"\xcf\xfa\xed\xfe") or head.startswith(b"!<ar"):
                os.remove(fp); removed.append(os.path.relpath(fp, rd))
    json.dump(removed, open(f"{rd}/stripped_binaries.json", "w"))
    open(f"{rd}/workspace_listing.txt", "w").write(sh("docker", "exec", c, "bash", "-c",
        "cd /workspace && find . -maxdepth 3 -not -path '*/target/*' | head -400; du -sh /workspace/* 2>/dev/null").stdout)


def grade(rd):
    os.makedirs(f"{rd}/grade", exist_ok=True)
    lock = open(f"{RB}/runs/.grade.lock", "w")
    fcntl.flock(lock, fcntl.LOCK_EX)  # one grading at a time, so timings do not interfere
    D = DATA
    cmd = ["docker", "run", "--rm", "--network", "none", "--cpus", "4", "--cpuset-cpus", "0-3", "--memory", "6g",
           "--memory-swap", "6g", "--pids-limit", "4096",
           "-v", f"{rd}/submission:/sub:ro", "-v", f"{D}/hidden/books:/data/hidden_books:ro",
           "-v", f"{D}/perf/books:/data/perf_books:ro", "-v", f"{D}/hidden/truth:/root/secret/hidden_truth:ro",
           "-v", f"{D}/perf/truth:/root/secret/perf_truth:ro",
           "-v", f"{D}/perf/baseline_4core.json:/root/secret/baseline_4core.json:ro",
           "-v", f"{D}/perf/baseline_1core.json:/root/secret/baseline_1core.json:ro",
           "-v", f"{rd}/grade:/out", "rb-grader:latest", "python3", "/grader/grade.py"]
    p = sh(*cmd)
    open(f"{rd}/grade/grader_stdout.txt", "w").write(p.stdout + p.stderr)
    fcntl.flock(lock, fcntl.LOCK_UN)
    try:
        return json.load(open(f"{rd}/grade/reward.json"))
    except Exception:
        return None


def n_tool_calls(rd):
    try:
        return sum(1 for _ in open(f"{rd}/tools.jsonl"))
    except FileNotFoundError:
        return 0


def process_metrics(rd):
    counts, gui, shots, bash_n, builds = {}, 0, 0, 0, 0
    try:
        for line in open(f"{rd}/tools.jsonl"):
            e = json.loads(line)
            counts[e["tool"]] = counts.get(e["tool"], 0) + 1
            gui += bool(e.get("gui"))
            if e["tool"] == "bash" and "check-public" in e["args"].get("command", ""):
                builds += 1
    except FileNotFoundError:
        pass
    return {"tool_calls": sum(counts.values()), "by_tool": counts, "gui_actions": gui, "check_public_calls": builds}


def summarize(rd):
    meta = json.load(open(f"{rd}/meta.json"))
    rew = None
    try:
        rew = json.load(open(f"{rd}/grade/reward.json"))
    except Exception:
        pass
    s = {"run": os.path.basename(rd), **meta, "process": process_metrics(rd)}
    if rew:
        s["score"] = rew.get("score"); s["hard_fail"] = rew.get("hard_fail")
        c = rew.get("correctness") or {}
        s["correctness"] = {k: c.get(k) for k in ("cells", "cells_matched", "cell_match_rate", "books_perfect",
                                                   "books_perfect_rate", "run_status", "mismatch_kinds")}
        s["performance"] = rew.get("performance")
        s["build"] = (rew.get("build") or {}).get("status")
    json.dump(s, open(f"{rd}/summary.json", "w"), indent=1)
    return s


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model"); ap.add_argument("--condition", default="full", choices=["full", "no-gui"])
    ap.add_argument("--minutes", type=int, default=120); ap.add_argument("--run-id")
    ap.add_argument("--defer-grade", action="store_true"); ap.add_argument("--grade-only")
    ap.add_argument("--max-continues", type=int, default=4); ap.add_argument("--lang", default="any", choices=list(LANG))
    args = ap.parse_args()
    if args.grade_only:
        rd = os.path.abspath(args.grade_only); grade(rd); print(json.dumps(summarize(rd), indent=1)); return
    rid = args.run_id or f"{time.strftime('%m%d-%H%M')}-{args.model.replace('claude-', '')}-{args.condition}"
    rd = f"{RB}/runs/{rid}"; os.makedirs(rd, exist_ok=True)
    c = f"rb-{rid}"
    sh("docker", "rm", "-f", c)
    r = sh("docker", "run", "-d", "--name", c, "--network", "none", "--cpus", "4", "--memory", "6g",
           "--memory-swap", "6g", "--pids-limit", "4096", "-e", f"CONDITION={args.condition}", "rb-agent-calc:latest")
    if r.returncode:
        sys.exit("container failed: " + r.stderr)
    if args.lang == "rust":
        contract = sh("docker", "exec", c, "cat", "/task/CONTRACT.md").stdout
        a = contract.index("Same toolchains as this sandbox:"); b = contract.index("No spreadsheet or formula libraries")
        subprocess.run(["docker", "exec", "-i", c, "bash", "-c", "cat > /task/CONTRACT.md"],
                       input=contract[:a] + RUST_ONLY + "\n\n" + contract[b:], text=True, check=True)
    if args.condition == "full":
        for _ in range(60):
            if sh("docker", "exec", c, "bash", "-c", 'DISPLAY=:1 xdotool search --name "LibreOffice Calc"').returncode == 0:
                break
            time.sleep(1)
        time.sleep(3)
    start = time.time(); deadline = start + args.minutes * 60
    cfg = {"mcpServers": {"sandbox": {"command": PY, "args": [f"{RB}/harness/sandbox_mcp.py"],
           "env": {"SANDBOX_CONTAINER": c, "RUN_DIR": rd, "DEADLINE": str(deadline), "CONDITION": args.condition}}}}
    json.dump(cfg, open(f"{rd}/mcp.json", "w"))
    prompt = TASK.format(minutes=args.minutes, gui=GUI[args.condition], lang=LANG[args.lang])
    open(f"{rd}/prompt.txt", "w").write(SYSTEM + "\n\n---\n\n" + prompt)
    meta = {"model": args.model, "condition": args.condition, "lang_rule": args.lang, "minutes": args.minutes, "task": "calc",
            "turns": []}
    sid, cont = None, 0
    msg = prompt
    while True:
        t0 = time.time(); calls_before = n_tool_calls(rd)
        sid, res, killed = run_claude(args, rd, msg, sid, deadline, f"{rd}/mcp.json", "claude_stream.jsonl")
        turn = {"started": round(t0 - start), "seconds": round(time.time() - t0), "killed_at_deadline": killed,
                "session_id": sid}
        if res:
            turn.update({k: res.get(k) for k in ("subtype", "is_error", "num_turns", "total_cost_usd", "usage", "result")})
            if isinstance(turn.get("result"), str):
                turn["result"] = turn["result"][-2000:]
        meta["turns"].append(turn)
        json.dump(meta, open(f"{rd}/meta.json", "w"), indent=1)
        left = (deadline - time.time()) / 60
        reply = (res or {}).get("result") or ""
        if killed or left < 8 or cont >= args.max_continues or reply.strip().rstrip(".").upper() == "DONE" or not sid:
            break
        if cont and n_tool_calls(rd) == calls_before:  # the last nudge produced no sandbox work
            break
        if (res or {}).get("is_error") and "limit" in str(reply).lower():
            meta["stopped"] = "usage limit"; break
        if str(reply).startswith("API Error"):
            meta["stopped"] = reply[:300]; break
        cont += 1
        msg = CONTINUE.format(left=int(left))
    meta["wall_minutes"] = round((time.time() - start) / 60, 1)
    meta["api_equivalent_cost_usd"] = round(sum((t.get("total_cost_usd") or 0) for t in meta["turns"]), 2)
    json.dump(meta, open(f"{rd}/meta.json", "w"), indent=1)
    collect(c, rd)
    sh("docker", "rm", "-f", c)
    if not args.defer_grade:
        grade(rd)
    print(json.dumps(summarize(rd), indent=1))


if __name__ == "__main__":
    main()
