# Round 3: follow the money

Rounds 1–2 mined signals and found nothing that survived out of sample. Round 3 asks a different
question first: **who actually makes money on pump.fun, consistently, after all costs, and how?**
Every number below comes from the full trade tape of three separate periods (holdout 15.8 h,
discovery 25.6 h, forward 4 h; ~45 h, 7.0M curve trades, 68k launches), with curve fees,
priority fees and tips charged per wallet. Scripts: `research/follow_money/`.

## 1. Who wins on the bonding curve (non-mayhem coins)

ROI = net PnL / SOL spent, per archetype, in each period:

| archetype | holdout | discovery | forward |
|---|---|---|---|
| coin creators (dev wallet) | **+42%** | **+34%** | **+33%** |
| create-slot buyers, not the dev | +5.6% | +6.6% | +1.0% |
| flash takers (< 10 s holds) | −15% | −14% | −5% |
| short takers (< 2 min) | −4% | −5% | −7.5% |
| long takers | −9% | −9% | −13% |

The create-slot profit sits **entirely in the transaction right after the create**:

| position after the create tx | holdout | discovery | forward |
|---|---|---|---|
| next tx (gap 1) | **+64%** | **+62%** | **+40%** |
| gap 2 | −15% | −13% | −23% |
| 5–10 trades into the slot (where our landing puts us) | −25% | −26% | −45% |

Gap-1 is the creator's own bundle or privileged backrunning. This independently confirms the
simulator's verdict on create-slot entries at our latency.

## 2. The consistent takers

About 100 wallets (not creators, not create-slot) were net profitable in all three periods over
hundreds to thousands of coins, paying almost nothing for speed. Two archetypes:

* **Slow dip-buyers** (~10 wallets): enter 30–120 s after launch in coins with strong recent net
  inflow (+7.4 SOL in 30 s) but ~10% below the 30 s peak, hold 1–5 min, ~60% win rate,
  +1–6% ROI.
* **Tail harvester** (one wallet): 0.1 SOL on ~1,000 coins/day, 32% win rate, profit carried by
  a few 10–65× coins. Negative without its top 5 coins → fails our "without best 2" rule.

Their selection skill is real and stable. Forward return after their entries minus everyone
else's entries at the same coin age and curve level:

| horizon | holdout | discovery | forward |
|---|---|---|---|
| 5 s | +1.8% | +2.1% | +1.4% |
| 30 s | +5.4% | +6.0% | +4.4% |
| 120 s | +9.7% | +10.5% | +6.7% |

But the average coin decays fast, and **even entering exactly where they entered, with our
landing and costs, is break-even** (best exit +0.4%/trade, most exits −0.5…−1.5%). Their thin
edge comes from being first plus hand-tuned exits. Not replicable at a profit.

Preset take-profit / stop-loss walls do not exist at a useful scale: sell multiples relative to
each wallet's entry show no spikes at 1.5×, 2×, 3× (spike ratio ≤ 1.2).

## 3. Mayhem coins

One program wallet trades in 12,525 of 12,692 mayhem coins (860k trades, 72,585 SOL volume in
25.6 h) and is roughly neutral (+0.26% of its volume). Short holds look great when bucketed after
the fact (+11…+20%), but that is outcome bias: at wallet level, counting every position, mayhem
traders lose −3.6%, −5.0%, −5.9%. Not a free lunch.

## 4. Fees are the wall

PumpSwap fee tiers (read from the on-chain fee config): 1.20% per side for freshly graduated
coins (0.95% of it to the creator), falling below 0.75% only above ~30,000 SOL market cap. The
curve charges 1.25% per side. Every small-cap taker strategy needs > 2.5% per round trip.

## 5. Predictable program flows on PumpSwap: the graduation boost

New decoder: `ucore::amm` (PumpSwap pool creation, trades, boost buy-and-burn), verified against
mainnet events; `backfill --amm` records it.

