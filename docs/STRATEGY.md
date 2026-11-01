# Strategy: Lineage

**One sentence.** Every pump.fun launch is attributed, in real time and using only
pre-launch information, to an *operator* (the wallet network behind it) and to a *launch
type*. Every launch is shadow-traded with a family of entry/exit playbooks under our *measured*
landing latency. Real money goes only to an operator or type whose **selection-honest**
track record has a lower confidence bound above zero, and only with the playbook that earned it.

## 1. What we took from the reference material, and what we threw away

| Source | Kept (as a mechanism, automated) | Dropped |
|---|---|---|
| FOMO / F-Project (3 FR videos) | ruggers are bots, so they repeat; funding traces: exchange hot wallet + exact non-round amount, mother wallets, last-transfer chains, fresh-wallet filter; qualify on ≥10 past launches; first-candle size cap; bundle size and fresh bundle wallets correlate with "real" runs; Max-Loss after N consecutive losses; sell on big buy; front-run sell on token consolidation; phantom SL; sell on no activity; buy on dev sell; buy the dip; "crumbs, not the cake" sizing | manual Solscan work, Axiom UI rules, fixed +100% TP chosen by eye, the assumption that we land in block 0 |
| Play-finder video (Robinhood chain) | "first taxed buyer" = the *true* entry, not the dev's price; P-columns (gain 2/5/10/30/60 s after the true entry) as the way to judge a play; sprayer/treasury watch that pre-arms the next fresh dev wallet; per-play TP/drawdown from the P-columns; one category per play | Telegram bots, Claude-in-Chrome hunting, Twitter/KOL anything |
| Your landing measurements | the whole latency model (§3.1). Same-slot 7/15, next slot 8/15, never first behind, 2–13 trades in between, +10.7% median price over the target, 20% price-limit refusals | — |

## 2. What is new here

1. **No human in the loop.** Operator discovery runs on *every* launch (~15–28k/day, about half
   SOL-paired and non-mayhem). Identity links are added only from facts that exist before the
   launch: the dev wallet (reuse), its first funding (non-hub sender = treasury/mother/previous
   dev, two hops), and for exchange hot wallets the `(hot wallet, exact amount)` signature,
   which automates FOMO's "Binance 2 → 2.955 SOL" method. Recurring create-slot buyers become
   the operator's insider wallets. Hubs (exchanges) are detected from fan-out, not hard-coded.
2. **Shadow-trade everything.** Each tradable launch gets 22 shadow positions (entries at create,
   at first-candle close, at the first dev sell, after the first dump × several exit rules),
   each filled by the landing model against the real event stream. The book keeps learning
   in production too, because live mode runs the same shadows.
3. **Selection-honest records.** Picking the best of 22 playbooks per operator in hindsight
   would overstate the edge. So before each launch we fix the playbook the operator's history
   points to, and the operator's record is the result *of that pre-chosen playbook*. The gate
   reads only that record:
   `shrunk = (n·mean + n0·prior)/(n + n0)`, `LCB = shrunk − z·sd/√n`, where the prior is the
   live population mean (≈ −5…−10% per trade: following random launches loses money).
   Entry requires `n ≥ 6`, `LCB > +3%`, a win rate ≥ 45%, a positive mean over the last 4, no
   loss streak ≥ 4, and the last 7 days only (operators change).
4. **Context cohorts.** The same machinery runs on launch *types* (dev-buy size × bundle SOL ×
   create-slot buyer count × funding kind), entered at first-candle close. These catch new
   operators that run a known recipe, and they have a stricter gate (n ≥ 25, z = 1.64).
5. **Latency-honest counterfactual fills.** Our fill is the curve state after the trades
   that really executed ahead of us, using the program's exact integer math (verified to the
   lamport against mainnet). Our own size is accounted for with a k-invariant SOL shift, so
   later trades see the curve as it would have been with us in it. Transactions in a contiguous
   group with the trigger (Jito bundles) are atomic: we can never land inside them. This
   rule alone removed a fake +64% "big bundle" edge in the pilot.
