# Runbook: from zero to scaled, without skipping the evidence

The engine refuses to be trusted on vibes. Every phase has an exit criterion. If a phase
fails its criterion, you fix the cause and repeat that phase; you never skip forward.

## 0. Build and self-checks (Frankfurt box)

```bash
cargo build --release -p unode --features grpc
cargo test --release -p ucore
```

Validate the transaction layer against the current pump.fun program from your own RPC.
It simulates a real `buy_exact_quote_in_v3` and `sell_v3` (sigVerify off, real funded wallets as
payers) and compares the program's numbers with our curve quote:

```bash
./target/release/simcheck --rpc "$RPC_URL"
```

Expect `err: null` for both and a quote error near 0 bps. **Re-run it daily and after any
pump.fun announcement**: the program has changed its trade interface several times in 2025–26.

## 1. Data phase: 30 days of recording (days 1–30)

```bash
GRPC_ENDPOINT=https://... GRPC_X_TOKEN=... \
  ./target/release/recorder --out data/live --firehose --amm   # --amm: also PumpSwap (graduations, boost)
```

* `--firehose` streams every non-vote transaction (~40–80 Mbit/s). It is what captures the
  fresh-wallet fundings (exchange hot wallet → dev, mother → dev, dev → next dev). Without
  it, operator attribution falls back to dev-wallet reuse and recurring insiders only.
* Storage is ~0.9 GB/day (zstd). Keep everything.
* Run it under systemd with `Restart=always`. A crash loses at most ~2 s of buffered events;
  segment files rotate hourly. Gaps can be filled later with `backfill --start --end`.

Every day:

```bash
./target/release/lab check  --data data/live              # decoder health
./target/release/lab replay --data data/live --out reports/$(date +%F)
```

`lab check` must show curve continuity ≥ 99.5% on non-mayhem coins. A drop means the program
changed its event layout: stop and update `ucore/src/pump.rs` from the new IDL.

## 2. Evaluation (day 30): is there a *consistent* edge?

The primary engine is the identity-free flow model (docs/STRATEGY.md §1). Evaluate it day
by day, out of sample:

```bash
python3 -m venv .venv && .venv/bin/pip install numpy pandas scikit-learn lightgbm

# 1) decision points + latency-honest labels for the whole month
./target/release/lab features --data data/live --out data/research/flow.csv \
    --max-age-s 3600 --min-progress 0.3 --max-progress 0.97 --stake 500000000
#    (repeat with --min-progress 0 --max-age-s 600 for the early-life regime)

# 2) walk-forward: train on the trailing 14 days, trade the next day, for every day
.venv/bin/python research/walkforward.py data/research/flow.csv --period 86400 \
    --train-periods 14 --label h60 --stake 0.5
#    also --label h30 / h15, and with features exported with --stress 1
```

**Go-live gate. All of these must hold on the walk-forward days (≥ 14 traded days):**

| check | threshold |
|---|---|
| profitable days | ≥ 70% |
| mean return per trade after all costs | > +2% |
| bootstrap 95% lower bound of mean return per trade | > 0 |
| best single day's share of total PnL | < 25% |
| same test with features exported at `--stress 1` | mean > 0 |
| trades per day | enough to matter (≥ 10) |

If it fails, the honest answer is "no consistent edge at this latency and cost". Do not
lower thresholds. Change something structural (stake size, holding window, regime) and
re-test on *new* days.

**Production model.** Train on the latest 14 days and save the model:

```bash
.venv/bin/python research/flow_model.py data/research/flow.csv --label h60 \
    --train-frac 1.0 --save research/models/flow.json
```

Then set `[brain.flow] enabled = true`, `model_path`, `label`, and a `threshold` equal to
the walk-forward's training-quantile threshold. Retrain daily (cron) and keep the previous
model if the new one fails the walk-forward check on the latest day. Check it in-engine with
the exact live code path:

```bash
./target/release/lab replay --data data/live --out reports/flow \
    --flow-model research/models/flow.json --flow-label h60 --flow-threshold 0.03
```

Only read trades *after* the model's training window. Trades inside the training window
look spectacular and mean nothing: on the pilot they showed +7–16%/trade in-sample and
≈0% held-out.

## 3. Paper trading (3–7 days)

```bash
cp config/trader.example.toml config/trader.toml   # mode = "paper"
./target/release/trader --config config/trader.toml
```

