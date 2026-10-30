"""Build the rebuild-bench analysis page from the run folders: grades, submission sources and transcripts.

python harness/results_page.py <out.html>
Prints the figures the prose quotes, so a regrade that moves them is easy to spot.
"""
import html, json, math, os, re, statistics, sys
from collections import Counter, defaultdict

RB = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = sys.argv[1]
MODELS = [("opus", "Opus 5.5"), ("sonnet", "Sonnet 5.5"), ("fable", "Fable 5.1"), ("haiku", "Haiku 5.5")]
ROUNDS = [("pilot", "Pilot"), ("sealed", "Sealed round"), ("rust", "Rust round")]
SETS = ["sealed1", "rust1"]
EV = "rust1"  # generated after all twelve runs ended, so it is the main test set here
WL = [("perf_chain", "Chain"), ("perf_mixed", "Mixed"), ("perf_lookup", "Lookups"), ("perf_aggregate", "Conditional sums")]
NAME, SHORT, RNAME = dict(MODELS), {m: n.split()[0] for m, n in MODELS}, dict(ROUNDS)
RUNS = [(k, m) for m, _ in MODELS for k, _ in ROUNDS]
esc = lambda s: html.escape(str(s), quote=True)


def rd(k, m):
    return f"{RB}/runs/{k}-{m}-full"


def sources(k, m):
    return [os.path.join(r, f) for r, _, ff in os.walk(f"{rd(k, m)}/submission") for f in ff if "/target" not in r]


def lang(k, m):
    fs = sources(k, m)
    return ("Rust" if any(f.endswith(".rs") for f in fs) else "Go" if any(f.endswith(".go") for f in fs)
            else "C++" if any(f.endswith((".cc", ".cpp")) for f in fs) else "Python")


def loc(k, m):
    return sum(sum(1 for _ in open(f, errors="ignore")) for f in sources(k, m) if f.endswith((".rs", ".go", ".cpp", ".cc", ".h", ".hpp", ".py")))


PROBE = re.compile(r"(?<![\w/.-])(?:/task/bin/)?lo-recalc\s+(?!--help)[^\s|;&]")


def probes(k, m):
    """Tool calls that ran LibreOffice (lo-recalc) on a workbook, directly or from a script."""
    n = 0
    for line in open(f"{rd(k, m)}/tools.jsonl"):
        s = json.dumps(json.loads(line).get("args")).replace('\\"', '"')
        n += bool(PROBE.search(s) or '"lo-recalc"' in s)
    return n


G = {(k, m, ev): json.load(open(f"{rd(k, m)}/grade-{ev}/reward.json")) for k, m in RUNS for ev in SETS}
META = {(k, m): json.load(open(f"{rd(k, m)}/meta.json")) for k, m in RUNS}
LANG = {r: lang(*r) for r in RUNS}
BASE = json.load(open(f"{RB}/data/calc/evalsets/{EV}/baseline_4core.json"))
E = lambda k, m, ev=EV: G[(k, m, ev)]["score"]["E"]
PASS = lambda k, m, ev=EV: G[(k, m, ev)]["score"]["resolved"] and not G[(k, m, ev)].get("hard_fail")
SPEED = lambda k, m, w, ev=EV: G[(k, m, ev)]["performance"][w]["speedup_vs_lo"]
OKPILL, BADPILL = "<span class='pill ok'>pass</span>", "<span class='pill bad'>fail</span>"


def fx(v):
    return f"{v:.2f}x" if v < 1 else f"{v:.1f}x" if v < 99.5 else f"{v:.0f}x"


def logpos(v, lo, hi):
    return max(0.0, min(100.0, (math.log2(max(v, 2 ** lo)) - lo) / (hi - lo) * 100))


def log_axis(lo, hi, minor):
    t = "".join(f"<span class='{'minor' if k in minor else ''}' style='left:{(k - lo) / (hi - lo) * 100:.2f}%'>"
                f"{'1/' + str(2 ** -k) if k < 0 else str(2 ** k)}x</span>" for k in range(lo, hi + 1))
    return f"<div class='row axis-row'><span></span><div class='axis'>{t}</div><span></span></div>"


# ---- 1. Three attempts per model on rust1 ----
LO1, HI1 = -1, 6
p1 = lambda v: logpos(v, LO1, HI1)
GUIDE1 = f"<span class='parity' style='left:{p1(1):.2f}%'></span><span class='cap' style='left:{p1(8):.2f}%'></span>"
MED = {m: statistics.median(E(k, m) for k, _ in ROUNDS) for m, _ in MODELS}
MED_S = {m: statistics.median(E(k, m, "sealed1") for k, _ in ROUNDS) for m, _ in MODELS}
rank = []
for m, name in MODELS:
    xs = [p1(E(k, m)) for k, _ in ROUNDS]
    dots = "".join(f"<span class='dot{'' if PASS(k, m) else ' miss'}' style='left:{p1(E(k, m)):.2f}%' data-tip=\""
                   f"{esc(f'{name}, {RNAME[k].lower()} ({LANG[(k, m)]}): {fx(E(k, m))}' + ('' if PASS(k, m) else ', missed the correctness gate'))}\"></span>"
                   for k, _ in ROUNDS)
    rank.append(f"<div class='row'><span class='name'>{SHORT[m]}</span><div class='track'>{GUIDE1}"
                f"<span class='range' style='left:{min(xs):.2f}%;width:{max(xs) - min(xs):.2f}%'></span>"
                f"<span class='med' style='left:{p1(MED[m]):.2f}%' data-tip=\"{esc(f'{name}, median of three attempts: {fx(MED[m])}')}\"></span>"
                f"{dots}</div><span class='val'>{fx(MED[m])}</span></div>")
WIN = {(a, b): sum(E(k1, a) > E(k2, b) for k1, _ in ROUNDS for k2, _ in ROUNDS) for i, (a, _) in enumerate(MODELS) for b, _ in MODELS[i + 1:]}
SPREAD = max(max(E(k, m) for k, _ in ROUNDS) / min(E(k, m) for k, _ in ROUNDS) for m, _ in MODELS[:3])


