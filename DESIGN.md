# Rebuild Bench — Design v0.1 (2026-10-14)

Benchmark AI agents on rebuilding four famous applications from scratch. The real
products (Photoshop, Lightroom, Word, Excel) cannot be redistributed or reverse
engineered under their EULAs, so each task uses an **open stand-in** as the thing the
agent explores and an **open file format** as the behavioral contract. Scoring is
**correctness-gated CPU efficiency** of the rebuild (GPU scoring dropped 2026-10-15, see §1.3).

| Task | Product | Stand-in the agent explores | Format contract | Oracle |
|---|---|---|---|---|
| `calc` | Excel | LibreOffice Calc | XLSX, ECMA-376 §18.17 formulas | LibreOffice headless recalculation via UNO |
| `raw` | Lightroom | darktable (RawTherapee as cross-check) | DNG + an edits JSON mirroring darktable module params | `darktable-cli` with generated XMP sidecars |
| `layers` | Photoshop | Krita (GIMP as cross-check) | PSD per Adobe's published spec + an ops JSON | GIMP batch / Krita Python applying the same ops |
| `docx` | Word | LibreOffice Writer | DOCX, ECMA-376 | LibreOffice headless → PDF → `pdftoppm` PNG + text |

Capabilities exercised: computer use (the stand-ins are GUI apps), multimodal reading
(rendered sheets, photos, pages), long-horizon engineering, CPU optimization
(multi-core scaling, SIMD, memory layout, algorithms).

---

## 1. Isolation architecture

Three container images. The agent never touches the grader or the hidden corpus; the
grader never contains the stand-in apps.

```
                 maintainer only (offline, versioned)
  ┌──────────────────────────────────────────────────────────┐
  │ ORACLE IMAGE: LibreOffice, darktable, RawTherapee, GIMP, │
  │ Krita, poppler, Liberation+DejaVu fonts, dnglab, Python  │
  │   → generates public/ and hidden/ ground truth           │
  └──────────────────────────────────────────────────────────┘
            │ public examples + contract docs           │ hidden truth (never leaves host)
            ▼                                           ▼
  ┌────────────────────────────┐          ┌────────────────────────────────────┐
  │ AGENT SANDBOX (Docker,     │  source  │ GRADER SANDBOX (Docker, --network  │
  │ --network none)            │ ───────▶ │ none, fresh per run)               │
  │ • stand-in app on Xvfb     │          │ • NO stand-in apps installed       │
  │ • noVNC + xdotool tools    │          │ • build from source, deny-list scan│
  │ • toolchains, whitelisted  │          │ • hidden corpus mounted read-only  │
  │   deps vendored            │          │ • pinned CPUs/mem, timed runs      │
  │ • public examples only     │          │ • writes reward.json               │
  └────────────────────────────┘          └────────────────────────────────────┘
```

The model API is called from the **host harness**, which drives the agent sandbox with
`docker exec`. The sandbox itself has no network, so nothing inside it can exfiltrate
or fetch source code for the stand-ins.

### 1.1 Agent sandbox

Base: the Anthropic computer-use reference container pattern (Xvfb + noVNC + xdotool),
Linux arm64 on Apple Silicon via Docker Desktop, OrbStack, or Colima.

Contains:
- the task's stand-in app with a GUI session already open on display `:1`
- `public/` examples: input files, ops/edits scripts, and the matching expected outputs
  (≈20% of the corpus; the hidden 80% never enters this image)
- `CONTRACT.md` for the task (replay interface, ops schema, allowed dependencies)
- toolchains: clang, rustc + cargo, go, python3, node; vendored copies of every
  whitelisted dependency (no registry access)
- agent tools exposed by the harness: `bash`, `read`, `write`, `screenshot`, `click`,
  `double_click`, `right_click`, `type`, `key`, `scroll`, `drag`, `wait`
- a `/workspace` the agent owns; the final submission is `/workspace/submission/`
  containing source, `build.sh`, `run.sh`, and `DEPS.txt`

