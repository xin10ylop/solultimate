# Strategy

## The requirement

The edge must be **consistent**. That rules out a play that works for a week and dies when
an operator rotates wallets, and it rules out copy trading and anything social. So the
primary engine is **identity-free**. It uses only the shape of each coin's order flow, which
every launch has, and it never asks *who* is trading. Nothing in it depends on a wallet list
that can go stale.

The engine trades only what survives a walk-forward test: retrain on trailing days, trade the
next day, and be profitable on most days, under the measured landing latency and every cost.

## 1. Identity-free flow engine (primary)

**Signal.** At every trade of a young coin (configurable age and curve-progress window, ≤1
evaluation per coin per second), `ucore::flow::features` computes 31 features. The groups:

| group | features |
|---|---|
| state | age, market cap, curve progress, return since create, drawdown from peak |
| momentum | log return over 2 / 5 / 15 / 60 s |
| flow | net SOL over 2 / 5 / 15 / 60 s; buys and sells over 5 / 15 s; unique buyers over 15 / 60 s; largest and average buy over 15 s |
| exits | share of sell volume from *flippers* (wallets whose first buy was < 10 s ago: the sniper sell-off) |
| concentration | top-1 / top-5 holder share, create-slot insider share still held, dev's remaining fraction |
| launch shape | create-slot SOL, dev buy, trade count, buy fraction |

None of these is a wallet identity. They describe structure (FOMO, sniper exits,
concentration) that exists on every coin, every day.

**Labels.** `ucore::flowsim` runs a real round trip from every decision point: the landing
model (same/next slot, 2–13 trades ahead, Jito bundles atomic), exact curve math, price
limits, and fixed costs kept separate so any stake size can be evaluated. Exit rules:
`h5` (5 s), `h15` (TP 25% / SL 15% / 15 s), `h30` (TP 50% / SL 20% / trail / 30 s),
`h60` (TP 100% / SL 30% / trail / 60 s).

**Model.** `research/flow_model.py` trains a heavily regularized LightGBM (Huber loss on
clipped net returns, 300 trees, ≥300 samples per leaf). `ucore::gbm` evaluates the dumped
trees in Rust in microseconds, matching Python to 4e-7. In the brain, `Entry::Flow`
enters when the predicted net return clears `flow.threshold`, with the same landing, exit
and risk machinery as everything else.

**Consistency test.** `research/walkforward.py` retrains for every period (a day on real
data) on the trailing N periods, picks the threshold on training data only, and trades
the next period with one trade per coin and at most `max_open` concurrent. The go-live
criterion is the share of profitable days and the concentration of PnL, not the total.

## 2. What the pilot data says

Full results: **[PILOT.md](PILOT.md)** (25.6 h of mainnet, every cost and the measured
latency included, walk-forward by hour). In short:

* No idea tested is consistently profitable yet. The best cells are positive only at the
  most selective threshold, on ≤ 50% of hours, with one or two hours carrying the PnL.
* The most promising lead is final-stretch coins (30–97% curve progress), 30–60 s holds,
  **with wallet-derived (non-copy) features**: funding kind, amount and delay, launch
  linkage, and the operator's past outcomes. These lift the 60 s hold from −7.4% to
  +8.5% per trade at the top 2% of scores.
* Big-bundle "edges" and in-sample model results are illusions. Mayhem coins bleed.

## 1b. Wallet intelligence without copy trading

The model may know *who* is behind a launch, but it never copies anyone's buys. The
wallet-derived features (`how_linked`, `fund_kind`, `fund_sol`, `fund_age_s`,
`op_created_before`, `op_resolved`, `op_mean_ln_peak`, `op_frac_2x`) describe the *type*
of operator and setup. A new operator with the same funding pattern and history scores the
same as the old one, so the model does not decay when wallets rotate. These features are
computed identically in the brain and in the research simulator, from pre-launch facts
only; an operator's outcomes become known 10 minutes after each launch.

## 3. Execution realism (applies to everything)

* **Landing model** (`ucore/src/fill.rs`), from your 15 measured landings: 7/15 same slot
  (2–13 coin trades ahead, median 4), 8/15 next slot (usually first), never first behind
  the trigger. The trigger's contiguous transaction group (Jito bundle) always executes
  before us. Stress levels 1–2 for robustness.
* **Slots are ~211 ms** (Nov 2026; `lab check` measures it). Every time setting is written
  in seconds.
* **Exact program math.** Buy/sell quotes match the program to the lamport. Simulated
  `buy_exact_quote_in_v3` / `sell_v3` succeed on mainnet.
* **Costs:** base 5,000 lamports; priority 120k CU × 3 lamports (buy) and 100k × 2 (sell);
  Nozomi tip 0.001 SOL per leg; curve fees as charged; price-limit refusals cost the
  priority fee only.

## 4. Optional research layers (off for trading by default)

These remain in the engine because they generate research data (the play sheets), but they
do not trade unless switched on (`operator_entries`, `context_entries`):

* **Operator book.** An identity graph built only from pre-launch facts: dev reuse,
  funding (two hops), and the `(exchange hot wallet, exact amount)` signature. For example,
  it found 18 dev wallets each funded with exactly 5.000 SOL from one exchange hot wallet,
  verified on-chain. These records are selection-honest, but by nature they rotate.
* **Context cohorts.** Launch types (dev buy × bundle × crowd × funding kind) with a strict gate.
* **Lifecycle shadows** at create, first-candle close, first dev sell and after the first dump.

## 5. Risk (applies to everything)

Stake ≤ min(config, 5% of bankroll); max open positions; one position per coin; daily loss
limit; pause after a loss streak; mayhem / non-SOL / cashback / depth>0 coins excluded;
kill switch; stream-lag guard; hot-wallet balance cap; orphan and leftover handling;
rent reclaim. See RUNBOOK.md.