# ---- 2. What each logged run did after its engine first passed the public check ----
def trajectory(k, m):
    """Minute the public check first passed the gate, and (minute, geometric-mean speed) of every public speed check."""
    first, perf = None, []
    for line in open(f"{rd(k, m)}/claude_stream.jsonl"):
        e = json.loads(line)
        content = (e.get("message") or {}).get("content")
        if e.get("type") != "user" or not isinstance(content, list):
            continue
        for b in content:
            if b.get("type") != "tool_result":
                continue
            c = b["content"]
            txt = (c if isinstance(c, str) else " ".join(x.get("text", "") for x in c if isinstance(x, dict))).replace("\\n", "\n")
            left = re.findall(r"time left: (\d+) min", txt)
            if not left:
                continue
            t = 60 - int(left[-1])
            for cells, books, total in re.findall(r"cells correct: \d+/\d+ = ([\d.]+)%\s+workbooks fully correct: (\d+)/(\d+)", txt):
                if first is None and float(cells) >= 99.9 and int(books) >= 0.95 * int(total):
                    first = t
            sp = re.findall(r"perf perf_\w+: ok, correct \d+/\d+, [\d.]+s vs LibreOffice [\d.]+s -> ([\d.]+)x", txt)
            if len(sp) == 4:
                perf.append((t, math.exp(sum(math.log(float(x)) for x in sp) / 4)))
    return first, perf


TL = {}
for m, _ in MODELS:
    for k in ("sealed", "rust"):
        first, perf = trajectory(k, m)
        TL[(k, m)] = {"pass": first, "wall": META[(k, m)]["wall_minutes"], "e0": next(v for t, v in perf if t >= first), "e1": perf[-1][1]}
timeline = []
for (k, m), d in TL.items():
    a, w = d["pass"] / 60 * 100, d["wall"] / 60 * 100
    who = f"{NAME[m]}, {RNAME[k].lower()}"
    tip1 = f"{who}: building until the public check first passed, at minute {d['pass']}"
    tip2 = f"{who}: kept working until minute {d['wall']:.0f}; its own speed check went from {fx(d['e0'])} to {fx(d['e1'])}"
    timeline.append(
        f"<div class='row tl'><span class='name'>{SHORT[m]}<small>{'sealed' if k == 'sealed' else 'Rust'}</small></span><div class='track'>"
        f"<span class='seg build' style='width:{a:.2f}%' data-tip=\"{esc(tip1)}\"></span>"
        f"<span class='seg after' style='left:calc({a:.2f}% + 2px);width:calc({w - a:.2f}% - 2px)' data-tip=\"{esc(tip2)}\"></span>"
        f"</div><span class='val'>{fx(d['e0'])} → {fx(d['e1'])}</span></div>")
TAXIS = ("<div class='row tl axis-row'><span></span><div class='axis'>"
         + "".join(f"<span class='{'minor' if t % 20 else ''}' style='left:{t / 60 * 100:.2f}%'>{t}{' min' if t == 60 else ''}</span>"
                   for t in range(0, 61, 10)) + "</div><span></span></div>")
STRONG = [d for (k, m), d in TL.items() if m != "haiku"]
HAIKU = [d for (k, m), d in TL.items() if m == "haiku"]
UNUSED = [60 - META[r]["wall_minutes"] for r in RUNS]


# ---- 3. Every wrong cell, sorted by the LibreOffice rule behind it ----
CATS = [("err", "Which error code to report", "Mostly formulas where two errors meet, such as #N/A divided by zero."),
        ("num", "How a number is printed as text", "LibreOffice prints 15 significant digits, and its last digit sometimes rounds differently."),
        ("crit", "Criteria built from an empty cell", "With A1 empty, \">\"&A1 makes LibreOffice match every text cell and no numbers."),
        ("coerce", "Text or TRUE/FALSE where a number is expected", "Whether LibreOffice skips the value, converts it or returns #VALUE!."),
        ("bool", "MIN over TRUE/FALSE cells", "When the smallest value comes from a TRUE/FALSE cell, LibreOffice returns TRUE or FALSE."),
        ("bug", "Ordinary bugs in one engine", "Five in Opus's Rust engine (ISLOGICAL) and one in Sonnet's sealed engine (COUNT).")]
CASCADE = {("rust1", "book_144659_0122", "F24"): "crit"}  # MOD(IFS(..., E28, ...)) is wrong only through E28's COUNTIFS


def category(ev, x):
    e, g = x["expected"], x["got"]
    if (ev, x["book"], x["ref"]) in CASCADE:
        return CASCADE[(ev, x["book"], x["ref"])]
    if e["t"] == g["t"] == "e":
        return "err"
    if e["t"] == g["t"] == "s":
        return "num"
    if {e["t"], g["t"]} == {"b", "n"}:
        return "bool"
    if "e" in (e["t"], g["t"]):
        return "coerce"
    return "crit" if '"&' in x["formula"] else "bug"


MISS, WHO = [], defaultdict(set)
for ev in SETS:
    for k, m in RUNS:
        sample = json.load(open(f"{rd(k, m)}/grade-{ev}/mismatch_sample.json"))
        c = G[(k, m, ev)]["correctness"]
        assert len(sample) == c["cells"] - c["cells_matched"], (k, m, ev)  # the sample holds every wrong cell
        for x in sample:
            key = (ev, x["book"], x["ref"])
            MISS.append((key, category(ev, x), (k, m), x)); WHO[key].add((k, m))
SH, FEW = Counter(), Counter()
for key, cat, r, x in MISS:
    (SH if len(WHO[key]) >= 3 else FEW)[cat] += 1
TOP = max(SH[c] + FEW[c] for c, _, _ in CATS)
cats = []
for c, label, desc in CATS:
    tip = f"{label}: {SH[c] + FEW[c]} wrong cells, {SH[c]} of them on cells that 3 or more engines got wrong"
    bars = ((f"<span class='sh' style='width:{SH[c] / TOP * 100:.2f}%'></span>" if SH[c] else "")
            + (f"<span class='fw' style='width:{FEW[c] / TOP * 100:.2f}%'></span>" if FEW[c] else ""))
    cats.append(f"<div class='cat'><div class='cat-head'><strong>{label}</strong><span>{esc(desc)}</span></div>"
                f"<div class='cat-bar' data-tip=\"{esc(tip)}\"><div class='ctrack'>{bars}</div><span class='val'>{SH[c] + FEW[c]}</span></div></div>")
