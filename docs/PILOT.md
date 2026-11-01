# Pilot results: 25.6 h of real mainnet data (Nov 2026)

Data: slots 463,690,000–464,121,647 fetched from public RPC (`backfill`): 431,265 blocks
(383 missing), measured slot time 214 ms. 37,339 launches (12,912 mayhem), 3.96M curve trades,
10.6M fresh-wallet fundings. Curve continuity holds on 99.8% of non-mayhem trades (decoder health).
Every number below includes the measured landing latency, exact curve fees, slippage and our
own price impact, plus tips, priority and base fees.

## Costs: the hurdle every idea must clear

| stake | curve fees (both legs) | tips + priority + base (both legs) | round trip |
|---|---|---|---|
| 0.1 SOL | 2.5% | 2.6% | ~5.1% |
| 0.25 SOL | 2.5% | 1.0% | ~3.5% |
| 0.5 SOL | 2.5% | 0.5% | ~3.0% |

## Hypotheses and results

| # | hypothesis | out-of-sample result | verdict |
|---|---|---|---|
| 1 | Buy every launch (create or first-candle close) | −4% … −6% per trade, 10–15% wins | no |
| 2 | Launches with big creator bundles | +64% → **−3.6%** once Jito bundles are atomic in the fill model | fake edge |
| 3 | Mayhem coins (agent-rewritten reserves) | jumps unpredictable (autocorr −0.02), drift −1…−3% per jump | avoid |
| 4 | Clones of currently hot tickers | no better than other launches | no |
| 5 | Flow model, coins ≤10 min old | best walk-forward cell +2.7%/trade, but 10 of 18 hours lose and one hour = 90% of PnL; others −1…−5% | no |
| 6 | Flow model, final-stretch coins (30–97% progress) | best cells +2% … +8.5%/trade at the most selective threshold, 5–15 trades/day, ≤ 50% of hours profitable, best hour = 94–142% of PnL | not consistent |
| 7 | #6 + wallet-derived features (funding kind/amount/delay, launch linkage, the operator's past outcomes; **no copy trading**) | improves the 60 s hold from −7.4% to +8.5%/trade (top 2%) and from −6.9% to +1.0% (top 5%) | most promising lead, unproven |
| 8 | Same models evaluated on their *training* hours | +7% … +16%/trade, 57–68% wins | overfitting illusion |
| 9 | Per-operator gating (identity-based) | a few records +9% … +26%/trade (n = 7–41); none clears a statistical bound in a day | research only |

Walk-forward details: `reports/wf/grid.txt` (hourly periods; train on the trailing 8 hours;
threshold chosen on training data; ≤ 3 open trades; one trade per coin). Reproduce everything with
`research/run_all.sh data/pilot reports/pilot 3600`.

## What this means

* At this latency and cost there is **no consistent edge in a day of data**. Anything that
  looked like one was in-sample or came from optimistic fills. The engine refuses to trade
  by construction.
* The one structural lead is #7: final-stretch coins, 30–60 s holds, with wallet-derived
  context. It needs weeks of data to be judged day by day (RUNBOOK §2).

## Levers that can change the answer

1. **Data.** 30 days of firehose recording lets you judge the go-live gate *per day*.
2. **Latency.** The edge is sensitive to landing position. `--stress 1` (more next-slot landings)
   costs ~1–2 points per trade. Reacting from deshredded transactions (Yellowstone
   `subscribe_deshred`, already in the client) instead of executed ones could move landings
   earlier in the slot. Measure it with the trader's journal before relying on it.
3. **Stake.** Moving from 0.25 to 0.5 SOL removes ~0.5 points of fixed cost per trade.
   Price impact is simulated, so the lab shows the trade-off.
4. **New regime.** Post-migration PumpSwap trading is not decoded yet. It is a different
   market with different participants.
