#!/usr/bin/env bash
# Continuous replay of G2 / G3a / G3b over the contiguous development epochs available (1058..1064),
# first epoch = warm-up, per-UTC-day scoring.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
D=data/dev_all
rm -rf "$D"; mkdir -p "$D"
first=""; last=""
for E in 1058 1059 1060 1061 1062 1063 1064; do
  if [ -f data/hist/e$E/DONE ]; then
    [ -z "$first" ] && first=$E
    last=$E
    for f in data/hist/e$E/ev_*.zst; do ln -s "$ROOT/$f" "$D/"; done
  elif [ -n "$first" ]; then break; fi
done
lo=$(( (first + 1) * 432000 )); hi=$(( (last + 1) * 432000 ))
echo "development epochs ${first}..${last} (warm-up ${first}); scoring slots [$lo, $hi)"
for C in ${CONFIGS:-G2 G3a G3b}; do
  ./target/release/lab replay --data "$D" --out reports/dev_$C --config config/eval/$C.toml --lse --live-costs --late-sells > reports/dev_$C.log 2>&1 &
done
wait
for C in ${CONFIGS:-G2 G3a G3b}; do
  echo "=================== $C (development days, never replayed before)"
  python3 research/archive/score_days.py reports/dev_$C/trades.csv research/archive/slot_times.csv $lo $hi | grep -vE "^Empty|^Columns|^Index"
done