WIDE = sorted(((len(w), key) for key, w in WHO.items() if key[0] == EV), reverse=True)[:4]


def show(v):
    s = v["v"]
    if v["t"] == "b":
        return "TRUE" if s else "FALSE"
    if v["t"] == "n":
        return f"{s:,.2f}".rstrip("0").rstrip(".") if s != int(s) else f"{int(s):,}"
    return s if len(s) <= 24 else s[:18] + "…"


def example(book, ref, note):
    rows = [(r, x) for key, cat, r, x in MISS if key == (EV, book, ref)]
    x = rows[0][1]
    got = json.loads(Counter(json.dumps(x2["got"]) for _, x2 in rows).most_common(1)[0][0])
    n_got = sum(1 for _, x2 in rows if x2["got"] == got)
    who = f"{n_got} of 12 engines" if n_got == len(rows) else f"{n_got} of the {len(rows)} engines that miss it"
    return (f"<figure class='cell'><div class='fbar'><span class='ref'>{ref}</span><span class='fx'>fx</span>"
            f"<code>={esc(x['formula'].replace('_xlfn.', ''))}</code></div>"
            f"<div class='res'><span><small>LibreOffice</small><b>{esc(show(x['expected']))}</b></span>"
            f"<span><small>{who}</small><b>{esc(show(got))}</b></span>"
            f"<span class='where'>{EV} · {x['book'].replace('book_', 'book ')}</span></div>"
            f"<figcaption>{note}</figcaption></figure>")


EXAMPLES = "".join([
    example("book_144659_0036", "E11", "Both arguments are errors. AVERAGEIF finds no \"Meat\" among numbers, so the first is #DIV/0!, and "
            "VALUE(\"Banana\") makes the second #VALUE!. LibreOffice reports the second one."),
    example("book_144659_0134", "F19", "EXP returns a number 199 digits long. LibreOffice prints 15 significant digits and pads the rest "
            "with zeros. Nine of the ten engines that miss this cell print every digit of the exact binary value."),
    example("book_144659_0052", "E12", "B5 is empty, so the criterion is a bare \">\". LibreOffice compares it as text, every region name "
            "in column D passes, and it sums the whole column. Where the criteria column holds numbers instead, LibreOffice matches nothing."),
])


def book_misses(k, m, ev):
    c = G[(k, m, ev)]["correctness"]
    return c["cells"] - c["cells_matched"]


# ---- 4. Two data structures, read from each submission's source ----
STRUCT = {  # (exact-match lookup index, criteria cache)
    ("pilot", "opus"): ("yes", "yes"),       # src/main.cpp: numIdx, maskCache
    ("sealed", "opus"): ("yes", "yes"),      # src/funcs.rs: lookup_cache, crit_cache
    ("rust", "opus"): ("yes", "yes"),        # src/main.rs: lcache, mcache
    ("pilot", "sonnet"): ("yes", "yes"),     # fast.go: lkIdx, bitCache
    ("sealed", "sonnet"): ("no", "yes"),     # src/eval.rs: ccache only
    ("rust", "sonnet"): ("yes", "yes"),      # src/eval.rs: idx_cache, mask_cache
    ("pilot", "fable"): ("yes", "partial"),  # index.go: colIndex; agg.go: strIdx covers text values only
    ("sealed", "fable"): ("yes", "no"),      # index.go: lookupIndex; criteria rescanned across goroutines
    ("rust", "fable"): ("yes", "yes"),       # src/eval.rs: idx, crit_cache
    ("pilot", "haiku"): ("yes", "no"),       # calc.py: exact_index
    ("sealed", "haiku"): ("yes", "yes"),     # engine.py: IDXCACHE, MCACHE
    ("rust", "haiku"): ("no", "no"),
}
PILL = {"yes": "<span class='pill ok'>yes</span>", "no": "<span class='pill bad'>no</span>", "partial": "<span class='pill mid'>text only</span>"}


def mini(v):  # log bar from 1x to 256x
    return f"<span class='mini' aria-hidden='true'><i style='width:{logpos(v, 0, 8):.1f}%'></i></span>{fx(v)}"


srows = []
for m, name in MODELS:
    for i, (k, rname) in enumerate(ROUNDS):
        ix, cc = STRUCT[(k, m)]
        lead = f"<td rowspan='3' class='grp-name'><strong>{SHORT[m]}</strong></td>" if i == 0 else ""
        srows.append(f"<tr{' class=grp' if i == 0 else ''}>{lead}<td>{rname}</td><td>{LANG[(k, m)]}</td><td>{PILL[ix]}</td>"
                     f"<td class='num'>{mini(SPEED(k, m, 'perf_lookup'))}</td><td>{PILL[cc]}</td><td class='num'>{mini(SPEED(k, m, 'perf_aggregate'))}</td></tr>")
COMPILED = [r for r in RUNS if LANG[r] != "Python"]
IDX_YES = [SPEED(k, m, "perf_lookup") for k, m in COMPILED if STRUCT[(k, m)][0] == "yes"]
CC_YES = [SPEED(k, m, "perf_aggregate") for k, m in COMPILED if STRUCT[(k, m)][1] == "yes"]


def share(m, w):
    """Part of the sealed-to-Rust change in log speed that comes from workload w, in percent."""
    parts = {x: math.log(SPEED("rust", m, x) / SPEED("sealed", m, x)) for x, _ in WL}
    return round(100 * parts[w] / sum(parts.values()))


hk = [SPEED("rust", "haiku", w) / SPEED("sealed", "haiku", w) for w in ("perf_chain", "perf_mixed")]
CHANGE = {
    "opus": "No change. Opus had already chosen Rust in the sealed round.",
    "sonnet": f"Its lookup index came back. Lookups went from {fx(SPEED('sealed', 'sonnet', 'perf_lookup'))} to "
              f"{fx(SPEED('rust', 'sonnet', 'perf_lookup'))}, {share('sonnet', 'perf_lookup')}% of the gain. Both engines were Rust.",
    "fable": f"It added a criteria cache. Conditional sums went from {fx(SPEED('sealed', 'fable', 'perf_aggregate'))} to "
             f"{fx(SPEED('rust', 'fable', 'perf_aggregate'))}, {share('fable', 'perf_aggregate')}% of the gain. The rest is faster code on the other three workloads.",
    "haiku": f"Its chain and mixed workloads ran {min(hk):.0f} to {max(hk):.0f} times faster, but it dropped the index and the cache its Python engine had, "
             f"and its mixed workload came out {G[('rust', 'haiku', EV)]['performance']['perf_mixed']['match_rate']:.2%} correct, which the score counts as 1/8.",
}
crows = "".join(f"<tr><td><strong>{SHORT[m]}</strong></td><td class='num'>{LANG[('sealed', m)]}, {fx(E('sealed', m))}</td>"
                f"<td class='num'>{fx(E('rust', m))}</td><td>{CHANGE[m]}</td></tr>" for m, _ in MODELS)
