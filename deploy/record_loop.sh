#!/usr/bin/env bash
# Continuous recorder over public RPC: follows the finalized tip in chunks (pump curve + PumpSwap).
# Usage: deploy/record_loop.sh <out_dir> [rpc]
set -u
OUT=${1:-data/live_amm}
RPC=${2:-https://solana-rpc.publicnode.com}
BIN=$(dirname "$0")/../target/release/backfill
mkdir -p "$OUT"
fin() { curl -s -m 20 -X POST "$RPC" -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"finalized"}]}' | python3 -c "import sys,json;print(json.load(sys.stdin)['result'])" 2>/dev/null; }
START=$(cat "$OUT/next_slot.txt" 2>/dev/null || true)
if [ -z "$START" ]; then F=$(fin); START=$((F - 50)); fi
while true; do
  F=$(fin)
  if [ -z "$F" ]; then sleep 20; continue; fi
  END=$((F - 10))
  if [ $((END - START)) -ge 2500 ]; then
    if "$BIN" --rpc "$RPC" --start "$START" --end "$END" --out "$OUT" --concurrency 16 ${AMM---amm} >> "$OUT/record.log" 2>&1; then
      START=$END
      echo "$START" > "$OUT/next_slot.txt"
      echo "$(date -u +%FT%TZ) recorded up to $END" >> "$OUT/progress.log"
    else
      sleep 30
    fi
  else
    sleep 60
  fi
done
