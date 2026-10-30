#!/bin/bash
# Generate a fresh evaluation set with random seeds AFTER agent runs finish, so no agent could have seen it.
# usage: harness/make_evalset.sh <name>   -> data/calc/evalsets/<name>/{hidden_books,hidden_truth,perf_books,perf_truth,baseline_*.json,seeds.txt}
set -e
RB=$(cd "$(dirname "$0")/.." && pwd); N=$1; E=$RB/data/calc/evalsets/$N
[ -z "$N" ] && { echo "usage: $0 <name>"; exit 2; }; [ -e "$E" ] && { echo "$E exists"; exit 1; }
mkdir -p "$E"
S1=$(( (RANDOM << 15 | RANDOM) % 900000 + 1000 )); S2=$(( (RANDOM << 15 | RANDOM) % 900000 + 1000 ))
echo "hidden_seed=$S1 perf_seed=$S2 created=$(date -u +%FT%TZ)" > "$E/seeds.txt"
docker run --rm --network none --cpus 4 --cpuset-cpus 0-3 --memory 6g -v "$RB/tasks/calc/oracle:/src:ro" -v "$RB/grader:/g:ro" -v "$E:/e" rb-oracle:latest bash -c "
set -e; cp /g/xlsxio.py /src/truth.py /src/baseline.py /tmp/
python3 /src/gen.py split --out /e/hidden_books --seed $S1 --count 160
python3 /src/gen.py perf --out /e/perf_books --seed $S2
python3 /tmp/truth.py --books /e/hidden_books --out /e/hidden_truth --check | tr -d '\n '; echo
python3 /tmp/truth.py --books /e/perf_books --out /e/perf_truth --check | tr -d '\n '; echo
python3 /tmp/baseline.py --books /e/perf_books --runs 5 --out /e/baseline_4core.json >/dev/null
taskset -c 0 python3 /tmp/baseline.py --books /e/perf_books --runs 3 --out /e/baseline_1core.json >/dev/null
"
python3 -c "import json; b=json.load(open('$E/baseline_4core.json')); print({k: round(v['total_s'],3) for k,v in b.items()})"