Does **not** contain: the hidden corpus, the oracle scripts, the grader, network.

Resource limits: `--cpus 4 --memory 12g --pids-limit 2048`, per-run wall-clock cap
(default 8 h) and dollar cap enforced by the harness.

Every computer-use action and shell command is logged with timestamps and a
screenshot hash, which feeds the process metrics in §4.3.

### 1.2 Grader sandbox

A fresh container from a **clean** image per submission: toolchains and vendored
whitelisted deps only. Steps:

1. Copy `submission/` in. Reject if it contains any prebuilt binaries, archives, or
   files > 5 MB that are not source/assets declared in `DEPS.txt`.
2. Static deny-list scan: any reference to `soffice`, `libreoffice`, `darktable`,
   `rawtherapee`, `gimp`, `krita`, `convert`, `magick`, `ffmpeg`, `python-uno`, or any
   dependency outside the whitelist → **hard fail**.
3. `build.sh` with network disabled, 30-minute cap.
4. Mount `hidden/` read-only. Run the contract CLI over the corpus with per-item
   timeouts. Capture stdout, stderr, exit codes, outputs.
5. Determinism check: rerun 10% of items; outputs must be byte-identical (images may
   differ by ≤1 LSB only if the submission declares GPU/float nondeterminism).
6. Efficiency runs on the workload set (§4.2): 1 warm-up + 5 measured, report median
   and CV. `--cpus` pinned, memory capped, nothing else running on the host.
7. Write `reward.json` (Harbor-compatible: `tests/` writes `/logs/verifier/reward.txt`
   with the headline score and a sidecar JSON with every metric).

Noise control: the stand-in baseline is measured in the **same** grader image on the
same host and the same day. A speedup counts only if it exceeds 2× the measured
coefficient of variation. The host fingerprint (chip, macOS, Docker runtime, VM CPUs)
is recorded in every result.

### 1.3 GPU scoring: out of scope

Decided 2026-10-15: efficiency is scored on CPU only. The grader is a Linux container
with no GPU, so submissions must run on CPU; GPU APIs (Metal, wgpu, CUDA, OpenCL) are
not available in the grader and GPU code is not gradable. Reasons: CPU efficiency
already captures the optimization work that matters for these four apps (algorithms,
multi-core scaling, SIMD, memory layout); only `raw` and `layers` could have used a GPU
at all; and GPU scoring on a Mac would need a macOS VM (50–60 GB of disk) and would
produce Mac-only numbers that nobody can reproduce in Docker. Parallel efficiency
(1 vs 4 cores) stays as a reported secondary metric (§4.2). A separate GPU track can be
added later without changing any contract.

### 1.4 Hard-fail rules

- Any call to a stand-in app or non-whitelisted dependency at build or run time.
- Build fails in the clean grader image.
- Nondeterministic outputs without a declared, bounded reason.
- Reading anything outside the item's input paths (checked with `strace`-style
  open-file logging in the grader for a sample of items).
- Output written by copying the input unchanged (detected by hashing).

### 1.5 Harness conditions (ablations)

Every task runs under matched conditions so the leaderboard has a within-model delta:

| Condition | Stand-in GUI | Computer use | Spec text | RE tools (REA, Ghidra) |
|---|---|---|---|---|
| `full` | yes | yes | yes | yes |
| `no-gui` | CLI of stand-in only | no | yes | yes |
| `spec-only` | no | no | yes | no |

`full` vs `no-gui` measures what computer use buys. `spec-only` measures how much is
memorized knowledge versus discovery.

---

## 2. Contracts (replay interfaces)

Every rebuild ships a CLI with exactly these commands. Grading only ever goes through
the CLI; a GUI is required by the task text but is smoke-tested, not scored, in v0.1.