CH = BASE["perf_chain.xlsx"]

# ---- appendix tables ----
grows, wrows, frows = [], [], []
for m, name in MODELS:
    for k, rname in ROUNDS:
        lead = f"<td><strong>{SHORT[m]}</strong></td><td>{rname}</td><td>{LANG[(k, m)]}</td>"
        sets = "".join(f"<td class='num'>{G[(k, m, ev)]['correctness']['cell_match_rate']:.2%}</td><td class='num'>{book_misses(k, m, ev)}</td>"
                       f"<td class='num'>{G[(k, m, ev)]['correctness']['books_perfect']}/160</td><td>{OKPILL if PASS(k, m, ev) else BADPILL}</td>"
                       f"<td class='num'>{fx(E(k, m, ev))}</td>" for ev in SETS)
        grows.append(f"<tr>{lead}{sets}</tr>")
        perf = G[(k, m, EV)]["performance"]
        wrows.append(f"<tr>{lead}" + "".join(f"<td class='num'>{fx(perf[w]['speedup_vs_lo'])}{'*' if perf[w]['match_rate'] < 0.999 else ''}</td>" for w, _ in WL) + "</tr>")
        cost = META[(k, m)].get("api_equivalent_cost_usd")
        frows.append(f"<tr>{lead}<td class='num'>{META[(k, m)]['wall_minutes']:.0f}</td><td class='num'>{sum(1 for _ in open(rd(k, m) + '/tools.jsonl'))}</td>"
                     f"<td class='num'>{probes(k, m)}</td><td class='num'>{loc(k, m):,}</td><td class='num'>{'$%.0f' % cost if cost else 'n/a'}</td></tr>")
PROBES = {r: probes(*r) for r in RUNS}
NO_PROBE = sum(1 for v in PROBES.values() if v == 0)
OPUS_PROBES = [PROBES[(k, "opus")] for k, _ in ROUNDS]

