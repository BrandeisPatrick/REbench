"""Build the pilot results page from the grader's reward.json files."""
import json, math, os, sys
RB = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = sys.argv[1]
RUNS = [("pilot-opus-full", "Opus 5.5", "C++"), ("pilot-sonnet-full", "Sonnet 5.5", "Go"),
        ("pilot-fable-full", "Fable 5.1", "Go"), ("pilot-haiku-full", "Haiku 5.5", "Python")]
WL = [("perf_chain", "Dependency chain", "60k formulas, 20k deep"), ("perf_mixed", "Mixed functions", "10k formulas"),
      ("perf_lookup", "Lookups", "5k lookups over 20k rows"), ("perf_aggregate", "Conditional sums", "1k SUMIFS over 50k rows")]
AUDIT = {"pilot-opus-full": "2 benign: a stray <code>cd</code> into a sandbox-only path that failed, and a read of its own timed-out command's output.",
         "pilot-sonnet-full": "4: two early read-only commands typed on the host that failed (sandbox paths do not exist there), one no-op, and one read of its own timed-out command's output.",
         "pilot-fable-full": "None.",
         "pilot-haiku-full": "1: a malformed command the host shell rejected before running anything."}
seeds = open(f"{RB}/data/calc/evalsets/pilot2/seeds.txt").read().split()
data = []
for rid, name, lang in RUNS:
    g = json.load(open(f"{RB}/runs/{rid}/grade-pilot2/reward.json")); m = json.load(open(f"{RB}/runs/{rid}/meta.json"))
    data.append((rid, name, lang, g, m))
base = data[0][3]["performance"]

def pos(v):  # log2 scale from 1/32 to 128
    return max(0.0, min(100.0, (math.log2(max(v, 1 / 32)) + 5) / 12 * 100))

P0 = pos(1.0); CAP = pos(8.0)
rows = []
for rid, name, lang, g, m in data:
    c, s = g["correctness"], g["score"]
    gate = '<span class="pill ok">pass</span>' if s["resolved"] else '<span class="pill bad">fail</span>'
    rows.append(f"<tr><td><strong>{name}</strong></td><td>{lang}</td><td class='num'>{m['wall_minutes']:.0f} of 60</td>"
                f"<td class='num'>{c['cell_match_rate']:.2%}</td><td class='num'>{c['books_perfect']}/{c['books']}</td><td>{gate}</td>"
                f"<td class='num'>{s['E']:.1f}x</td><td class='num'><strong>{s['task_score']:.3f}</strong></td></tr>")
perf = []
for key, label, desc in WL:
    lo = base[key]["baseline_4core_s"]
    bars = []
    for rid, name, lang, g, m in data:
        e = g["performance"][key]; v = e.get("speedup_vs_lo", 0); ok = e["match_rate"] >= 0.999
        a, b = sorted((P0, pos(v)))
        cls = "fast" if v >= 1 else "slow"
        bars.append(f"<div class='bar-row'><span class='bar-name'>{name}</span><div class='track'><span class='parity' style='left:{P0:.2f}%'></span>"
                    f"<span class='cap' style='left:{CAP:.2f}%'></span><span class='bar {cls}' style='left:{a:.2f}%;width:{max(b - a, 0.6):.2f}%'></span></div>"
                    f"<span class='bar-val'>{v:.2f}x{'' if ok else ' <em>below 99.9%</em>'}</span></div>")
    perf.append(f"<div class='wl'><div class='wl-head'><h3>{label}</h3><span class='muted'>{desc} · LibreOffice {lo:.2f} s</span></div>{''.join(bars)}</div>")
