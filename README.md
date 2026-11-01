# solultimate

A Rust engine for pump.fun on Solana. It learns which launch **operators** (serial devs and
their wallet networks) and which launch **types** can be followed profitably *with your
real latency*, and it trades only those. It does not use Twitter, KOLs, alpha groups or any
manual curation.

```
             ┌─────────────── same code in backtest and live ───────────────┐
Yellowstone  │  decode → MintTracker ─┬─ shadow positions (18 variants,     │
gRPC  ───────┤  (pump events, SOL     │   every launch, measured latency)   │
(or RPC      │   fundings, token      ├─ OperatorBook (identity graph +     │
 backfill)   │   transfers)           │   selection-honest track records)   │
             │                        ├─ ContextBook (launch types)         │
             │                        └─ Gate (shrunk LCB) → real position  │
             └──────────────────────────────────────────┬───────────────────┘
                                                        │ intents
                                     pump v3 tx builder → Nozomi / RPC senders
```

* **[docs/STRATEGY.md](docs/STRATEGY.md)**: the logic, what was taken from the reference
  videos and what is new, the math, and the pilot results on real mainnet data.
* **[docs/RUNBOOK.md](docs/RUNBOOK.md)**: the phases (30-day recording → evaluation gate →
  paper → canary → scale), the go-live thresholds and the safety controls.

## Layout

| path | what |
|---|---|
| `crates/ucore` | pure logic: pump.fun decoding, curve math, event store, latency/fill model, mint tracker + dump detectors, exit policies, operator book, brain, tx builder |
| `crates/unode/src/bin/recorder.rs` | 24/7 Yellowstone recorder → event store (feature `grpc`) |
| `crates/unode/src/bin/backfill.rs` | historical blocks over JSON-RPC → event store |
| `crates/unode/src/bin/lab.rs` | `check` (decoder health), `replay` (causal backtest + reports + go-live checklist), `compare` (live vs model) |
| `crates/unode/src/bin/simcheck.rs` | simulates our buy/sell transactions on mainnet against the live program |
| `crates/unode/src/bin/trader.rs` | live trader: paper / live, kill switch, risk limits (feature `grpc`) |
| `config/` | `trader.example.toml`, `brain.default.toml` (every tunable, with defaults) |

## Quick start

```bash
cargo build --release -p unode --features grpc
cargo test  --release -p ucore

# pull a few hours of history from a public RPC and replay it
./target/release/backfill --rpc https://solana-rpc.publicnode.com --last 9000 --out data/pilot
./target/release/lab check  --data data/pilot
./target/release/lab replay --data data/pilot --out reports/pilot
cat reports/pilot/summary.md        # plus launches.csv (play sheet) and trades.csv

# verify the transaction layer against the current program
./target/release/simcheck --rpc https://api.mainnet-beta.solana.com
```

## Honest status

* Decoder, curve math and transaction layer are verified against mainnet. The math matches the
  program to the lamport, curve continuity holds on 99.8% of non-mayhem trades, and simulated
  `buy_exact_quote_in_v3` / `sell_v3` both succeed.
* The strategy is a hypothesis machine with a strict gate. It is **not** proven profitable.
  A ~2-day public-RPC pilot is not a month of data. Go live only after the runbook's
  evaluation gate passes on your own 30-day recording, and then only at canary size.
* Memecoin trading can lose everything you put in the hot wallet. Keep the treasury elsewhere.