CSS = """
/* Layout: one reading column. Findings first, then one claim per section with the chart that supports it,
   then the changes to the benchmark, then the raw data folded away. */
:root{--bg:#f3f5f8;--surface:#ffffff;--fg:#17202a;--muted:#5b6775;--line:#d8dee6;--soft:#e9eef4;--code:#eef2f7;--accent:#0b6e8f;
--chart:#0070a8;--good:#1f8a4c;--good-bg:#e3f4ea;--bad:#b3362a;--bad-bg:#fbe3e0;
--display:"Bricolage Grotesque","Avenir Next",Avenir,"Helvetica Neue",Arial,sans-serif;--body:"Source Sans 3","Helvetica Neue",Arial,sans-serif;
--mono:"JetBrains Mono",ui-monospace,Menlo,Consolas,monospace}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#0f1419;--surface:#161c24;--fg:#e4e9ef;--muted:#97a3b1;--line:#2a333e;
--soft:#1d252f;--code:#1b232d;--accent:#62bbd8;--chart:#2b95cc;--good:#5ccf93;--good-bg:#15301f;--bad:#f07a6e;--bad-bg:#3a1a16;color-scheme:dark}}
:root[data-theme="dark"]{--bg:#0f1419;--surface:#161c24;--fg:#e4e9ef;--muted:#97a3b1;--line:#2a333e;--soft:#1d252f;--code:#1b232d;--accent:#62bbd8;
--chart:#2b95cc;--good:#5ccf93;--good-bg:#15301f;--bad:#f07a6e;--bad-bg:#3a1a16;color-scheme:dark}
*{box-sizing:border-box}
body{background:var(--bg);color:var(--fg);font-family:var(--body);font-size:16px;line-height:1.55;margin:0}
.wrap{max-width:920px;margin:0 auto;padding-block:32px 72px;padding-left:max(16px,env(safe-area-inset-left,0px));padding-right:max(16px,env(safe-area-inset-right,0px))}
h1,h2,h3{font-family:var(--display);text-wrap:balance;margin:0;line-height:1.15}
h1{font-size:clamp(29px,4.6vw,42px);letter-spacing:-.01em;max-width:22ch;margin-top:10px}
h2{font-size:clamp(21px,2.7vw,26px);margin-top:56px;padding-top:22px;border-top:1px solid var(--line);max-width:32ch}
h3{font-size:17px;margin-top:30px}
p,li{max-width:68ch}
a{color:var(--accent)}a:focus-visible,summary:focus-visible{outline:2px solid var(--accent);outline-offset:3px;border-radius:4px}
.eyebrow{font-family:var(--mono);font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:var(--muted)}
.lede{font-size:18px;color:var(--muted);margin-top:14px}
.muted{color:var(--muted)}.small{font-size:14px}
code{font-family:var(--mono);font-size:.88em}
.summary{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,390px),1fr));gap:12px;margin-top:26px}
.finding{background:var(--surface);border:1px solid var(--line);border-radius:10px;padding:14px 16px;display:flex;flex-direction:column;gap:6px;min-width:0}
.finding strong{font-family:var(--display);font-size:17px;line-height:1.25}
.finding span{color:var(--muted);font-size:15px}
.finding a{font-size:14px;font-family:var(--mono);margin-top:auto;padding-top:4px;text-decoration:none}
.finding a:hover{text-decoration:underline}
.panel{background:var(--surface);border:1px solid var(--line);border-radius:10px;padding:14px 18px 16px;margin-top:16px;min-width:0}
.panel-title{font-size:13px;color:var(--muted);margin:0 0 6px;font-family:var(--mono);letter-spacing:.04em;text-transform:uppercase}
.row{display:grid;grid-template-columns:72px 1fr 96px;gap:12px;align-items:center;margin:11px 0;font-size:14px}
.row.tl{grid-template-columns:84px 1fr 116px}
.name{color:var(--muted);white-space:nowrap;display:flex;flex-direction:column;line-height:1.15}
.name small{font-size:12px;opacity:.85}
.val{font-family:var(--mono);font-size:13px;text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}
.track{position:relative;height:14px;background:var(--soft);border-radius:3px}
.parity{position:absolute;top:-3px;bottom:-3px;width:2px;margin-left:-1px;background:var(--fg);opacity:.5}
.cap{position:absolute;top:-3px;bottom:-3px;border-left:2px dashed var(--bad);opacity:.65}
.range{position:absolute;top:6px;height:2px;background:var(--muted)}
.med{position:absolute;top:-5px;bottom:-5px;width:3px;margin-left:-1.5px;background:var(--fg);border-radius:2px}
.dot{position:absolute;top:7px;width:13px;height:13px;border-radius:50%;transform:translate(-50%,-50%);background:var(--chart);box-shadow:0 0 0 2px var(--surface)}
.dot.miss{background:var(--surface);border:2px solid var(--chart)}
.seg{position:absolute;top:2px;height:10px}
.seg.build{left:0;box-shadow:inset 0 0 0 2px var(--chart);border-radius:3px 0 0 3px}
.seg.after{background:var(--chart);border-radius:0 4px 4px 0}
.dot::after,.seg::after,.med::after{content:"";position:absolute;inset:-8px -3px}
.axis-row{margin-top:2px}.axis{position:relative;height:16px;font-family:var(--mono);font-size:11px;color:var(--muted)}
.axis span{position:absolute;transform:translateX(-50%);white-space:nowrap}
.legend{display:flex;flex-wrap:wrap;gap:6px 18px;font-size:13px;color:var(--muted);margin-top:12px}
.legend span{display:inline-flex;align-items:center;gap:7px}
.k{display:inline-block;flex:none}
.k-dot{width:11px;height:11px;border-radius:50%;background:var(--chart)}.k-ring{width:11px;height:11px;border-radius:50%;border:2px solid var(--chart)}
.k-med{width:3px;height:14px;background:var(--fg);border-radius:2px}.k-line{width:14px;border-top:2px solid var(--fg);opacity:.6}
.k-cap{width:14px;border-top:2px dashed var(--bad)}.k-solid{width:18px;height:10px;background:var(--chart);border-radius:2px}
.k-hollow{width:18px;height:10px;box-shadow:inset 0 0 0 2px var(--chart);border-radius:2px}
.points{padding-left:20px;margin:14px 0 0}.points li{margin:10px 0}
.cats{display:flex;flex-direction:column;gap:15px}
.cat-head{display:flex;flex-wrap:wrap;gap:1px 10px;align-items:baseline}
.cat-head strong{font-size:15px}.cat-head span{font-size:14px;color:var(--muted)}
.cat-bar{display:grid;grid-template-columns:1fr 40px;gap:10px;align-items:center;margin-top:6px}
.ctrack{display:flex;gap:2px;height:12px}.ctrack span{height:100%;border-radius:2px;flex:none}
.ctrack .sh{background:var(--chart)}.ctrack .fw{box-shadow:inset 0 0 0 2px var(--chart)}
.cells{display:flex;flex-direction:column;gap:12px;margin-top:14px}
.cell{margin:0;background:var(--surface);border:1px solid var(--line);border-radius:10px;overflow:hidden;min-width:0}
.fbar{display:flex;gap:10px;align-items:baseline;padding:9px 14px;background:var(--code);border-bottom:1px solid var(--line);font-family:var(--mono);font-size:13px}
.fbar .ref{flex:none;padding:1px 7px;border:1px solid var(--line);border-radius:4px;background:var(--surface);font-size:12px}
.fbar .fx{flex:none;color:var(--muted);font-style:italic}
.fbar code{font-size:13px;overflow-wrap:anywhere;min-width:0}
.res{display:flex;flex-wrap:wrap;align-items:flex-end;gap:8px 28px;padding:10px 14px 2px}
.res span{display:flex;flex-direction:column;min-width:0}
.res small{font-size:11px;color:var(--muted);text-transform:uppercase;letter-spacing:.07em;font-family:var(--mono)}
.res b{font-family:var(--mono);font-size:15px;font-weight:500;overflow-wrap:anywhere}
.res .where{margin-left:auto;font-family:var(--mono);font-size:11.5px;color:var(--muted)}
.cell figcaption{margin:6px 14px 12px;font-size:14.5px;color:var(--muted);max-width:70ch}
.tablewrap{overflow-x:auto;margin-top:14px;border:1px solid var(--line);border-radius:10px;background:var(--surface)}
table{border-collapse:collapse;width:100%;font-size:14.5px}
th,td{text-align:left;padding:9px 12px;border-top:1px solid var(--line);vertical-align:middle}
thead th{border-top:0;background:var(--soft);font-family:var(--mono);font-size:11.5px;letter-spacing:.05em;text-transform:uppercase;color:var(--muted);font-weight:500}
thead tr+tr th{border-top:1px solid var(--line)}
td.num,th.num{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}th.grp{text-align:center}
tr.grp td{border-top:2px solid var(--line)}td.grp-name{vertical-align:top}
.t-struct{min-width:660px}.t-change{min-width:620px}.t-data{min-width:860px}.t-wide{min-width:640px}
.pill{font-family:var(--mono);font-size:12px;padding:2px 8px;border-radius:999px;white-space:nowrap}
.ok{background:var(--good-bg);color:var(--good)}.bad{background:var(--bad-bg);color:var(--bad)}.mid{background:var(--soft);color:var(--muted)}
.mini{display:inline-block;position:relative;width:64px;height:6px;background:var(--soft);border-radius:3px;margin-right:10px;vertical-align:middle;overflow:hidden}
.mini i{position:absolute;left:0;top:0;bottom:0;background:var(--chart);border-radius:3px}
.note{border-left:3px solid var(--accent);padding:8px 14px;background:var(--surface);border-radius:0 8px 8px 0;max-width:74ch;margin-top:16px}
.recs{padding-left:22px}.recs li{margin:12px 0}
details{margin-top:12px;background:var(--surface);border:1px solid var(--line);border-radius:10px;padding:12px 16px}
summary{cursor:pointer;font-weight:600}
details .tablewrap{border-radius:8px}
details p{margin:10px 0 0}
#tip{position:fixed;pointer-events:none;opacity:0;background:var(--fg);color:var(--bg);font-size:13px;padding:5px 9px;border-radius:6px;max-width:300px;transition:opacity .1s;z-index:10}
@media (prefers-reduced-motion:reduce){#tip{transition:none}}
@media (max-width:560px){.row{grid-template-columns:54px 1fr 64px;gap:8px}.row.tl{grid-template-columns:54px 1fr 90px}
.name{font-size:12.5px}.val{font-size:11.5px}.axis{font-size:10px}.axis .minor{display:none}.panel{padding:12px 14px 14px}
.res .where{margin-left:0;width:100%}}
"""

