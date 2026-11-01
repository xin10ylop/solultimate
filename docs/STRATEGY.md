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

## 2. What the pilot data says (real mainnet blocks, Nov 2026)

| hypothesis (identity-free unless marked) | result on pilot |
|---|---|
| Buy every launch at create / first-candle close | −4% to −6% per trade (≈ the round-trip cost): no edge |
| "Big bundle" launches | looked +64%/trade until bundles were modelled as atomic; −3.6% after. **Fake edge from optimistic fills** |
| Flow model, coins 2 s–10 min old | best held-out cells −2% to −5%/trade: no edge |
| Flow model, "final stretch" coins (30–97% progress, ≤1 h old) | held-out ≈ break-even (+0.2% … +1%/trade at 0.1 SOL). The model improves on the −3.5% baseline by ~3.5 pts, but not past costs |
| Same model, *in-sample* hours | +7% … +16%/trade, 57–68% win rate. **This is what overfitting looks like**; it vanishes out of sample |
| Mayhem-mode coins (pump.fun's agent rewrites reserves) | agent jumps are unpredictable (autocorr −0.02) with **negative drift −1% to −3% per jump**: avoid |
| Clones of hot tickers (on-chain narrative) | no better than other launches |
| Serial operators (identity-based, off by default) | a few strong records (+9% … +26%/trade, n = 7–41), none statistically proven in hours of data, and they rotate |

The honest summary is that **no strategy is proven profitable yet.** The most promising
identity-free regime (final stretch, 30–60 s holds) is at break-even after all costs on
hours of data. Two levers decide it, and both need the 30-day recording:

1. **More data.** A 31-feature model needs weeks, not hours.
2. **Stake size vs fixed costs.** Tips + priority + base fees are ~0.0026 SOL per round
   trip: 2.6% of a 0.1 SOL stake but 0.5% of 0.5 SOL. Curve fees (0.95% + 0.30% per side)
   are the floor at any size. The pilot's break-even at 0.1 SOL is roughly +2% at 0.5 SOL,
   if price impact stays small (the simulator includes our own impact).

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
