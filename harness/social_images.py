"""Render the social-media images for the rebuild-bench results: 1600x900 layouts saved as 3200x1800 PNGs by headless Chrome.

python harness/social_images.py <out_dir>
Every number comes from results_page.py, so the images always match the results page. Each image carries one short
claim in a spreadsheet formula bar and one chart labelled on its marks, so it still reads at phone size.
"""
import html, os, runpy, shutil, subprocess, sys, tempfile, time

RB = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
OUTDIR = sys.argv[1]
sys.argv = [sys.argv[0], os.devnull]  # results_page.py writes its page to argv[1]; only its numbers are needed here
R = runpy.run_path(f"{RB}/harness/results_page.py")
E, SPEED, TL, MED, META, SHORT, fx, logpos = R["E"], R["SPEED"], R["TL"], R["MED"], R["META"], R["SHORT"], R["fx"], R["logpos"]
MODELS, ROUNDS = R["MODELS"], R["ROUNDS"]
esc = lambda s: html.escape(str(s), quote=True)

CSS = """
/* A spreadsheet sheet: one claim in the formula bar, one chart on a faint cell grid, labels on the marks. */
*{box-sizing:border-box;margin:0}
html,body{width:1600px;height:900px;overflow:hidden}
body{background:#fbfcfd;color:#17202a;font-family:"Source Sans 3","Helvetica Neue",Arial,sans-serif;-webkit-font-smoothing:antialiased}
.sheet{width:1600px;height:900px;display:grid;grid-template-rows:auto 1fr auto}
.fbar{display:flex;align-items:center;gap:24px;padding:34px 60px 34px 40px;background:#fff;border-bottom:2px solid #d8dee6}
.nb{flex:none;font:500 24px/1 "JetBrains Mono",monospace;color:#5b6775;border:2px solid #d8dee6;border-radius:8px;padding:12px 18px}
.fx{flex:none;font:italic 600 40px/1 Georgia,"Times New Roman",serif;color:#a3adb8}
.formula{font:800 62px/1.06 "Bricolage Grotesque","Avenir Next",sans-serif;letter-spacing:-.015em;text-wrap:balance}
.formula.code{font:500 27px/1.35 "JetBrains Mono",monospace;letter-spacing:0}
main{position:relative;min-height:0;padding:10px 80px 0 64px;display:flex;flex-direction:column;justify-content:center;
 background-image:linear-gradient(#edf0f4 1.5px,transparent 1.5px),linear-gradient(90deg,#edf0f4 1.5px,transparent 1.5px);background-size:128px 44px}
footer{display:flex;justify-content:space-between;padding:14px 60px 22px 64px;font:600 21px "JetBrains Mono",monospace;color:#98a2ae}
.row{display:grid;gap:30px;align-items:center}
.name{font:700 34px/1 "Source Sans 3",sans-serif}
.track{position:relative;height:46px;background:#e6ebf1;border-radius:6px}
.end{font:600 32px/1 "JetBrains Mono",monospace;white-space:nowrap;text-align:right;font-variant-numeric:tabular-nums}
.axis{position:relative;height:40px;margin-top:4px;font:500 22px "JetBrains Mono",monospace;color:#7b8794}
.axis span{position:absolute;top:10px;transform:translateX(-50%);white-space:nowrap}
.axis-title{font:600 22px "Source Sans 3",sans-serif;color:#7b8794}
.parity{position:absolute;top:-16px;bottom:-16px;width:4px;margin-left:-2px;background:#17202a;opacity:.4}
"""
FONTS = ("https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,800&family=Source+Sans+3:wght@500;600;700"
         "&family=JetBrains+Mono:wght@500;600;700&display=swap")


def sheet(claim, body, css="", namebox=None, code=False):
    nb = f"<span class='nb'>{esc(namebox)}</span>" if namebox else ""
    return (f"<!doctype html><html><head><meta charset='utf-8'><link rel='stylesheet' href='{FONTS}'><style>{CSS}{css}</style></head>"
            f"<body><div class='sheet'><div class='fbar'>{nb}<span class='fx'>fx</span><div class='formula{' code' if code else ''}'>{claim}</div></div>"
            f"<main>{body}</main><footer><span>rebuild-bench</span><span>@BrandeisPatrick</span></footer></div></body></html>")


