# REbench

A reverse-engineering benchmark for AI agents. The agent gets a working application to probe and a time limit,
and has to rebuild the application's behavior from scratch. A separate grader checks the rebuild on hidden inputs:
exact results first, then speed against the original.

The first task, `calc`, asks for the formula engine of LibreOffice Calc 25.2 in 60 minutes.

- **What the agent gets:** a Linux sandbox with no internet, LibreOffice Calc (GUI and command line), 40 example
  workbooks with LibreOffice's answers, and a [contract](tasks/calc/agent/CONTRACT.md) listing 87 functions and the
  output format.
- **What it must build:** a program, `./calc eval --in book.xlsx --out values.json`, that computes every formula in
  a workbook. It must build from source, with no spreadsheet libraries.
- **How it is graded:** in a clean container without LibreOffice, on 160 hidden workbooks generated after every run
  in the round has finished, plus four large workloads timed against LibreOffice.

## Results

Four models, three attempts each (pilot, sealed and Rust-only rounds), all graded on the same evaluation set,
`rust1` (160 workbooks, 9,487 formula cells).

| Model | Speed vs LibreOffice, median | Range over 3 attempts | Passed the correctness gate |
|---|---|---|---|
| Opus 5.5 | 27.4x | 16.8–29.1x | 3 of 3 |
| Sonnet 5.5 | 17.3x | 12.3–20.0x | 3 of 3 |
| Fable 5.1 | 10.6x | 10.2–17.7x | 2 of 3 |
| Haiku 5.5 | 1.2x | 0.58–1.52x | 1 of 3 |

