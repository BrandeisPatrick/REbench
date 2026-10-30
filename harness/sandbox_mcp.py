"""MCP server exposing the agent's tools. Every tool executes inside the sandbox container.

env: SANDBOX_CONTAINER, RUN_DIR, DEADLINE (unix seconds), CONDITION (full | no-gui)
"""
import json, os, subprocess, time
from mcp.server.fastmcp import FastMCP, Image

C = os.environ["SANDBOX_CONTAINER"]
RUN_DIR = os.environ["RUN_DIR"]
DEADLINE = float(os.environ["DEADLINE"])
GUI = os.environ.get("CONDITION", "full") != "no-gui"
SHOTS = os.path.join(RUN_DIR, "screenshots")
os.makedirs(SHOTS, exist_ok=True)
mcp = FastMCP("sandbox")
_log = open(os.path.join(RUN_DIR, "tools.jsonl"), "a", buffering=1)
_shot_n = [len(os.listdir(SHOTS))]
MAX_OUT = 30000


def log(tool, args, out_len, t0, **extra):
    _log.write(json.dumps({"ts": round(time.time(), 2), "tool": tool, "args": args, "out_len": out_len,
                           "sec": round(time.time() - t0, 2), **extra}) + "\n")


TIME_UP = ("[harness] Time is up. The run is over and the sandbox is closed, so this tool call was not "
           "executed. Stop working and end your turn with a short summary.")


def expired():
    return time.time() > DEADLINE


def left():
    m = max(0, (DEADLINE - time.time()) / 60)
    return f"[time left: {m:.0f} min]"


def trunc(s):
    if len(s) <= MAX_OUT:
        return s
    h = MAX_OUT // 2
    return s[:h] + f"\n... [{len(s) - MAX_OUT} characters truncated] ...\n" + s[-h:]


def dexec(argv, timeout, data=None):
    return subprocess.run(["docker", "exec", "-i", "-w", "/workspace", C] + argv, input=data,
                          capture_output=True, timeout=timeout)


def short(a, n=300):
    s = a if isinstance(a, str) else json.dumps(a)
    return s if len(s) <= n else s[:n] + "..."


@mcp.tool()
def bash(command: str, timeout: int = 300) -> str:
    """Run a bash command inside the sandbox (working directory /workspace). Returns combined
    stdout/stderr and the exit code. timeout is in seconds (max 1800); the command is killed after it."""
    if expired():
        return TIME_UP
    t0 = time.time()
    t = max(1, min(int(timeout), 1800))
    try:
        p = dexec(["timeout", "-k", "5", str(t), "bash", "-lc", command], t + 30)
        out = (p.stdout + p.stderr).decode(errors="replace")
        code = p.returncode
    except subprocess.TimeoutExpired:
        out, code = "", 124
    res = trunc(out) + f"\n[exit code {code}{' (timed out)' if code == 124 else ''}] {left()}"
    log("bash", {"command": short(command)}, len(out), t0, exit=code)
    return res


@mcp.tool()
def read_file(path: str, offset: int = 1, limit: int = 2000) -> str:
    """Read a text file from the sandbox with line numbers. offset is the first line (1-based)."""
    if expired():
        return TIME_UP
    t0 = time.time()
    p = dexec(["awk", "-v", f"s={offset}", "-v", f"e={offset + limit - 1}",
               'NR>=s && NR<=e {printf "%6d\\t%s\\n", NR, $0} NR>e {exit}', path], 60)
    out = (p.stdout + p.stderr).decode(errors="replace")
    log("read_file", {"path": path}, len(out), t0)
    return trunc(out) or "(empty)"


@mcp.tool()
def write_file(path: str, content: str) -> str:
    """Create or overwrite a file in the sandbox with the given content (parent directories are created)."""
    if expired():
        return TIME_UP
    t0 = time.time()
    p = dexec(["bash", "-c", 'mkdir -p "$(dirname "$1")" && cat > "$1"', "_", path], 60, content.encode())
    log("write_file", {"path": path, "bytes": len(content)}, 0, t0)
    return (f"wrote {len(content)} bytes to {path}" if p.returncode == 0 else p.stderr.decode()) + f" {left()}"