# ---- 1. Minutes used in the Rust round, where all four had the same harness and language ----
def minutes_used():
    css = """.trow{grid-template-columns:190px 1fr 150px;height:112px}
.build{position:absolute;left:0;top:0;bottom:0;background:#fff;box-shadow:inset 0 0 0 4px #0070a8;border-radius:6px 0 0 6px}
.after{position:absolute;top:0;bottom:0;background:#0070a8;border-radius:0 7px 7px 0}
.tag{position:absolute;top:50%;transform:translateY(-50%);padding-left:16px;font:700 23px "Source Sans 3",sans-serif;white-space:nowrap}
.build .tag{color:#0070a8}.after .tag{color:#fff}
.note{position:absolute;top:50%;transform:translateY(-50%);white-space:nowrap;font:700 29px "Source Sans 3",sans-serif;color:#b3362a}"""
    runs = [("rust", m) for m, _ in MODELS]
    rows = []
    for i, r in enumerate(runs):
        d = TL[r]
        a, w = d["pass"] / 60 * 100, d["wall"] / 60 * 100
        note = f"<span class='note' style='left:calc({w:.2f}% + 22px)'>said DONE, own test failing</span>" if r[1] == "haiku" else ""
        rows.append(f"<div class='row trow'><span class='name'>{SHORT[r[1]]}</span><div class='track'>"
                    f"<span class='build' style='width:{a:.2f}%'>{'<span class=tag>building</span>' if i == 0 else ''}</span>"
                    f"<span class='after' style='left:calc({a:.2f}% + 4px);width:calc({w - a:.2f}% - 4px)'>"
                    f"{'<span class=tag>improving</span>' if i == 0 else ''}</span>{note}</div>"
                    f"<span class='end'>{d['wall']:.0f} min</span></div>")
    axis = "".join(f"<span style='left:{t / 60 * 100:.2f}%'>{t}{' min' if t == 60 else ''}</span>" for t in (0, 20, 40, 60))
    first_pass = max(TL[r]["pass"] for r in runs)
    return sheet(f"All four had a working engine by minute {first_pass}. Then they split apart.",
                 f"{''.join(rows)}<div class='axis' style='margin-left:220px;margin-right:180px'>{axis}</div>", css)


# ---- 2. Three attempts per model ----
def attempts():
    lo, hi = -1, 6
    p = lambda v: logpos(v, lo, hi)
    css = """.vrow{grid-template-columns:190px 1fr 210px;height:128px}
.range{position:absolute;top:21px;height:5px;background:#8a95a3}
.dot{position:absolute;top:23px;width:40px;height:40px;border-radius:50%;transform:translate(-50%,-50%);background:#0070a8;box-shadow:0 0 0 4px #fbfcfd}
.med{position:absolute;top:-14px;bottom:-14px;width:7px;margin-left:-3.5px;background:#17202a;border-radius:4px}
.end b{display:block;font:700 46px/1 "JetBrains Mono",monospace}.end small{display:block;margin-top:8px;font:600 21px "Source Sans 3",sans-serif;color:#7b8794}"""
    rows = []
    for m, _ in MODELS:
        vals = [E(k, m) for k, _ in ROUNDS]
        dots = "".join(f"<span class='dot' style='left:{p(v):.2f}%'></span>" for v in vals)
        rows.append(f"<div class='row vrow'><span class='name'>{SHORT[m]}</span><div class='track'>"
                    f"<span class='parity' style='left:{p(1):.2f}%'></span>"
                    f"<span class='range' style='left:{p(min(vals)):.2f}%;width:{p(max(vals)) - p(min(vals)):.2f}%'></span>"
                    f"<span class='med' style='left:{p(MED[m]):.2f}%'></span>{dots}</div>"
                    f"<div class='end'><b>{fx(MED[m])}</b><small>median</small></div></div>")
    axis = "".join(f"<span style='left:{(k - lo) / (hi - lo) * 100:.2f}%'>{2 ** k}x</span>" for k in (0, 2, 4, 6))
    return sheet(f"Same model, same task, three tries. Up to {R['SPREAD']:.1f}x apart.",
                 f"{''.join(rows)}<div class='axis' style='margin-left:220px;margin-right:240px'>{axis}</div>"
                 f"<p class='axis-title' style='margin-left:220px'>times faster than LibreOffice</p>", css)