### 2.1 `calc`
```
calc eval   --in book.xlsx --out values.json      # every cell: value, type, error, formatted string
calc recalc --in book.xlsx --mutate ops.json --out values.json   # set cells, then recalc
calc open   --in book.xlsx                        # GUI smoke test (screenshot compared loosely)
```
`values.json` schema: `{sheet: {A1: {v: number|string|bool|null, t: "n"|"s"|"b"|"e", e: "#DIV/0!"?, f: "1,234.50"}}}`.
v0.1 function set: 150 functions (math, stats, text, date/time, lookup incl. XLOOKUP,
logical, IFERROR, SUMIFS family, array formulas, dynamic arrays). Out of scope v0.1:
pivot tables, charts, VBA, external links, data validation.

### 2.2 `raw`
```
raw develop --in photo.dng --edits edits.json --out out.png   # 16-bit PNG, sRGB
raw thumb   --in photo.dng --out thumb.jpg --size 512
```
`edits.json` mirrors a fixed darktable pipeline: exposure, white balance, highlight
reconstruction, demosaic (RCD), filmic/sigmoid tone, color balance (HSL), local
contrast, sharpen, denoise (profiled), lens correction (lensfun params inlined), crop,
rotate. Each key maps 1:1 to a darktable module parameter so the oracle XMP is
generated mechanically. v0.1 inputs are DNG only. v0.2 adds proprietary CR2/NEF/ARW
(true reverse engineering of undocumented formats).

### 2.3 `layers`
```
layers flatten --in file.psd --out out.png                      # composite as the stand-in would
layers apply   --in file.psd --ops ops.json --out out.png       # ops then flatten
layers write   --in file.psd --ops ops.json --out file2.psd     # round-trip, stand-in must open it
```
Ops v0.1: layer opacity/visibility/blend mode (normal, multiply, screen, overlay,
soft/hard light, darken, lighten, difference, color dodge/burn), layer masks, group
layers, adjustment layers (levels, curves, hue/sat, brightness/contrast), filters
(gaussian blur, unsharp mask, median), transforms (scale/rotate bicubic), selections
(rect, ellipse, magic wand with tolerance) used as masks, PatchMatch fill of a masked
region (graded by tolerance, see §4.1). Out of scope v0.1: text layers, smart objects,
layer effects, 32-bit/float documents, CMYK.

### 2.4 `docx`
```
docx render --in doc.docx --out pages/%03d.png --dpi 96   # Liberation fonts pinned
docx text   --in doc.docx --out text.txt                  # paragraphs separated by \n, tables cell\tcell
docx write  --in doc.docx --ops ops.json --out doc2.docx  # edit round-trip (v0.2)
```
v0.1 scope: paragraphs, runs and character formatting, styles and inheritance,
numbering/lists, tables (borders, merges, widths), sections/margins/page size,
headers/footers, footnotes, inline and anchored images, page breaks, tabs. Out of
scope v0.1: fields beyond PAGE/NUMPAGES, track changes rendering, comments, equations,
text boxes, columns, right-to-left scripts.

### 2.5 Dependency whitelist (all tasks)

Standard library of the chosen language, plus: zip/deflate, XML parsing, PNG/JPEG/TIFF
codecs, a windowing layer for the GUI (winit, SDL2 or GLFW; Qt and GTK are not
allowed), SIMD intrinsics and threads. No GPU APIs (§1.3). Not allowed: image-processing libraries (OpenCV,
libvips, ImageMagick, skia), raw decoders (LibRaw, rawspeed), spreadsheet or document
libraries (openpyxl, python-docx, Apache POI, docx4j), layout/text engines (HarfBuzz,
Pango, FreeType — v0.1 rasterizes with a provided bitmap font atlas for `docx`; TrueType
rasterization becomes a v0.2 sub-task).

---

## 3. Ground truth generation (oracle image)

