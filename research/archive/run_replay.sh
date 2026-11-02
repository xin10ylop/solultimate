#!/usr/bin/env bash
# Continuous replay of frozen G / G2 over the contiguous run of imported archive epochs (newest
# backwards from 1072) plus all previously recorded data, then per-UTC-day scoring of archive days.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
D=data/hist_all
rm -rf "$D"; mkdir -p "$D"
first=""
for E in 1072 1071 1070 1069 1068 1067 1066 1065 1064 1063 1062 1061 1060 1059 1058; do
  [ -f data/hist/e$E/DONE ] || break
  first=$E
  for f in data/hist/e$E/ev_*.zst; do ln -s "$ROOT/$f" "$D/"; done
done
[ -z "$first" ] && { echo "no complete epoch yet"; exit 1; }
for f in data/all/ev_*.zst; do ln -sf "$(readlink -f "$f")" "$D/"; done
# score from the second imported epoch on (first = warm-up), up to the end of the archive
lo=$(( (first + 1) * 432000 )); hi=463424017
[ "$first" = 1072 ] && lo=$(( 1072 * 432000 + 216000 ))   # only one epoch: half of it as warm-up
echo "contiguous archive epochs ${first}..1072; scoring slots [$lo, $hi)"
for C in G2 G; do
  ./target/release/lab replay --data "$D" --out reports/arch_$C --config config/eval/$C.toml --lse --live-costs --late-sells > reports/arch_$C.log 2>&1 &
done
wait
for C in G2 G; do
  echo "=================== $C (archive days, never seen)"
  python3 research/archive/score_days.py reports/arch_$C/trades.csv research/archive/slot_times.csv $lo $hi
done
