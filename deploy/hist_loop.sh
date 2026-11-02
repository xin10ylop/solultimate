#!/usr/bin/env bash
# Import archived epochs (Old Faithful) newest → oldest into data/hist/e<epoch>, resumable.
# Usage: deploy/hist_loop.sh "<epochs...>" [to_slot_for_first]
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=$ROOT/target/release/carstream
EPOCHS=${1:-"1072 1071 1070 1069 1068 1067 1066 1065 1064 1063 1062 1061 1060 1059 1058"}
FIRST_TO=${2:-463424017}
mkdir -p "$ROOT/data/hist"
first=1
for E in $EPOCHS; do
  OUT=$ROOT/data/hist/e$E
  TO=18446744073709551615
  if [ $first = 1 ]; then TO=$FIRST_TO; fi
  first=0
  while [ ! -f "$OUT/DONE" ]; do
    echo "$(date -u +%FT%TZ) epoch $E start" >> "$ROOT/data/hist/progress.log"
    "$BIN" --epoch "$E" --out "$OUT" --slim --resume --to-slot "$TO" --conns 24 >> "$OUT.log" 2>&1
    if [ -f "$OUT/DONE" ]; then
      echo "$(date -u +%FT%TZ) epoch $E done $(cat "$OUT/DONE")" >> "$ROOT/data/hist/progress.log"
    else
      echo "$(date -u +%FT%TZ) epoch $E exited without DONE; retrying" >> "$ROOT/data/hist/progress.log"
      rm -f "$OUT/PARTIAL"
      sleep 20
    fi
  done
done
echo "$(date -u +%FT%TZ) all done" >> "$ROOT/data/hist/progress.log"
