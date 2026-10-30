"""Prepare a subagent-driven run: start the sandbox container and write the agent definition.

python harness/prepare_run.py --run-id ID --model opus|sonnet|haiku|fable --condition full|no-gui --minutes N
Writes /Users/patrickli/Documents/vibe/.claude/agents/<agent-name>.md whose ONLY tools are the sandbox
MCP tools (no host shell, no host files), plus runs/<ID>/{meta.json,prompt.txt,mcp.json}.
The deadline starts when this script finishes; the MCP server refuses every tool call after it.
"""
import argparse, json, os, subprocess, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from run import RB, PY, SYSTEM, TASK, GUI, sh

AGENTS_DIR = "/Users/patrickli/Documents/vibe/.claude/agents"
TOOLS = ["bash", "read_file", "write_file", "edit_file"]
GUI_TOOLS = ["screenshot", "click", "type_text", "key", "scroll", "drag"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run-id", required=True); ap.add_argument("--model", required=True)
    ap.add_argument("--condition", default="full", choices=["full", "no-gui"]); ap.add_argument("--minutes", type=int, required=True)
    a = ap.parse_args()
    rd = f"{RB}/runs/{a.run_id}"; os.makedirs(rd, exist_ok=True)
    c = f"rb-{a.run_id}"
    sh("docker", "rm", "-f", c)
    r = sh("docker", "run", "-d", "--name", c, "--network", "none", "--cpus", "4", "--memory", "6g", "--memory-swap", "6g",
           "--pids-limit", "4096", "-e", f"CONDITION={a.condition}", "rb-agent-calc:latest")
    if r.returncode:
        sys.exit("container failed: " + r.stderr)
    if a.condition == "full":
        for _ in range(60):
            if sh("docker", "exec", c, "bash", "-c", 'DISPLAY=:1 xdotool search --name "LibreOffice Calc"').returncode == 0:
                break
            time.sleep(1)
        time.sleep(3)
    deadline = time.time() + a.minutes * 60
    env = {"SANDBOX_CONTAINER": c, "RUN_DIR": rd, "DEADLINE": str(deadline), "CONDITION": a.condition}
    sbx = f"{rd}/sbx"
    open(sbx, "w").write("#!/bin/bash\n" + " ".join(f"{k}='{v}'" for k, v in env.items()) +
                         f" exec {PY} {RB}/harness/sbx.py \"$@\"\n")
    os.chmod(sbx, 0o755)
    name = sbx
    prompt = TASK.format(minutes=a.minutes, gui=GUI[a.condition])
    open(f"{rd}/prompt.txt", "w").write(prompt)
    json.dump({"model": a.model, "condition": a.condition, "minutes": a.minutes, "task": "calc", "driver": "claude-code-subagent+sbx",
               "agent_name": name, "container": c, "deadline": deadline, "started": time.time()}, open(f"{rd}/meta.json", "w"), indent=1)
    print(json.dumps({"agent": name, "container": c, "deadline_local": time.strftime("%H:%M:%S", time.localtime(deadline))}))


if __name__ == "__main__":
    main()