6. **Dump radar.** These are exit signals the operator cannot easily hide:
   * a seller that never bought on the curve (its tokens came by transfer, i.e. a hidden insider);
   * token consolidation into one wallet (≥12% of supply: FOMO's front-run sell, automated);
   * a sell by the operator's known wallets;
   * a big sell (≥12% of curve SOL);
   * the operator's own learned *dump clock* (30th percentile of past dump times).
7. **Adaptive anti-farm cap.** The entry price limit is the operator's own p75 first-candle
   market cap × 1.35. A "farm" launch with an abnormally big first candle simply fails our
   price limit at the cost of the priority fee.
8. **One code path.** The recorder, the backtest, paper trading and live trading all feed
   the same `Brain` with the same events. `lab compare` checks every live fill against its
   shadow twin, which is how bugs and model optimism are found during the canary.

## 3. Mechanics

### 3.1 Landing model (`ucore/src/fill.rs`)
Solana slots are ~211 ms as of Nov 2026 (`lab check` measures it; `ucore::pump::SLOT_MS`).
All time-based settings are written in seconds and converted with that constant, while
the landing model itself is in slots, like your measurements.

* With p = 7/15 we land in the trigger slot after `k` more trades of the coin
  (k ∈ {2,3,4,5,6,8,10,13}, median 4), or at slot end if fewer trades happen.
* Otherwise we land in the next slot, behind `j ∈ {0,1,2,4}` trades of the coin (usually first).
* The trigger's atomic group (consecutive transactions trading the coin) always executes
  before us.
* Buys carry `min_tokens_out`; sells carry `min_sol_out`. If the landing price violates the
  limit, the transaction fails and costs base + priority fee (not the tip). Sells retry with a
  wider limit.
* Stress levels: `--stress 1` (25% same slot, deeper queues) and `--stress 2` (always ≥1 slot late).

### 3.2 Costs (`CostModel`, both legs)
Base fee 5,000; priority 120k CU × 3 lamports/CU (buy) and 100k × 2 (sell); Nozomi tip
0.001 SOL per leg; curve fees as charged (0.95% protocol + 0.30% creator). At a 0.1 SOL
stake the round trip costs ≈ 5.1% before any price move. **That is the hurdle every
playbook must clear.**

### 3.3 Exits (`ucore/src/policy.rs`)
TP ladder on position value (net of fees), SL with optional arming delay, trailing stop,
max hold, idle exit, dump-signal exit (Off / Hard / All), operator dump clock, and forced exit
on curve completion.

### 3.4 Risk (`RiskCfg`)
Stake ≤ min(config, 5% of bankroll); at most 3 open positions, one per operator; daily loss
limit; pause after 5 consecutive losses; mayhem, non-SOL-quote, cashback and depth>0 coins
are excluded.

## 4. What the data says so far (pilot on real mainnet blocks)

See `reports/pilot/summary.md` for the generated report. The key facts:

* **Mayhem coins** (~34% of launches) have virtual reserves rewritten by pump.fun's agent
  between trades. Constant-product replay does not apply, so they are excluded.
* **Decoder health.** Curve continuity holds on 99.8% of non-mayhem trades.
* **Most pump transactions fail** (~80% of pump txs in a block): sniper/bot spam.
* **Base rate.** Blindly following launches at our latency loses about the round-trip cost
  (median ≈ −5%, win rate 10–15%). The edge, if any, is in selection, not in speed.
* **The fake edge.** A "big bundle" bucket looked like +64% per trade until bundles were
  modelled as atomic. After the fix it was −3.6%. Anything that only works if you land inside
  the creator's bundle is not an edge for us.

The pilot is a few days at most. It shows the machinery working and what to expect. It
cannot validate an edge. The 30-day recording plus the go-live gate in the runbook does that.

## 5. Known limitations

* Operators adapt when they notice snipers (FOMO's "farming"). The recency check, the loss
  streak bench and the 7-day lookback limit the damage. They cannot prevent the first losses
  after a change.
* Our own buys are not in the historical data. The k-shift accounts for price impact, but
  not for operators reacting to us.
* Fresh-wallet funding lineage needs the firehose feed. Without it, attribution relies on dev
  reuse and recurring insiders only.
* Synthetic migration (the last buy may overflow into the AMM) is treated as an immediate
  exit at the last curve price.