Paper mode runs the full live path (processed gRPC, same decoder, same brain, warm-started
from `data/live`), but fills come from the landing model and nothing is sent. Compare its
journal with a replay of the same hours recorded by the recorder: they must agree, apart from
processed-vs-confirmed differences. This validates the live data path.

## 4. Canary live (until ≥ 50 real trades)

* `mode = "live"`, `stake_lamports = 100_000_000` (0.1 SOL), `max_open = 1`,
  `daily_loss_limit_lamports = 300_000_000`, wallet funded with ≤ 3 SOL.
* At 0.1 SOL the fixed costs (tips + priority + base fees, both legs) are ~2.6% of the stake
  and the curve fees another ~2.5%. That is why the canary is judged on **fill quality vs
  model**, not on PnL:

```bash
./target/release/lab compare --journal data/journal.jsonl
```

For each real trade, it prints the model's shadow result for the same launch and variant. A
persistent negative gap (real < model by more than ~2% per trade) means our landing model is
optimistic. Fix `latency` in the config with the measured distribution (slot offset and
trades ahead of us), re-run the evaluation, and only then continue.

## 5. Scale ladder

Double the stake only after **100 more live trades** in which:

* the live mean return is ≥ 70% of the replay's mean for the same period;
* the live − model gap is within ±2%;
* the drawdown stays inside the daily limits.

Cap the stake at ~3% of the operator's typical create-slot (bundle) SOL. Above that you are
a visible share of their exit liquidity, and ruggers react to visible snipers ("crumbs, not
the cake").

## Safety controls (all on by default)

| control | where |
|---|---|
| kill switch: `touch STOP` → sell all, block entries, exit when flat | trader `kill_file` |
| refuse to start live if the hot wallet holds > N SOL | `max_wallet_lamports` |
| keypair must be chmod 600; never logged | trader |
| no new entries while the feed lags | `max_stream_lag_ms` |
| daily loss limit → no entries until UTC midnight | `risk.daily_loss_limit_lamports` |
| loss streak → pause for N slots | `risk.max_consec_losses`, `pause_slots_after_streak` |
| max open positions, one per operator | `risk.max_open` |
| entry price cap from the operator's own first-candle history (anti-farm) | `gate.first_candle_mult` |
| mayhem / non-SOL quote / cashback / depth>0 coins never traded | brain |
| buy not seen in N slots → failed; if it lands later it is sold immediately | trader orphan logic |
| curve completion while holding → immediate exit | policy `migrated` |

Keep the main treasury in a different wallet (ideally hardware). The hot wallet holds only
what the current ladder step needs.

## G2 paper trading (current candidate)

```
./target/release/trader --config config/trader.G2.paper.toml      # mode = "paper": nothing is sent
./target/release/recorder --out data/live --firehose --amm        # in parallel, for replays and the per-day test
```
Judge it per UTC day with `research/follow_money/score_periods.py` on the trader journal or on a
replay of the recorded day (`lab replay --config config/eval/G2.toml --lse --live-costs --late-sells`).
Go to canary (0.1 SOL, 1 open position) only after ≥ 14 days with ≥ 70% of days profitable at
racing fees and positive without the best 2 trades.

## Historical days without a box: the ledger archive

Every past epoch (432,000 slots ≈ 26 h) is published as one CAR file at
`https://files.old-faithful.net/<epoch>/epoch-<epoch>.car` (300–650 GB). `carstream` streams one over
parallel HTTP range requests and decodes it with the same `decode_tx` as the recorder, so the output
is an ordinary event store for `lab replay` / `lab signals`.

```bash
cargo build --release --features grpc --bin carstream --bin evdiff
./target/release/carstream --epoch 1071 --out data/hist/e1071 --slim --resume --conns 48 --chunk-mb 16
deploy/hist_loop.sh "1072 1071 1070"          # several epochs, newest first, resumable
./target/release/evdiff data/hist/e1073 data/pilot2 463536000 463537500   # parity vs RPC data
```

* ~180 MB/s on 4 cores (CPU-bound; two loops on disjoint epochs reach ~250 MB/s), ~1 GB of
  events per epoch with `--slim` (funding / transfer signatures dropped; the brain never reads them).
* Handles the v1 transaction layout (version byte 0x81, signatures last) and split data frames.
  There are no vote transactions in recent epochs.
* `research/archive/run_replay.sh` replays G / G2 over the contiguous imported epochs plus all
  recorded data and scores each UTC day (`research/archive/score_days.py`).