# ---- 3. Haiku in Python, then in Rust ----
def haiku_rust():
    lo, hi = -4, 4
    p = lambda v: logpos(v, lo, hi)
    css = """.grp{display:grid;grid-template-columns:200px 1fr 250px;gap:30px;align-items:center;margin:22px 0}
.gname{font:700 34px/1 "Source Sans 3",sans-serif}
.bars{display:flex;flex-direction:column;gap:14px}
.brow{display:grid;grid-template-columns:100px 1fr;gap:16px;align-items:center}
.series{font:700 25px "Source Sans 3",sans-serif}.series.py{color:#c26a1b}.series.rs{color:#0070a8}
.bar{position:absolute;top:0;bottom:0}.bar.py{background:#c26a1b}.bar.rs{background:#0070a8}
.right{border-radius:0 7px 7px 0}.left{border-radius:7px 0 0 7px}
.bv{position:absolute;top:50%;transform:translateY(-50%);font:600 27px "JetBrains Mono",monospace;white-space:nowrap}
.lo{position:absolute;bottom:calc(100% + 18px);transform:translateX(-50%);font:600 21px "Source Sans 3",sans-serif;color:#7b8794;white-space:nowrap}
.verdict b{display:block;font:800 44px/1 "Bricolage Grotesque",sans-serif}.verdict small{display:block;margin-top:8px;font:700 23px "Source Sans 3",sans-serif}
.good{color:#1f8a4c}.bad{color:#b3362a}"""

    def bar(v, cls, first):
        a, b = sorted((p(1), p(v)))
        lab = "<span class='lo' style='left:{:.2f}%'>LibreOffice</span>".format(p(1)) if first else ""
        val = (f"<span class='bv' style='left:calc({b:.2f}% + 14px)'>{fx(v)}</span>" if v >= 1
               else f"<span class='bv' style='left:calc({a:.2f}% - 14px);transform:translate(-100%,-50%)'>{fx(v)}</span>")
        return (f"<div class='track'><span class='parity' style='left:{p(1):.2f}%'></span>{lab}"
                f"<span class='bar {cls} {'right' if v >= 1 else 'left'}' style='left:{a:.2f}%;width:{max(b - a, 0.5):.2f}%'></span>{val}</div>")

    groups = []
    for i, (w, label) in enumerate([("perf_chain", "Formulas"), ("perf_lookup", "Lookups")]):
        py, rs = SPEED("sealed", "haiku", w), SPEED("rust", "haiku", w)
        verdict = (f"<b class='good'>{rs / py:.0f}x faster</b>" if rs > py
                   else f"<b class='bad'>{py / rs:.0f}x slower</b><small class='bad'>dropped its hash index</small>")
        groups.append(f"<div class='grp'><span class='gname'>{label}</span><div class='bars'>"
                      f"<div class='brow'><span class='series py'>Python</span>{bar(py, 'py', i == 0)}</div>"
                      f"<div class='brow'><span class='series rs'>Rust</span>{bar(rs, 'rs', False)}</div></div>"
                      f"<div class='verdict'>{verdict}</div></div>")
    return sheet("Rust sped up Haiku's formulas. Its lookups got slower.", "".join(groups), css)


# ---- 4. The most-missed kind of cell ----
def wrong_cells():
    css = """.q{font:800 60px/1.08 "Bricolage Grotesque",sans-serif;letter-spacing:-.012em;text-align:center}
.tiles{display:flex;justify-content:center;gap:40px;margin-top:40px}
.tile{background:#fff;border:3px solid #d8dee6;border-radius:16px;padding:28px 44px;min-width:440px;text-align:center}
.tile small{display:block;font:700 25px "Source Sans 3",sans-serif;color:#5b6775}
.tile b{display:block;margin-top:14px;font:700 92px/1 "JetBrains Mono",monospace}.tile.bad b{color:#b3362a}
.stat{margin-top:46px;text-align:center;font:700 31px "Source Sans 3",sans-serif;color:#5b6775}.stat b{color:#17202a}"""
    key = (R["EV"], "book_144659_0036", "E11")
    x = next(xx for kk, _, _, xx in R["MISS"] if kk == key)
    bugs = R["SH"]["bug"] + R["FEW"]["bug"]
    body = (f"<p class='q'>Which error does LibreOffice show?</p>"
            f"<div class='tiles'><div class='tile'><small>LibreOffice</small><b>{esc(R['show'](x['expected']))}</b></div>"
            f"<div class='tile bad'><small>{len(R['WHO'][key])} of 12 AI-built engines</small><b>{esc(R['show'](x['got']))}</b></div></div>"
            f"<p class='stat'><b>{len(R['MISS']) - bugs} of {len(R['MISS'])}</b> wrong cells were LibreOffice quirks like this.</p>")
    return sheet("=" + esc(x["formula"].replace("_xlfn.", "")), body, css, namebox="E11", code=True)