Speed is the geometric mean over the four workloads, defined under [Scoring](#scoring). Per-attempt numbers are in
[runs/RESULTS.md](runs/RESULTS.md), and the full analysis is [runs/results.html](runs/results.html) (open it in a
browser). API-equivalent cost per attempt in the sealed and Rust rounds: Opus $20–26, Sonnet $11–12, Fable $29–44,
Haiku about $3.

What the runs show:

- **One attempt is not enough to rank models.** A model's attempts differ by up to 1.7x, so single attempts can put
  Opus, Sonnet and Fable in any order. Across three attempts each, Opus beats Sonnet in 7 of 9 pairings.
- **Correctness came quickly; the rest of the hour went to speed.** Every run with a full transcript passed the
  public examples by minute 18. Opus, Sonnet and Fable then made their engines 1.3 to 6 times faster by their own
  measurements. Haiku stopped 8 to 10 minutes after passing. No run used the whole hour.
- **The remaining errors are LibreOffice conventions, not bugs.** Of 142 wrong cells over 24 gradings, 136 come from
  five behaviors: which error code wins, how numbers are formatted as text, criteria built from an empty cell, text
  and booleans used as numbers, and MIN over booleans. Most engines miss the same cells, and the gate allows at most
  9 wrong cells, so pass or fail can flip between test sets.
- **Speed came from two data structures more than from the language.** The fast engines build a hash index for
  exact lookups and cache which rows match each SUMIFS/COUNTIFS condition. Sonnet's two Rust engines differ by 1.6x,
  mostly on lookups, where only the faster one has the index. Haiku's Rust engine was 5 to 8 times faster than its
  Python one on two workloads but dropped both structures, so overall it stayed near LibreOffice's speed.

## How a run works

1. `harness/run.py` starts the agent sandbox (`docker/agent-calc.Dockerfile`) and runs Claude Code headless
   (`claude -p`) with no built-in tools. The agent works only through the sandbox tools in `harness/sandbox_mcp.py`:
   shell, file read, write and edit, and screenshot, click, type, key, scroll and drag for the GUI. Tool calls go to
   `tools.jsonl` and the full transcript to `claude_stream.jsonl`. If the agent stops early, the harness tells it how
   many minutes are left, up to four times.
2. After every run in the round has finished, `harness/make_evalset.sh <name>` generates a new evaluation set from
   random seeds in the oracle image: 160 workbooks, four performance workloads, LibreOffice's answers and its timings.
3. `harness/finish_run.py <run> --grade-only --evalset <name>` builds and scores the submission in the grader image:
   no network, 4 pinned cores, 6 GB, an unprivileged user, and answers readable only by root. Prebuilt binaries and
   spreadsheet libraries are rejected.
4. `harness/report.py` writes `runs/RESULTS.md`, and `harness/results_page.py` builds the analysis page.

The pilot round ran differently: Claude Code subagents limited to the sandbox by instruction, with every transcript
audited afterwards (`harness/prepare_run.py`, `harness/finish_run.py --transcript`). Their tool-call logs are
included; their transcripts are not.

## Scoring

- **Correctness gate:** at least 99.9% of hidden formula cells and at least 95% of hidden workbooks fully correct.
  Numbers must match within a relative tolerance of 1e-9; text, booleans and error codes must match exactly.
- **Speed (E):** the geometric mean of the rebuild's speedup over LibreOffice on four workloads: a 60k-formula
  dependency chain, 10k mixed formulas, 5k exact lookups over 20k rows, and 1k SUMIFS/COUNTIFS/AVERAGEIFS over 50k
  rows. A workload under 99.9% correct counts as 1/8. LibreOffice is timed as a warm process (load plus full
  recalculation, startup excluded). The rebuild is timed as its whole `./calc eval` process, median of 5 runs.
- **Score:** 0.5 + log2(E)/6, clipped to 0–1, if the gate passes, otherwise 0. Parity scores 0.5 and 8x scores 1.0.
  Three of the four models are past 8x, so the results above report E.

## Repository layout

| Path | Contents |
|---|---|
| `tasks/calc/agent/` | What the agent sees: `CONTRACT.md`, the `lo-recalc`, `xlsx-values` and `check-public` tools, and the public examples |
| `tasks/calc/oracle/` | The workbook generator (`gen.py`) and the scripts that record LibreOffice's answers and timings |
| `docker/` | Images: shared toolchain, agent sandbox, grader, oracle |
| `grader/` | Builds and scores a submission |
| `harness/` | Runs attempts, makes evaluation sets, grades, reports |
| `data/calc/` | Public examples and every evaluation set used: seeds, workbooks, LibreOffice's answers and timings |
| `runs/` | One folder per attempt: prompt, submitted source, tool log, transcript, and `grade-<set>/` with `reward.json` and every wrong cell |

## Running it

You need Docker with at least 4 CPUs and 6 GB for containers (the runs used Colima on an Apple M4), Python 3, and the
Claude Code CLI, logged in (the runs used version 2.1.312).

```bash
python3 -m venv .venv && .venv/bin/pip install mcp
docker build -f docker/toolchain.Dockerfile -t rb-toolchain .
docker build -f docker/agent-calc.Dockerfile -t rb-agent-calc .
docker build -f docker/grader.Dockerfile -t rb-grader .
docker build -f docker/oracle.Dockerfile -t rb-oracle .
.venv/bin/python harness/run.py --model claude-opus-5-5 --condition full --minutes 60 --run-id sealed-opus-2 --defer-grade
harness/make_evalset.sh my-set
.venv/bin/python harness/finish_run.py sealed-opus-2 --grade-only --evalset my-set
.venv/bin/python harness/report.py
```

Add `--lang rust` to `run.py` for the Rust-only condition. `report.py` collects run folders whose names start with
`pilot-`, `sealed-` or `rust-`. Any existing attempt can be regraded on any set in `data/calc/evalsets/` the same way.

## Status

Only `calc` is built. [DESIGN.md](DESIGN.md) also describes `raw` (darktable), `layers` (Krita and GIMP) and `docx`
(LibreOffice Writer), which do not exist yet.

## License

[MIT](LICENSE)