S = lambda xs: (min(xs), max(xs))
html_out = f"""<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>Rebuild Bench Results</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,500;12..96,700&family=Source+Sans+3:wght@400;600&family=JetBrains+Mono:wght@400;500&display=swap">
<style>{CSS}</style>
<div class="wrap">
<div class="eyebrow">rebuild-bench · calc task · 12 rebuilds by 4 models over 3 rounds · test sets sealed1 and rust1</div>
<h1>Every model built a working engine within 20 minutes. The rest of the hour set them apart.</h1>
<p class="lede">Opus 5.5, Sonnet 5.5, Fable 5.1 and Haiku 5.5 each rebuilt LibreOffice Calc's formula engine from scratch three times, with 60 minutes per attempt and no internet. The last round required Rust. Every engine was graded on about 9,500 hidden formula cells and timed against LibreOffice on four workloads. Across the three attempts, Opus's median engine runs {MED['opus']:.0f} times faster than LibreOffice, Sonnet's {MED['sonnet']:.0f}, Fable's {MED['fable']:.0f} and Haiku's {MED['haiku']:.1f}. This page explains where those differences come from and what they mean for the benchmark.</p>

<div class="summary">
<div class="finding"><strong>Three attempts give a stable order</strong><span>Opus beats Sonnet in {WIN[('opus', 'sonnet')]} of 9 pairings of their attempts. A single model's attempts vary by up to {SPREAD:.1f}x, enough to put the top three in any order.</span><a href="#ranking">The ranking ↓</a></div>
<div class="finding"><strong>Passing is quick; pushing further varies</strong><span>All eight logged runs passed the public check by minute {max(d['pass'] for d in TL.values())}. Opus, Sonnet and Fable then made their engines {min(d['e1'] / d['e0'] for d in STRONG):.1f} to {max(d['e1'] / d['e0'] for d in STRONG):.0f} times faster. Haiku stopped within {max(d['wall'] - d['pass'] for d in HAIKU):.0f} minutes.</span><a href="#effort">How the hour was used ↓</a></div>
<div class="finding"><strong>The remaining errors are LibreOffice conventions</strong><span>{len(MISS) - SH['bug'] - FEW['bug']} of the {len(MISS)} wrong cells come from five rules, such as which error code wins. Most engines miss the same cells.</span><a href="#conventions">The conventions ↓</a></div>
<div class="finding"><strong>Speed comes from two data structures</strong><span>A hash index for lookups and a cache for criteria explain the largest gaps. Writing in Rust made formulas faster but never made up for a missing structure.</span><a href="#speed">Speed and Rust ↓</a></div>
</div>

<h2 id="ranking">Three attempts give a stable order</h2>
<p>Each model's three engines were graded on {EV}, a test set generated after all twelve runs ended. A dot is one attempt and the tick is the median of the three.</p>
<div class="panel"><p class="panel-title">Speed against LibreOffice, test set {EV}</p>{''.join(rank)}{log_axis(LO1, HI1, {-1, 1, 3, 5})}
<div class="legend"><span><i class="k k-dot"></i>attempt that passed the correctness gate</span><span><i class="k k-ring"></i>attempt that missed it</span><span><i class="k k-med"></i>median of three</span><span><i class="k k-line"></i>parity with LibreOffice</span><span><i class="k k-cap"></i>8x, where the score stops rising</span></div></div>
<p>Opus beats Sonnet in {WIN[('opus', 'sonnet')]} of 9 pairings of their attempts, Sonnet beats Fable in {WIN[('sonnet', 'fable')]} of 9, and Opus beats Fable in {WIN[('opus', 'fable')]} of 9. Every model beats Haiku in all 9. On sealed1, the other test set, the medians are {fx(MED_S['opus'])}, {fx(MED_S['sonnet'])}, {fx(MED_S['fable'])} and {fx(MED_S['haiku'])}, in the same order.</p>
<p>Single attempts overlap. Sonnet's best attempt ({fx(max(E(k, 'sonnet') for k, _ in ROUNDS))}) beat Opus's worst ({fx(min(E(k, 'opus') for k, _ in ROUNDS))}), and Fable's best ({fx(max(E(k, 'fable') for k, _ in ROUNDS))}) beat Sonnet's worst ({fx(min(E(k, 'sonnet') for k, _ in ROUNDS))}). Depending on which single attempts you pick, the top three can come out in any of the six possible orders. Cost follows yet another order: Fable's logged runs cost about ${META[('rust', 'fable')]['api_equivalent_cost_usd']:.0f} and ${META[('sealed', 'fable')]['api_equivalent_cost_usd']:.0f} at API prices, Opus's ${META[('rust', 'opus')]['api_equivalent_cost_usd']:.0f} and ${META[('sealed', 'opus')]['api_equivalent_cost_usd']:.0f}, Sonnet's ${META[('rust', 'sonnet')]['api_equivalent_cost_usd']:.0f} and ${META[('sealed', 'sonnet')]['api_equivalent_cost_usd']:.0f}, and Haiku's about ${META[('rust', 'haiku')]['api_equivalent_cost_usd']:.0f} each.</p>

<h2 id="effort">Every engine passed the public check within {max(d['pass'] for d in TL.values())} minutes</h2>
<p>The models could run the grader's own check on 40 public workbooks at any time. In the eight runs with full logs, every engine first passed that check between minute {min(d['pass'] for d in TL.values())} and minute {max(d['pass'] for d in TL.values())}. At that point its own speed check read between {fx(min(d['e0'] for d in TL.values()))} and {fx(max(d['e0'] for d in TL.values()))}. What happened next depended on the model.</p>
<div class="panel"><p class="panel-title">Minutes used of 60, sealed and Rust rounds</p>{''.join(timeline)}{TAXIS}
<div class="legend"><span><i class="k k-hollow"></i>building until the public check passed</span><span><i class="k k-solid"></i>working after that</span><span>label: own speed check when it first passed → when it stopped</span></div></div>
<ul class="points">
<li><strong>Opus, Sonnet and Fable kept improving.</strong> They worked another {min(d['wall'] - d['pass'] for d in STRONG):.0f} to {max(d['wall'] - d['pass'] for d in STRONG):.0f} minutes and finished {min(d['e1'] / d['e0'] for d in STRONG):.1f} to {max(d['e1'] / d['e0'] for d in STRONG):.0f} times faster by their own measure, well past the 8x where the score stops rising.</li>
<li><strong>Haiku stopped soon after passing.</strong> Both of its runs ended {min(d['wall'] - d['pass'] for d in HAIKU):.0f} to {max(d['wall'] - d['pass'] for d in HAIKU):.0f} minutes later, at {fx(HAIKU[0]['e1'])} and {fx(HAIKU[1]['e1'])}, with {60 - max(d['wall'] for d in HAIKU):.0f} to {60 - min(d['wall'] for d in HAIKU):.0f} minutes left, and each listed known gaps before it stopped. The sealed run's notes say its mixed workload was just under the 99.9% bar, which the score counts as 1/8. The Rust run checked itself six times in its last three minutes, scored 99.87% each time, below the bar, and replied DONE.</li>
<li><strong>Nobody used the whole hour.</strong> All twelve runs stopped with {min(UNUSED):.0f} to {max(UNUSED):.0f} minutes left. Offered the remaining time, each of the eight logged runs replied DONE after at most one more minute of work.</li>
<li><strong>They learned mostly from the examples.</strong> None of the twelve runs touched the LibreOffice window, and {NO_PROBE} never asked LibreOffice to calculate a workbook of their own. Opus probed the most, {min(OPUS_PROBES)} to {max(OPUS_PROBES)} times per run. The hidden workbooks come from the same generator as the public ones, so fitting the examples is enough to pass.</li>
</ul>

<h2 id="conventions">The remaining errors are LibreOffice conventions, and most engines miss the same ones</h2>
<p>All twelve engines got between {min(G[(k, m, ev)]['correctness']['cell_match_rate'] for k, m in RUNS for ev in SETS):.2%} and 100% of the hidden cells right on both test sets. Across the 24 gradings that leaves {len(MISS)} wrong cells, and we sorted every one. Only {SH['bug'] + FEW['bug']} are ordinary bugs. The other {len(MISS) - SH['bug'] - FEW['bug']} come from five rules in areas the task contract tells the models to work out for themselves, such as error propagation and number-to-text conversion.</p>
<div class="panel"><p class="panel-title">The {len(MISS)} wrong cells, by the rule behind them</p><div class="cats">{''.join(cats)}</div>
<div class="legend"><span><i class="k k-solid"></i>on cells that 3 or more of the 12 engines got wrong</span><span><i class="k k-hollow"></i>on cells fewer engines got wrong</span></div></div>
<p>{sum(SH.values())} of the {len(MISS)} misses fall on cells that at least three engines got wrong, and four cells on {EV} were missed by {WIDE[-1][0]} to {WIDE[0][0]} of the twelve. Three of those cells:</p>
<div class="cells">{EXAMPLES}</div>
<p>The gate allows nine wrong cells in about 9,500. A test set holds only a few cells for each rule, so the verdict depends on which ones it happens to contain. Fable's Rust engine missed {book_misses('rust', 'fable', 'sealed1')} cells on sealed1 and passed, then {book_misses('rust', 'fable', 'rust1')} on rust1 and failed. Haiku's missed {book_misses('rust', 'haiku', 'sealed1')} and {book_misses('rust', 'haiku', 'rust1')}, the other way round.</p>

<h2 id="speed">Speed comes from two data structures more than from the language</h2>
<p>Most of the speed score comes from two workloads where LibreOffice is slow: 5,000 exact lookups over 20,000 rows ({BASE['perf_lookup.xlsx']['total_s']:.1f} s) and 1,000 conditional sums over 50,000 rows ({BASE['perf_aggregate.xlsx']['total_s']:.1f} s). Two classic structures decide them: a hash index for exact-match lookups, and a cache of the rows that match each criterion. We checked every engine's source code for both.</p>
<div class="tablewrap"><table class="t-struct"><thead><tr><th>Model</th><th>Round</th><th>Language</th><th>Lookup index</th><th class="num">Lookups</th><th>Criteria cache</th><th class="num">Conditional sums</th></tr></thead><tbody>{''.join(srows)}</tbody></table></div>
<p class="muted small">Speed against LibreOffice on {EV}. Bars use a log scale from 1x to 256x.</p>
<p>Compiled engines with an index ran lookups {S(IDX_YES)[0]:.0f} to {S(IDX_YES)[1]:.0f} times faster than LibreOffice. The one without, Sonnet's sealed engine, reached {fx(SPEED('sealed', 'sonnet', 'perf_lookup'))}. With a criteria cache, conditional sums ran {S(CC_YES)[0]:.0f} to {S(CC_YES)[1]:.0f} times faster. Fable's sealed Go engine had none and reached {fx(SPEED('sealed', 'fable', 'perf_aggregate'))}, even though it spread the work over four cores. Python costs Haiku roughly another factor of ten: its sealed engine had both structures and still ran lookups at {fx(SPEED('sealed', 'haiku', 'perf_lookup'))}.</p>
<h3>What forcing Rust changed</h3>
<div class="tablewrap"><table class="t-change"><thead><tr><th>Model</th><th class="num">Sealed round</th><th class="num">Rust round</th><th>What changed</th></tr></thead><tbody>{crows}</tbody></table></div>
<p class="muted small">Overall speed on {EV}. The share of a gain is that workload's part of the change in the geometric mean.</p>
<p>The dependency chain is mostly a file-loading test. LibreOffice spends {CH['load_s'] * 1000:.0f} ms loading it and {CH['calc_s'] * 1000:.0f} ms calculating it, and Opus's Rust engine timed itself at about 26 of its 30 ms on unzipping and parsing. It still ranks the models in the same order in every round: Opus, Sonnet, Fable, Haiku.</p>

<h2 id="changes">What to change in the benchmark</h2>
<ul class="recs">
<li><strong>Run at least three attempts per model</strong> and report the median and head-to-head win rates. One attempt can move a model's speed by {SPREAD:.1f}x.</li>
<li><strong>Test LibreOffice's conventions directly.</strong> Build a probe set for each rule (error codes, number printing, empty-cell criteria, coercion, TRUE/FALSE results) and score each one. Keep some rules out of the public examples, so that finding them takes queries to LibreOffice. That measures reverse engineering instead of example-fitting, and it stops the gate from flipping between test sets.</li>
<li><strong>Make the speed workloads exercise the engine.</strong> Scale them up 10 to 100 times, add recalculation after single-cell edits, and time loading separately, so that file parsing stops dominating.</li>
<li><strong>Remove the 8x cap from the score and the contract.</strong> Opus, Sonnet and Fable all score the maximum 1.000 whenever they pass. A 64x cap, or plain log speed, would separate 10x from 27x.</li>
<li><strong>Measure persistence on its own.</strong> Add a condition where the model must keep improving until the hour ends, to see what the {min(UNUSED):.0f} to {max(UNUSED):.0f} unused minutes are worth.</li>
<li><strong>Add a GUI-only condition</strong> if computer use is the goal. In twelve runs no model touched the LibreOffice window.</li>
</ul>

<h2 id="data">Data</h2>
<details><summary>How the benchmark works</summary>
<p>Each attempt had to rebuild the formula engine of LibreOffice Calc 25.2 (87 functions) as a program that reads an .xlsx workbook and writes every formula's value. It had 60 minutes in an offline sandbox with LibreOffice installed, 40 public example workbooks with expected values, and a check command that scores the public set with the grader's rules.</p>
<p>The score has a correctness gate (99.9% of hidden cells and 95% of hidden workbooks), then speed against LibreOffice in a warm process (load plus full recalculation) on four workloads, combined as a geometric mean. A workload under 99.9% correct counts as 1/8. The score is 0.5 at parity and 1.0 at 8x.</p>
<p>The pilot ran the models as subagents with a free choice of language. The sealed round ran headless Claude Code with only the sandbox tools, still with a free choice. The Rust round used the sealed harness and required Rust, which the grader enforced. Test set sealed1 was generated after the sealed round and rust1 after the Rust round, with 160 workbooks each. All grades come from an idle machine; regrading two sealed engines in the last pass reproduced their earlier speeds within about 5%.</p></details>
<details><summary>All 24 gradings</summary>
<div class="tablewrap"><table class="t-data"><thead><tr><th colspan="3"></th><th colspan="5" class="grp">Test set sealed1</th><th colspan="5" class="grp">Test set rust1</th></tr>
<tr><th>Model</th><th>Round</th><th>Language</th><th class="num">Cells</th><th class="num">Wrong</th><th class="num">Books</th><th>Gate</th><th class="num">Speed</th><th class="num">Cells</th><th class="num">Wrong</th><th class="num">Books</th><th>Gate</th><th class="num">Speed</th></tr></thead><tbody>{''.join(grows)}</tbody></table></div></details>
<details><summary>Workload speeds on {EV}</summary>
<div class="tablewrap"><table class="t-wide"><thead><tr><th>Model</th><th>Round</th><th>Language</th>{''.join(f'<th class="num">{label}</th>' for _, label in WL)}</tr></thead><tbody>{''.join(wrows)}</tbody></table></div>
<p class="muted small">* Under 99.9% of the workload's cells correct, so the score counts it as 1/8. LibreOffice takes {BASE['perf_chain.xlsx']['total_s']:.2f} s, {BASE['perf_mixed.xlsx']['total_s']:.2f} s, {BASE['perf_lookup.xlsx']['total_s']:.2f} s and {BASE['perf_aggregate.xlsx']['total_s']:.2f} s.</p></details>
<details><summary>Run facts</summary>
<div class="tablewrap"><table class="t-wide"><thead><tr><th>Model</th><th>Round</th><th>Language</th><th class="num">Minutes</th><th class="num">Tool calls</th><th class="num">LibreOffice probes</th><th class="num">Lines of code</th><th class="num">Cost</th></tr></thead><tbody>{''.join(frows)}</tbody></table></div>
<p class="muted small">Cost is the CLI's estimate at API prices; the runs used a Max plan, and the pilot harness did not record cost. No run used the GUI tools.</p></details>
</div>
<div id="tip" role="tooltip"></div>
<script>
const tip = document.getElementById("tip");
function onPointer(e) {{
  const t = e.target.closest("[data-tip]");
  if (!t) {{ tip.style.opacity = 0; return; }}
  tip.textContent = t.dataset.tip; tip.style.opacity = 1;
  tip.style.left = Math.max(8, Math.min(e.clientX + 12, innerWidth - tip.offsetWidth - 8)) + "px";
  tip.style.top = (e.clientY + 16) + "px";
}}
document.addEventListener("pointermove", onPointer);
document.addEventListener("pointerdown", onPointer);
</script>"""
open(OUT, "w").write(html_out)

print("wrote", OUT, len(html_out), "bytes")
print("medians rust1", {m: round(v, 2) for m, v in MED.items()}, "sealed1", {m: round(v, 2) for m, v in MED_S.items()}, "spread", round(SPREAD, 2))
print("wins", WIN)
print("timeline", {f"{k}-{m}": (d["pass"], d["wall"], round(d["e0"], 1), round(d["e1"], 1), round(d["e1"] / d["e0"], 1)) for (k, m), d in TL.items()})
print("unused", round(min(UNUSED), 1), round(max(UNUSED), 1), "probes", {f"{k}-{m}": v for (k, m), v in PROBES.items()})
print("misses", len(MISS), "shared", dict(SH), "few", dict(FEW), "widest", WIDE)
print("idx compiled", [round(v, 1) for v in IDX_YES], "cache compiled", [round(v, 1) for v in CC_YES])
print("shares sonnet lookup", share("sonnet", "perf_lookup"), "fable aggregate", share("fable", "perf_aggregate"), "haiku ratios", [round(v, 1) for v in hk])
