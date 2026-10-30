"""Print the full prompt for a subagent-driven run: tool rules + sbx usage + the task."""
import json, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from run import RB, SYSTEM, TASK

rd = f"{RB}/runs/{sys.argv[1]}"
m = json.load(open(f"{rd}/meta.json"))
sbx = f"{rd}/sbx"
gui = {"full": ("LibreOffice Calc is open on a 1280x800 virtual display. Operate it with `sbx screenshot` (then view the "
                "printed PNG path with your Read tool), `sbx click`, `sbx type`, `sbx key`, `sbx scroll` and `sbx drag`. "
                "The lo-recalc and xlsx-values commands query LibreOffice from the sandbox shell."),
       "no-gui": "There is no display. Query LibreOffice from the sandbox shell with the lo-recalc and xlsx-values commands."}
print(f"""{SYSTEM}

You are the test subject in a benchmark run. Strict rules:
- You operate a sandboxed Linux computer ONLY through the command `{sbx}` (called `sbx` below), which you run with
  your Bash tool. Every Bash call you make must be an invocation of sbx, optionally with a heredoc piped into it.
- Your Read tool may be used ONLY to view the PNG paths that `sbx screenshot` prints.
- Do not run any other command on this host, do not read or write any other host file, do not use web search or web
  fetch, and do not use any other tool. The host is not part of the task; everything you need is in the sandbox.

sbx usage:
  sbx bash '<command>' [timeout_seconds]   run a command in the sandbox, cwd /workspace (default timeout 300, max 1800)
  sbx write <sandbox_path> <<'EOF'         create or overwrite a sandbox file with the heredoc content
  sbx read <sandbox_path> [offset] [limit]   print a sandbox file with line numbers
  sbx edit <sandbox_path> <<'EOF'          replace text; stdin is JSON {{"old": "...", "new": "...", "all": false}}
  sbx screenshot                           save the display as a PNG and print its path
  sbx click X Y [left|middle|right] [count]   sbx type '<text>'   sbx key '<keys, e.g. ctrl+s Return>'
  sbx scroll X Y [up|down] [amount]        sbx drag X1 Y1 X2 Y2

{TASK.format(minutes=m['minutes'], gui=gui[m['condition']])}""")