| Task | Source corpus | Generator | Truth files |
|---|---|---|---|
| `calc` | LibreOffice `sc/qa/unit/data/xlsx`, Apache POI `test-data/spreadsheet`, procedurally generated workbooks (seeded formula DAGs with controlled function mix and size up to 1M cells) | `truth_calc.py`: UNO `calculateAll()`, dump every cell | `values.json` per book |
| `raw` | raw.pixls.us (CC0), DNG subset; others converted with `dnglab` | `truth_raw.sh`: generate XMP from `edits.json`, `darktable-cli in.dng edits.xmp out.png` with 16-bit PNG conf | `out.png`, `thumb.jpg` |
| `layers` | psd-tools and Krita test PSDs, plus generated layer stacks (seeded: 2–40 layers, random blend modes, masks, groups) | `truth_layers.py`: GIMP 3 batch (python-fu) applies ops, exports PNG; Krita cross-check flags items where the two stand-ins disagree by >1 LSB (those items are dropped) | `out.png`, `file2.psd` |
| `docx` | LibreOffice `sw/qa/extras/ooxmlexport/data`, Apache POI, python-docx tests, generated docs (seeded structure) | `truth_docx.sh`: `soffice --headless --convert-to pdf`, `pdftoppm -r 96 -png`, `--convert-to txt` | `pages/*.png`, `text.txt` |

Split: 20% public (shipped in the agent image), 80% hidden. Generators are seeded, so
each benchmark release regenerates the hidden split (contamination control, the
REBench/CrackMeBench pattern). The oracle image is pinned by digest; truth is
regenerated only on a version bump and stored with a checksum manifest.

Calibration before launch: run each oracle on two stand-in versions and on x86 vs arm64;
the measured disagreement sets the correctness tolerance (§4.1). Items that disagree
across oracle versions are excluded.

---

## 4. Metrics

Four layers. Efficiency is the headline but only counts above the correctness gate.

### 4.1 Correctness

| Task | Primary metric | Gate for "resolved" | Diagnostics |
|---|---|---|---|
| `calc` | cell match rate: numbers within 1e-9 relative (1e-12 absolute near zero), strings exact, booleans exact, error codes exact; formatted-string match rate separately | ≥ 99.9% cells on hidden set **and** 100% on ≥ 95% of workbooks | per-function pass rate (which of the 150 functions fail), error-propagation accuracy, circular-reference handling |
| `raw` | PSNR and SSIM vs oracle PNG; ΔE2000 in CIELAB (mean and p95) | median PSNR ≥ 40 dB, SSIM ≥ 0.98, p95 ΔE ≤ 2.0 on ≥ 95% of items; thresholds re-set after calibration | per-module ablation (single-module edits) localizes failures; highlight/shadow clipping maps |
| `layers` | pixel match rate within ±1 LSB per channel; PSNR; per-op pass rate | ≥ 99.5% pixels within ±1 LSB on ≥ 95% of items; PatchMatch regions graded by SSIM ≥ 0.9 against oracle and texture-statistics distance | per-blend-mode and per-adjustment pass rate; PSD parse rate (layer count/names/bounds match); round-trip rate (stand-in opens `file2.psd` and flattens to the same image) |
| `docx` | page count match; per-page SSIM against reference render; text exact match (whitespace-normalized) | page count exact on ≥ 90% of docs, mean SSIM ≥ 0.90, text exact ≥ 95% | line-break agreement (word boxes from PDF vs rebuild, IoU), table-cell count, style-resolution accuracy on synthetic docs with known expected properties |

Partial credit is reported as the raw rates; the gate is binary per task.

### 4.2 Efficiency (headline, resolved tasks only)

Workload sets per task, run in the grader with pinned resources:

| Task | Workloads | Primary resource | Baseline |
|---|---|---|---|
| `calc` | full recalc of 10k / 100k / 1M-cell books; 1,000 incremental single-cell edits (p50/p99 latency); 100-book batch | CPU time (user+sys) and incremental latency | LibreOffice headless on the same books |
| `raw` | 24 MP and 61 MP DNGs, 20 images each; thumbnail generation of 500 raws | wall-clock per image at 4 cores, and again at 1 core | `darktable-cli` |
| `layers` | 2 MP / 24 MP / 100 MP documents with 5, 20, 40 layers; each filter at 24 MP | wall-clock per operation per megapixel | GIMP batch |
| `docx` | 10 / 100 / 1,000-page documents; 500-doc batch render; `text` on 10k docs | pages per second; wall-clock per batch | LibreOffice headless |

