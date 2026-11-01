# Money: bankroll, costs per trade, and what returns would require

**Status:** no strategy has passed validation yet (see `PILOT.md`, `ROUND3.md`), so there is
no measured expected return. The engine refuses to trade live until one passes the go-live gate
in `RUNBOOK.md`. Everything below is the cost and sizing arithmetic any candidate must clear.

## Cost of one round trip (buy + sell), as % of stake

Curve fees are 1.25% per side (PumpSwap 1.20% for newly graduated coins). Fixed costs are
per transaction, so they weigh more at small stakes.

| stake | curve fees | fixed, racing (measured live: 3.5M lamports priority + 0.001 tip per leg = 0.009 SOL) | fixed, default (0.0026 SOL) | fixed, not racing (≈0.0002 SOL) |
|---|---|---|---|---|
| 0.05 SOL | 2.5% | 18% → **20.5% total** | 5.1% → 7.6% | 0.4% → 2.9% |
| 0.1 SOL | 2.5% | 9% → **11.5%** | 2.6% → 5.1% | 0.2% → 2.7% |
| 0.25 SOL | 2.5% | 3.6% → **6.1%** | 1.0% → 3.5% | 0.1% → 2.6% |
| 0.5 SOL | 2.5% | 1.8% → **4.3%** | 0.5% → 3.0% | 0.04% → 2.5% |
| 1 SOL | 2.5% | 0.9% → **3.4%** | 0.26% → 2.8% | 0.02% → 2.5% |

Failed (refused) buys also pay priority (~0.0004 SOL each in the measured data), and our own
price impact grows with stake. Source: `crates/ucore/src/fill.rs` (`CostModel`).

## Bankroll rules (current config, `config/trader.example.toml`)

| setting | value |
|---|---|
| bankroll | 2 SOL |
| stake | 0.1 SOL (canary) |
| max stake | 5% of bankroll |
| max open positions | 2 |
| daily loss limit | 0.3 SOL → no entries until UTC midnight |
| losing streak | 5 losses → pause ~3 h |
| hot-wallet cap | trader refuses to start live above N SOL |

**With 1 SOL:** the 5% rule gives 0.05 SOL stakes. At that size any strategy that races for
landing costs ~20% per round trip, so it cannot work. Only non-racing strategies (taking a
position ahead of a known event, low priority, no tip) are viable, and they still need > 2.9%
per trade. 1 SOL is enough for a canary of such a strategy, not for scaling.

Practical minimums: 0.1 SOL stakes → ≥ 2 SOL bankroll; 0.25 SOL stakes → ≥ 5 SOL;
0.5 SOL stakes → ≥ 10 SOL.

## What a daily return would require

daily PnL = trades per day × stake × net edge per trade (after every cost above)

| net edge per trade | 20 trades/day at 0.1 SOL | 50 trades/day at 0.1 SOL | 50 trades/day at 0.5 SOL |
|---|---|---|---|
| +1% | +0.02 SOL | +0.05 SOL | +0.25 SOL |
| +3% | +0.06 SOL | +0.15 SOL | +0.75 SOL |
| +5% | +0.10 SOL | +0.25 SOL | +1.25 SOL |

These are arithmetic, not forecasts. For calibration, the best consistently profitable real
taker wallets in our data net +1% … +6% per SOL traded (`ROUND3.md` §2).

## Rollout (from `RUNBOOK.md`)

1. Paper on live data until the go-live gate holds on ≥ 14 traded days.
2. Canary: live, 0.1 SOL stakes, 1 open position, ≤ 3 SOL in the wallet, until ≥ 50 real trades;
   judged on fill quality vs the simulator, not on profit.
3. Double the stake only after 100 more live trades that stay inside the daily limits and match
   the simulator.