# ---- 5. Median speed against cost ----
def cost():
    css = """.hdr{display:grid;grid-template-columns:190px 1fr 230px;gap:30px;font:600 22px "Source Sans 3",sans-serif;color:#7b8794;margin-bottom:6px}
.hdr span:last-child{text-align:right}
.crow{grid-template-columns:190px 1fr 230px;height:122px}
.ctrack{position:relative;height:62px}
.cbar{position:absolute;left:0;top:0;bottom:0;background:#0070a8;border-radius:0 8px 8px 0}
.cv{position:absolute;top:50%;transform:translateY(-50%);font:700 34px "JetBrains Mono",monospace;white-space:nowrap}
.cv.in{color:#fff;transform:translate(-100%,-50%)}
.price{font:700 42px/1 "JetBrains Mono",monospace;text-align:right}.price.top{color:#c26a1b}"""
    costs = {m: sorted(META[(k, m)]["api_equivalent_cost_usd"] for k in ("sealed", "rust")) for m, _ in MODELS}
    priciest = max(costs, key=lambda m: costs[m][1])
    rows = []
    for m, _ in MODELS:
        w = MED[m] / 30 * 100
        val = (f"<span class='cv in' style='left:calc({w:.2f}% - 16px)'>{fx(MED[m])}</span>" if w > 25
               else f"<span class='cv' style='left:calc({w:.2f}% + 16px)'>{fx(MED[m])}</span>")
        lo, hi = costs[m]
        price = f"${lo:.0f}" if round(lo) == round(hi) else f"${lo:.0f}–{hi:.0f}"
        rows.append(f"<div class='row crow'><span class='name'>{SHORT[m]}</span><div class='ctrack'><span class='cbar' style='width:{w:.2f}%'></span>{val}</div>"
                    f"<span class='price{' top' if m == priciest else ''}'>{price}</span></div>")
    body = (f"<div class='hdr'><span></span><span>times faster than LibreOffice (median of 3 runs)</span><span>cost per run</span></div>"
            f"{''.join(rows)}")
    return sheet("The most expensive model finished third.", body, css)


def render(page, png):
    """Write a 2x PNG of the page with headless Chrome, which writes the file but does not always exit on its own."""
    work = tempfile.mkdtemp(prefix="rb-social-")
    src = os.path.join(work, "image.html")
    open(src, "w").write(page)
    if os.path.exists(png):
        os.remove(png)  # the wait below watches for the file, so a stale one would end it early
    chrome = subprocess.Popen([CHROME, "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
                               "--disable-extensions", "--hide-scrollbars", "--window-size=1600,900", "--virtual-time-budget=8000",
                               "--force-device-scale-factor=2", f"--user-data-dir={work}/profile", f"--screenshot={png}", f"file://{src}"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    last, ok, deadline = -1, False, time.time() + 60
    while time.time() < deadline and not ok:
        time.sleep(0.4)
        if os.path.exists(png):
            size = os.path.getsize(png)
            ok, last = size > 0 and size == last, size
    chrome.kill()
    subprocess.run(["pkill", "-9", "-f", work], capture_output=True)  # Chrome's helpers carry this render's profile folder
    shutil.rmtree(work, ignore_errors=True)
    if not ok:
        raise SystemExit(f"Chrome produced no screenshot for {png}")


os.makedirs(OUTDIR, exist_ok=True)
for i, (name, make) in enumerate([("minutes-used", minutes_used), ("attempts", attempts), ("haiku-rust", haiku_rust),
                                  ("wrong-cells", wrong_cells), ("cost", cost)], 1):
    page = make()
    open(os.path.join(OUTDIR, f"{i}-{name}.html"), "w").write(page)
    render(page, os.path.join(OUTDIR, f"{i}-{name}.png"))
    print("wrote", os.path.join(OUTDIR, f"{i}-{name}.png"))
