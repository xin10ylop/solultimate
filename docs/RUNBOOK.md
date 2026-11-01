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
  ./target/release/recorder --out data/live --firehose
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

## 2. Evaluation (day 30)

The replay is causal: every decision uses only information that existed at that slot, and
every operator's record is selection-honest. So the whole month after the first week is
out-of-sample by construction. Still, tune nothing on the last 10 days:

1. Tune on days 1–20 only (gate thresholds, variant family, dump thresholds).
   `lab replay --to-slot <day20>` with different `--config` files.
2. Freeze the config. Run days 21–30 once: `lab replay --from-slot 0` (the warm-up must
   include the earlier days, so replay everything and read only the day 21–30 rows).
3. Stress the landing model: `--stress 1` (more next-slot, deeper queue) and `--stress 2`
   (always a full slot late).

**Go-live gate. All of these must hold on days 21–30 with the frozen config:**

| check | threshold |
|---|---|
| real (gated) trades | ≥ 60 |
| mean return per trade after all costs | > +4% |
| bootstrap 95% lower bound of mean return | > 0 |
| profitable days | ≥ 7 of 10 |
| largest single day's share of total PnL | < 35% |
| largest single operator's share of PnL | < 30% |
| `--stress 1` mean return | > 0 |
| `--stress 2` mean return | > −2% (it shows how fast the edge dies if you are slow) |
| max drawdown | < 25% of the bankroll used in the replay |

If any line fails, the honest answer is "no edge yet". Record another 2 weeks (the
operator population changes) or change the hypothesis. Don't loosen thresholds.

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