```
E              = geomean over workloads of (baseline_time / rebuild_time), median of 5 runs each
efficiency     = clamp(0.5 + log2(E) / 6, 0, 1)       # parity 0.5, 8× faster 1.0, 8× slower 0
task_score     = resolved ? efficiency : 0
```
Secondary efficiency metrics, reported not scored in v0.1: peak RSS, binary size,
cold-start time, parallel efficiency (speedup at 1 vs 4 cores), and throughput
scaling slope (log-log fit of time vs input size).

### 4.3 Process metrics (how the agent got there)

Logged by the harness, reported per run: tokens in/out, cost, wall-clock, number of
shell commands, number of computer-use actions, screenshots taken, stand-in features
exercised (menu/dialog titles seen in screenshots, deduplicated), time to first
successful build, number of builds, public-example pass rate over time (learning
curve), generalization gap (public pass rate − hidden pass rate; a large gap means
overfitting to examples), refusals or safety stops, and the condition (§1.5).

Derived leaderboard views: score vs cost Pareto front, score per dollar, and the
`full` − `no-gui` delta per model.

### 4.4 Robustness (reported, gates in v0.2)

- Crash rate on a fuzzed corpus (truncated, corrupted, and adversarial inputs); the
  rebuild must exit non-zero cleanly, never hang or segfault.
- Memory-safety signal: an ASan/UBSan build (when the language supports it) run on the
  public set.
- Determinism across two grader runs and across `--cpus 1` vs `--cpus 4`.
- Resource discipline: no item may exceed 4× the baseline's peak RSS.

### 4.5 Aggregation

```
benchmark_score = mean over the 4 tasks of task_score         # 0..1
resolve_rate    = fraction of tasks resolved
```
Report per task: resolved, correctness rates, E, efficiency, cost, condition. Report
per model: benchmark_score, resolve_rate, cost, and the ablation deltas. Keep raw
`reward.json` for every run so metrics can be recomputed when thresholds change.

### 4.6 Unscored diagnostics

Language chosen, LOC, dependency count, tests written by the agent and their coverage,
whether the agent built a GUI, and a short rubric-free summary of the architecture
(generated from the source tree, for the writeup only).

---

## 5. Build order and budget

1. Oracle image + `truth_calc.py` + `calc` contract + grader (1 weekend). Dry-run the
   grader on LibreOffice's own output to confirm the gate is reachable at 100%.
2. Agent sandbox with LibreOffice GUI on Xvfb + harness loop + `full`/`no-gui`
   conditions; first runs with two models (1 weekend, ≈ $100–300).
3. `raw` and `layers` oracles and contracts; calibration runs on two stand-in versions
   (1 weekend).
4. `docx` oracle and contract; font pinning; line-break diagnostics (1 weekend).
5. Full sweep: 4 tasks × 3 models × 2 conditions ≈ 24 runs; at $50–150 per run,
   $1,200–3,600, plus occasional long "grand" runs at $500+.

## 6. Open questions

- Gate thresholds are provisional until the oracle-vs-oracle calibration is measured.
- Whether to score the GUI at all in v0.2 (scripted screenshot checks of the rebuild).
- Text rendering for `docx`: bitmap atlas in v0.1 vs TrueType rasterization sub-task.
- Whether `spec-only` should get the public examples (currently yes).

## 7. References

MirrorCode (Epoch/METR), ProgramBench, SRE-Bench, CrackMeBench, AgentRE-Bench,
BinaryAudit (Harbor task format), REBench, Anthropic computer-use reference container,
ECMA-376, Adobe PSD and DNG specifications, raw.pixls.us, dnglab, LibreOffice QA
corpora, Apache POI test data.