audit = "".join(f"<li><strong>{name}.</strong> {AUDIT[rid]}</li>" for rid, name, lang, g, m in data)
html = f"""<title>Rebuild Bench Pilot</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,500;12..96,700&family=Source+Sans+3:wght@400;600&family=JetBrains+Mono:wght@400;500&display=swap">
<style>
/* Layout: one reading column; leaderboard first, then speed by workload on a log scale, then findings, integrity and decisions. */
:root{{--bg:#f3f5f8;--surface:#fff;--fg:#17202a;--muted:#5b6775;--line:#d8dee6;--soft:#e9eef4;--accent:#0b6e8f;
--good:#1f8a4c;--good-bg:#e3f4ea;--bad:#b3362a;--bad-bg:#fbe3e0;--slow:#c26a1b;
--display:"Bricolage Grotesque","Avenir Next",Avenir,"Helvetica Neue",Arial,sans-serif;--body:"Source Sans 3","Helvetica Neue",Arial,sans-serif;--mono:"JetBrains Mono",ui-monospace,Menlo,monospace}}
@media (prefers-color-scheme:dark){{:root:not([data-theme="light"]){{--bg:#0f1419;--surface:#161c24;--fg:#e4e9ef;--muted:#97a3b1;--line:#2a333e;--soft:#1d252f;--accent:#62bbd8;
--good:#5ccf93;--good-bg:#15301f;--bad:#f07a6e;--bad-bg:#3a1a16;--slow:#e89a55;color-scheme:dark}}}}
:root[data-theme="dark"]{{--bg:#0f1419;--surface:#161c24;--fg:#e4e9ef;--muted:#97a3b1;--line:#2a333e;--soft:#1d252f;--accent:#62bbd8;
--good:#5ccf93;--good-bg:#15301f;--bad:#f07a6e;--bad-bg:#3a1a16;--slow:#e89a55;color-scheme:dark}}
*{{box-sizing:border-box}}body{{background:var(--bg);color:var(--fg);font-family:var(--body);font-size:16px;line-height:1.5;margin:0}}
.wrap{{max-width:960px;margin:0 auto;padding-block:32px 64px;padding-inline:20px}}
h1,h2,h3{{font-family:var(--display);text-wrap:balance;margin:0;line-height:1.15}}h1{{font-size:clamp(28px,4.5vw,40px)}}
h2{{font-size:22px;margin-top:44px;padding-top:20px;border-top:1px solid var(--line)}}h3{{font-size:16px}}
p,li{{max-width:70ch}}.eyebrow{{font-family:var(--mono);font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}}
.lede{{font-size:18px;color:var(--muted)}}.muted{{color:var(--muted);font-size:14px}}
.tablewrap{{overflow-x:auto;margin-top:14px;border:1px solid var(--line);border-radius:10px;background:var(--surface)}}
table{{border-collapse:collapse;width:100%;min-width:640px;font-size:15px}}th,td{{text-align:left;padding:10px 12px;border-top:1px solid var(--line)}}
thead th{{border-top:0;background:var(--soft);font-family:var(--mono);font-size:12px;letter-spacing:.05em;text-transform:uppercase;color:var(--muted);font-weight:500}}
td.num,th.num{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}}
.pill{{font-family:var(--mono);font-size:12px;padding:2px 8px;border-radius:999px}}.ok{{background:var(--good-bg);color:var(--good)}}.bad{{background:var(--bad-bg);color:var(--bad)}}
.wls{{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,420px),1fr));gap:14px;margin-top:14px}}
.wl{{background:var(--surface);border:1px solid var(--line);border-radius:10px;padding:14px 16px;min-width:0}}
.wl-head{{display:flex;flex-wrap:wrap;gap:4px 10px;align-items:baseline;margin-bottom:10px}}
.bar-row{{display:grid;grid-template-columns:84px 1fr 104px;gap:10px;align-items:center;margin:7px 0;font-size:14px}}
.bar-name{{color:var(--muted)}}.bar-val{{font-family:var(--mono);font-size:13px;text-align:right;font-variant-numeric:tabular-nums}}.bar-val em{{display:block;font-style:normal;color:var(--bad);font-size:11px}}
.track{{position:relative;height:14px;background:var(--soft);border-radius:3px}}
.bar{{position:absolute;top:2px;height:10px;border-radius:2px}}.bar.fast{{background:var(--accent)}}.bar.slow{{background:var(--slow)}}
.parity{{position:absolute;top:-3px;bottom:-3px;width:2px;background:var(--fg);opacity:.55}}.cap{{position:absolute;top:-3px;bottom:-3px;border-left:2px dashed var(--bad);opacity:.7}}
.legend{{display:flex;flex-wrap:wrap;gap:6px 18px;font-size:13px;color:var(--muted);margin-top:10px}}.legend i{{display:inline-block;width:14px;height:0;vertical-align:middle;margin-right:6px}}
.findings{{padding-left:20px}}.findings li{{margin:10px 0}}code{{font-family:var(--mono);font-size:.9em}}
.note{{border-left:3px solid var(--accent);padding:8px 14px;background:var(--surface);border-radius:0 8px 8px 0;max-width:74ch}}
</style>
<div class="wrap">
<div class="eyebrow">rebuild-bench · calc task · pilot round · evaluation set pilot2 ({seeds[0]}, {seeds[1]})</div>
<h1>Three of four Claude models rebuilt a spreadsheet engine that beats LibreOffice</h1>
<p class="lede">Each model had 60 minutes to rebuild LibreOffice Calc's formula engine from scratch. Opus, Sonnet and Fable passed the 99.9% correctness gate and ran 10 to 18 times faster than LibreOffice. Haiku fell just short of the gate.</p>
<h2>Leaderboard</h2>
<div class="tablewrap"><table><thead><tr><th>Model</th><th>Language</th><th class="num">Minutes used</th><th class="num">Hidden cells correct</th><th class="num">Books fully correct</th><th>Gate</th><th class="num">Speedup</th><th class="num">Score</th></tr></thead><tbody>{''.join(rows)}</tbody></table></div>
<p class="muted">9,761 hidden formula cells in 160 workbooks. The gate needs 99.9% of cells and 95% of books. Speedup is the geometric mean over the four workloads below, measured against LibreOffice in a warm process. Score is 0.5 at parity and 1.0 at 8x or more.</p>
<h2>Speed by workload</h2>
<div class="wls">{''.join(perf)}</div>
<div class="legend"><span><i style="border-top:2px solid var(--fg)"></i>parity with LibreOffice</span><span><i style="border-top:2px dashed var(--bad)"></i>8x, where the score maxes out</span><span>log scale from 1/32x to 128x</span></div>
<h2>What the pilot shows</h2>
<ul class="findings">
<li><strong>The speed score saturates.</strong> Three models score a perfect 1.000 because their speedups pass the 8x cap, so the score cannot rank them. Raising the cap or scoring on a wider log range would separate Opus and Sonnet at 18x from Fable at 10x.</li>
<li><strong>The dependency chain is the only hard workload.</strong> LibreOffice is slow at conditional sums and lookups, so every compiled rebuild beats it by about 25x to 99x there. The 60k-formula chain is where the models differ, from 1.5x to 3.5x.</li>
<li><strong>Language choice decided Haiku's result.</strong> Haiku wrote pure Python. It was nearly as correct as the others but about 28 times slower than LibreOffice on the chain. The other three picked Go or C++ without being told to.</li>
<li><strong>No model touched the GUI.</strong> All four explored LibreOffice through the command-line helpers, so this round did not test computer use. A condition with the GUI as the only way to query LibreOffice would.</li>
<li><strong>Public scores predicted hidden scores.</strong> Every model reported 99.96% to 100% on the 40 public books and landed between 99.86% and 99.97% on the 160 hidden ones.</li>
<li><strong>All four quit early.</strong> They stopped after 27 to 51 minutes and declined most of the extra time they were offered.</li>
<li><strong>The remaining errors are deep edge cases.</strong> All four return #DIV/0! on the same cell where LibreOffice reports #NUM!, and most other misses involve which error wins when two collide, MAX and MEDIAN on mixed types, or text-to-number rules.</li>
</ul>
<h2>How fair this round was</h2>
<p>The agents ran as subagents of a Claude Code session because the standalone CLI was logged out. They were limited by instruction to one command that runs everything inside a sealed, offline container, and their transcripts were audited afterwards. Answers could not leak: the evaluation set was generated with fresh random seeds after the last run ended. Submissions were built and run as an unprivileged user in a separate container with no LibreOffice and no network. Each one was timed alone on 4 pinned cores.</p>
<ul>{audit}</ul>
<div class="note">These are pilot results from one run per model. The sealed harness, which gives the model only the sandbox tools, is built and tested but needs <code>claude auth login</code> once. Kimi needs its Moonshot balance topped up.</div>
<h2>Decisions for the next round</h2>
<ul>
<li><strong>Widen the speed score</strong> so 10x and 18x score differently, for example capping at 64x.</li>
<li><strong>Add harder speed workloads</strong> where LibreOffice is already fast, such as longer chains and wide recalculation after single-cell edits.</li>
<li><strong>Add a GUI-only condition</strong> to actually test computer use.</li>
<li><strong>Run three seeds per model</strong> so differences of a few cells are not noise.</li>
<li><strong>Build the darktable task</strong> as the visual counterpart.</li>
</ul>
</div>"""
open(OUT, "w").write(html); print("wrote", OUT, len(html), "bytes")