@mcp.tool()
def edit_file(path: str, old_string: str, new_string: str, replace_all: bool = False) -> str:
    """Replace old_string with new_string in a sandbox file. old_string must match exactly and be
    unique unless replace_all is true."""
    if expired():
        return TIME_UP
    t0 = time.time()
    p = dexec(["cat", path], 60)
    if p.returncode != 0:
        return p.stderr.decode()
    s = p.stdout.decode(errors="surrogateescape")
    n = s.count(old_string)
    if n == 0:
        return "old_string not found; no change made"
    if n > 1 and not replace_all:
        return f"old_string occurs {n} times; make it unique or set replace_all"
    s = s.replace(old_string, new_string) if replace_all else s.replace(old_string, new_string, 1)
    q = dexec(["bash", "-c", 'cat > "$1"', "_", path], 60, s.encode(errors="surrogateescape"))
    log("edit_file", {"path": path, "replacements": n if replace_all else 1}, 0, t0)
    return (f"edited {path} ({n if replace_all else 1} replacement)" if q.returncode == 0 else q.stderr.decode()) + f" {left()}"


if GUI:
    def xdo(args, t0, name, logargs):
        if expired():
            return TIME_UP
        p = dexec(["env", "DISPLAY=:1", "xdotool"] + args, 30)
        log(name, logargs, 0, t0, gui=True)
        return ("ok" if p.returncode == 0 else p.stderr.decode()) + f" {left()}"

    @mcp.tool()
    def screenshot() -> Image:
        """Capture the 1280x800 virtual display showing LibreOffice Calc."""
        t0 = time.time()
        if expired():
            p = dexec(["bash", "-c", "DISPLAY=:1 import -window root -resize 8x5 png:-"], 60)
            return Image(data=p.stdout, format="png")
        p = dexec(["bash", "-c", "DISPLAY=:1 import -window root png:-"], 60)
        _shot_n[0] += 1
        with open(os.path.join(SHOTS, f"{_shot_n[0]:05d}.png"), "wb") as f:
            f.write(p.stdout)
        log("screenshot", {}, len(p.stdout), t0, gui=True)
        return Image(data=p.stdout, format="png")

    @mcp.tool()
    def click(x: int, y: int, button: str = "left", count: int = 1) -> str:
        """Move the mouse to (x, y) on the display and click. button: left | middle | right. count 2 = double-click."""
        b = {"left": "1", "middle": "2", "right": "3"}.get(button, "1")
        return xdo(["mousemove", str(x), str(y), "click", "--repeat", str(max(1, count)), "--delay", "80", b],
                   time.time(), "click", {"x": x, "y": y, "button": button, "count": count})

    @mcp.tool()
    def type_text(text: str) -> str:
        """Type text with the keyboard into the focused window. Use key() for Enter, Tab, shortcuts."""
        return xdo(["type", "--delay", "12", "--", text], time.time(), "type_text", {"text": short(text, 120)})

    @mcp.tool()
    def key(keys: str) -> str:
        """Press keys, xdotool syntax, space-separated: e.g. "Return", "ctrl+s", "Escape", "Down Down Return"."""
        return xdo(["key", "--delay", "60", "--"] + keys.split(), time.time(), "key", {"keys": keys})

    @mcp.tool()
    def scroll(x: int, y: int, direction: str = "down", amount: int = 3) -> str:
        """Scroll the mouse wheel at (x, y). direction: up | down | left | right."""
        b = {"up": "4", "down": "5", "left": "6", "right": "7"}.get(direction, "5")
        return xdo(["mousemove", str(x), str(y), "click", "--repeat", str(max(1, amount)), b],
                   time.time(), "scroll", {"x": x, "y": y, "direction": direction, "amount": amount})

    @mcp.tool()
    def drag(x1: int, y1: int, x2: int, y2: int) -> str:
        """Press the left button at (x1, y1), move to (x2, y2) and release."""
        return xdo(["mousemove", str(x1), str(y1), "mousedown", "1", "sleep", "0.2", "mousemove", str(x2), str(y2),
                    "sleep", "0.2", "mouseup", "1"], time.time(), "drag", {"from": [x1, y1], "to": [x2, y2]})


if __name__ == "__main__":
    mcp.run()