**Mechanism (measured).** Graduation (`MigrateV2`) creates the PumpSwap pool and, for most coins,
a **boost vault**: a fixed **17.58 SOL** for SOL-paired coins (next to ~85 SOL of real pool
liquidity), booked as virtual quote reserves so the opening price is continuous. A single
protocol authority (`HTVZVEQMBs…`) then calls `boost_buy_and_burn` on the pool **every 55 slots**
(p10/p90 53–58), starting ~2 s after pool open, ~30 times over ~5.6 min, each time spending a
roughly fixed share of the vault's remaining balance (published in every event). The crank's own
transaction caps slippage at **5%** and pays low priority (lands mid-block).

So the timing and size of every crank buy are known in advance: a scheduled, protocol-funded,
non-informational buyer: 4,044 burns on 118 pools and 2,011 SOL spent in the 4 h forward period
(~12,000 SOL/day). Tested three ways on the full forward period (174 SOL-paired graduations,
109 with a boost program, 3,109 burn cycles), with the pool's real fee tier on both legs:

| idea | result |
|---|---|
| ride the boost from pool open | mean −2.8% at 15 s, −7% at 60 s, −24% at 2 min, −39% at 6 min; graduation selling overwhelms ~37% of cumulative crank pressure |
| front-run every burn (buy 1–4 slots before the predicted slot, sell 1 slot after) | **−1.4% … −1.6% per cycle**, 11–13% wins |
| front-run only burns whose predicted impact > round-trip fee (+0 … +1%) | −1.3% … −1.9% per cycle |

Price around burns is a sawtooth: **+1.2% jump at each burn** (median 0.95%), decay between
burns; the predicted burn slot is exact to ±2 slots for half the cycles. The jump is well below the **2.4% round-trip
fee** at the graduation fee tier. Real and predictable, but priced below the fee wall.

(An early version of this test reported +0.9%/cycle. That was a bug: some trades report a 0-bps
fee and the simulator charged it. The pool's fee tier is now used.)

## 6. What this means

* Consistent profit on pump.fun goes to **creators** (+33–42% ROI, plus 0.30% of curve volume and
  0.95% of early PumpSwap volume as creator fees) and to **the creator's own bundle at create+1**
  (+40–64%). 86–93% of create+1 positions belong to a wallet tied to one creator.
* Takers who win do it with real, stable **selection skill** (+5–10% excess at 30–120 s), yet net
  only +1–6%, and their exact entries are break-even at our landing.
* Every protocol mechanism examined (mayhem agent, boost crank, graduation, child coins) moves
  price by less than the **~2.4–2.5% round-trip fee**. That fee is the binding constraint, more
  than latency.
* An edge for us must therefore be **large per trade (> 3% after our landing)**, which in this
  market means information about future flow that others do not price: which coins an operator
  will pump (round 2's launch-structure family, +8% on some days) or which will draw retail flow.
  It must be judged per day across many regimes, which is what the 30-day recording is for.

## 7. Coins quoted in other tokens, and cross-pool arbitrage

**Inventory (4 h forward period).** 22% of curve launches are quoted in a token other than SOL:
PUMP (561), USDC (65) and ~135 other tokens. PumpSwap had 7,726 traded pools; ~1,980 have a
non-SOL quote, spread over ~1,035 quote tokens, of which only 138 also have a SOL pool on
PumpSwap. Many coins trade in several pools (PUMP: 4 SOL pools, 2 USDC pools, pump-coin pools;
some pump coins sit in 10+ pools). Pools created outside migrations pay a flat 0.30% per hop
instead of 1.2%.

**Arbitrage test.** Where one coin trades in several pools, prices must agree, and a gap can be
closed in one atomic transaction (it fails and costs only the network fee if the gap is gone).
`arb.py` checked 674 two-pool and 15,854 three-pool SOL cycles at the end of every slot for 3.94 h
(263k evaluations), using exact reserves and each pool's fee:

* gaps above the summed fees appear in 2% of two-pool and 21% of three-pool evaluations, but they
  sit in pools so thin that the best trade is dust;
* opportunities worth ≥ 0.001 SOL after a 0.0002 SOL transaction cost: **1 in 3.94 h**, open for
  1 slot (SOL → USDC → PUMP → SOL).

Liquid gaps are closed inside the same slot by existing arbitrage bots. That race belongs to
searchers co-located with block producers, not to a bot landing 1–2 slots late. Not covered yet:
quote tokens whose SOL market is on Raydium / Meteora / Orca (~900 of them), which needs decoders
for those DEXes.
