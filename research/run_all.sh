#!/usr/bin/env bash
# Full evaluation in one go.
#   research/run_all.sh <data_dir> <out_dir> [period_seconds=86400] [python=.venv/bin/python]
# On 30 days of recording use the default daily period; on a short pilot use 3600.
set -euo pipefail
DATA=${1:?data dir}; OUT=${2:?out dir}; PERIOD=${3:-86400}; PY=${4:-.venv/bin/python}
BIN=./target/release
mkdir -p "$OUT"
$BIN/lab check  --data "$DATA" | tee "$OUT/check.txt"
$BIN/lab replay --data "$DATA" --out "$OUT/replay" > /dev/null
for regime in early stretch; do
  if [ $regime = early ]; then ARGS="--max-age-s 600 --min-progress 0 --max-progress 0.85"; else ARGS="--max-age-s 3600 --min-progress 0.3 --max-progress 0.97"; fi
  for stake in 250000000 500000000; do
    for stress in 0 1; do
      f="$OUT/flow_${regime}_${stake}_s${stress}.csv"
      $BIN/lab features --data "$DATA" --out "$f" $ARGS --stake $stake --stress $stress
      sol=$(python3 -c "print($stake/1e9)")
      for label in h15 h30 h60; do
        echo "### regime=$regime stake=$sol stress=$stress label=$label" | tee -a "$OUT/walkforward.txt"
        $PY research/walkforward.py "$f" --period "$PERIOD" --label $label --stake "$sol" \
            --train-periods 14 --min-train-periods 3 2>/dev/null | tail -3 | tee -a "$OUT/walkforward.txt"
      done
    done
  done
done
echo "done: $OUT/walkforward.txt, $OUT/replay/summary.md"
