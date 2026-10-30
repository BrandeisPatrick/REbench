"""Host-side sandbox command for subagent-driven runs. Same tools, logging and deadline as sandbox_mcp.py.

usage (via the per-run wrapper runs/<id>/sbx):
  sbx bash '<command>' [timeout_s]      run a command in the sandbox (cwd /workspace)
  sbx write <path>  < content           create/overwrite a sandbox file from stdin
  sbx read <path> [offset] [limit]      print a sandbox file with line numbers
  sbx edit <path>   < json              {"old": "...", "new": "...", "all": false}
  sbx screenshot                        save the display to a PNG on the host and print its path
  sbx click X Y [left|middle|right] [count]
  sbx type '<text>'      sbx key '<keys>'      sbx scroll X Y [up|down] [amount]      sbx drag X1 Y1 X2 Y2
"""
import json, os, sys, time
sys.argv, _argv = [sys.argv[0]], sys.argv
import importlib.util
spec = importlib.util.spec_from_file_location("sm", os.path.join(os.path.dirname(os.path.abspath(__file__)), "sandbox_mcp.py"))
sm = importlib.util.module_from_spec(spec); spec.loader.exec_module(sm)
argv = _argv[1:]


def main():
    if not argv:
        print(__doc__); return 2
    cmd, rest = argv[0], argv[1:]
    if cmd == "bash":
        print(sm.bash(rest[0], int(rest[1]) if len(rest) > 1 else 300))
    elif cmd == "write":
        print(sm.write_file(rest[0], sys.stdin.read()))
    elif cmd == "read":
        print(sm.read_file(rest[0], int(rest[1]) if len(rest) > 1 else 1, int(rest[2]) if len(rest) > 2 else 2000))
    elif cmd == "edit":
        e = json.loads(sys.stdin.read())
        print(sm.edit_file(rest[0], e["old"], e["new"], bool(e.get("all"))))
    elif cmd == "screenshot":
        if not sm.GUI:
            print("no display in this condition"); return 1
        sm.screenshot()
        if sm.expired():
            print(sm.TIME_UP); return 0
        print(os.path.join(sm.SHOTS, f"{sm._shot_n[0]:05d}.png"), sm.left())
    elif cmd in ("click", "type", "key", "scroll", "drag"):
        if not sm.GUI:
            print("no display in this condition"); return 1
        if cmd == "click":
            print(sm.click(int(rest[0]), int(rest[1]), rest[2] if len(rest) > 2 else "left", int(rest[3]) if len(rest) > 3 else 1))
        elif cmd == "type":
            print(sm.type_text(rest[0]))
        elif cmd == "key":
            print(sm.key(rest[0]))
        elif cmd == "scroll":
            print(sm.scroll(int(rest[0]), int(rest[1]), rest[2] if len(rest) > 2 else "down", int(rest[3]) if len(rest) > 3 else 3))
        else:
            print(sm.drag(*map(int, rest[:4])))
    else:
        print(__doc__); return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
