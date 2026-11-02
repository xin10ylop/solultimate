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

## 8. The mayhem agent, decoded

Mayhem program `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e` (no published IDL; reverse-engineered
from 4,332 live agent transactions and a full 10-minute window; scripts in `research/mayhem/`).

* A crank key (`Gygj9QQb…`) calls the program's `buy` / `sell` (and `_v2`) ~14 times per second
  across all mayhem coins. Each call CPIs a pump curve trade and emits a `MayhemTradeEvent`
  (295-byte payload: direction @0, agent wallet @1, mint @33, SOL / token amounts @65 / @73,
  running net SOL and net tokens as i128 @97 / @113, curve reserves @129–192, current time @161
  and the mayhem end time = start + 86,400 s @169).
* **Inside every agent transaction (95.5%) the curve's virtual SOL is rewritten before the trade**,
  in the same direction as the trade (agent buy → price jumps up, 99.8%; sell → down). Virtual
  tokens and real SOL never change. Jump quartiles −15% / +16%, 5th/95th percentile ±32%.
  Rewrites never happen outside agent transactions (0 of 7,669).
* Direction is a coin flip: P(buy | previous buy) 0.504, P(buy | previous sell) 0.512; no
  dependence on the agent's inventory; no bit of the previous event predicts it better than 51.6%;
  by price deviation before acting (no look-ahead) P(buy) stays within 0.48–0.55. Breaking even
  on a +15% / −15% bet against a 2.5% round-trip fee needs ≥ 58% correct.
* Activity is concentrated in each coin's first minutes (median rewrite at 47 s of age, 90%
  within 3.4 min); almost no coin trades near its 24 h end.
* Rewrites never push virtual SOL below the real SOL held by the curve, so there is no trapped
  value to capture by buying out the curve (best case across 2,604 coins: 0.04 SOL).

Verdict: a program-set random walk with large jumps. Nothing predictable to trade.

## 9. Operator "dry powder" (rejected)

Hypothesis: wallets funded by a launch's operator funders (the dev's and create-slot buyers'
funders, exchange-scale hubs excluded) that have not traded the coin yet are future buying power
aimed at it. Measured at our realistic entry (two slots after the create), all three periods
(`research/follow_money/drypowder.py`):

| dry powder > 50 SOL | n | 60 s | 300 s |
|---|---|---|---|
| forward | 28 | +10.8% | +6.0% |
| discovery | 400 | −3.2% | −14.8% |
| holdout | 279 | −12.3% | −22.6% |

Dry-powder wallets bought the coin later in only 1.3–2.1% of launches. The forward result was noise.

PUMP buyback (checked as a scheduled flow): one authority sweeps all 8 buyback vaults about every
2 minutes (≈ 884,000 SOL claimed in total), so buying is spread in small chunks against PUMP's deep
liquidity. Negligible impact.

## 10. Anchored-token arbitrage, funding harvest, incentives, cohort latency (all rejected)

**Anchored quote tokens vs Hyperliquid** (`research/anchor/`). Pump coins quoted in wXMR, WBTC,
DOGE, HYPE, tokenized stocks and pre-IPO tokens route flow through those tokens' thin Solana pools;
the anchors trade deep on Hyperliquid (crypto perps and `xyz` stock / pre-IPO perps).
* Historical minute candles (3.5 days): DOGE / HYPE / WBTC gaps median 0.07–0.3%, gone within a
  minute; wXMR gaps persist 15–25 min, ~4 per day above 1%, capacity of a few thousand USD.
* Live executable quotes (19 pairs, Jupiter $500 / $3,000 vs Hyperliquid book, all fees): taking
  both sides at once always loses (−0.1% … −4%); standing one-sided offsets of 0.05–0.2% are the
  perps' normal basis; even buying each token at its best moment and selling at its best moment
  in the window lost on every pair (gaps swing 0.05–0.2% vs 0.2–0.5% round-trip cost).
* SOL → coin → SOL Jupiter round trips for memecoins quoted in these tokens: 0.91–0.994.

**Funding harvest** (spot long + perp short): last 30 days median funding 9% APR, best coin 31%;
rotating into the leaders loses to costs (−38% … −112% APR). Safe and consistent, not very profitable
in this regime.

**PUMP volume incentives**: both global volume accumulators are empty (program off): no fee rebate.

**Fee-only creating** (no dev buy, no supply held): such launches get almost no volume (median
0.00–0.01 SOL, mean creator fee 0.001–0.012 SOL vs ~0.02 SOL to create).

**Copying the consistent cohort's exact decisions with our latency** (`latency_decomp.py`):

| our position vs theirs | discovery | holdout | forward |
|---|---|---|---|
| their own fill | +3.7% | +2.9% | +3.7% |
| next trade behind them | +1.4% | +0.6% | +1.4% |
| 2 trades later | +0.2% | −0.6% | +0.1% |
| 4 trades later / next slot | −0.3% | −1.2% | −0.4% |

Their edge is position in the order flow, not logic we could learn.

## 11. Completers, leader geography, liquidations, other launchpads

* **Curve completers** (180 SOL-paired graduations): 68% complete with a single buy, mostly whole-curve
  85 SOL self-graduations with nothing sold back within 10 min, plus the mayhem agent (45). Completers
  lose on average (median −0.42 SOL, 33% win). Not an opportunity.
* **Leader geography** (`research/geo/`, 764,804 slots, 654 leaders geolocated): 55.7% of slots have a
  leader within 500 km of Frankfurt (Frankfurt 22.7%, Amsterdam 18.5%, London 8.1%). Same-slot
  reactions to big buys happen almost as often with far leaders (62%) as with near ones (69%), and the
  consistent cohort trades in exact proportion to slot share in every region: their edge is landing
  infrastructure that works everywhere, not geography.
* **Liquidations**: a Kamino tracker counts ~308,661 liquidations and ~$3.7M of total liquidator
  profit since launch (~$12 per event), contested by co-located bots. Too small to be very profitable.
* **Other launchpads**: per block, pump curve 18 txs, PumpSwap 51, Meteora DAMM v2 18, LaunchLab 0.8,
  Meteora DBC 1.6. LaunchLab (Bonk.fun, StonkFun) SOL-quoted volume ≈ 11,000 SOL/day, ~3% of pump.fun.

## 12. Back to the videos: run mode + quick exit (candidate G, in validation)

The videos' run-mode rule (operator bundle ≥ 30% of supply, ≥ 30% of its wallets fresh-funded within
30 min, entry at first-candle close) was re-tuned with a quick-exit grid (2–45 s holds × stop × TP ×
dump reaction, `lab signals --exit-quick / --exit-hold`) on all three periods, then frozen as G
(`research/frozen_candidates.md`, `config/eval/G.toml`) before the gap period.

* Optimum is interior: hold 8–10 s, no take-profit, no dump reaction; ≥ 12 s and dump reactions both hurt.
* Robust to landing: stress 1 and 2 barely change results (entry is the next slot either way).
* Live brain replay takes exactly the same trades as the signal lab (79 / 204 / 11, correlation 0.98–0.99).

**Correction.** Signal-lab `net` excludes fixed costs (stored in `fixed`). Earlier G figures
(+5.9% / +10.1% / +5.4%) omitted ~3.6 points of priority + tips at 0.25 SOL. Corrected, per attempt
including refused buys (12%):

| G (10 s, −25%) | holdout | discovery | forward |
|---|---|---|---|
| racing fees 0.009 SOL / round trip | +1.8% | +5.5% | +0.4% |
| live brain replay (racing fees) | +1.0% | +5.7% | +1.6% |
| normal fees 0.0026 SOL / round trip (fills only) | +4.9% | +9.0% | +4.4% |

At racing fees G is marginal (fails "without best 2" on holdout and forward); at normal fees it is solid
(the −15% stop variant passes "without best 2" in all three periods). Normal fees are plausible
because G does not depend on queue position, which must be confirmed live. Pending: out-of-sample
gap period, live recorder days, measured fills.

### G and G2: out-of-sample and live results (live brain, continuous replay, all costs)

G2 = G + known operator + first candle ≤ 160 SOL (both FOMO rules, frozen before the gap period).
Continuous replay over holdout → discovery → forward → gap → live (operator history carried over),
racing fees (0.009 SOL / round trip) unless noted:

| period | G trades | G mean (w/o best 2) | G2 trades | G2 mean (w/o best 2) | G2 normal fees |
|---|---|---|---|---|---|
| holdout 15.8 h | 90 | +0.7% (−1.6%) | 58 | +2.1% (−1.6%) | +4.7% |
| discovery 25.6 h | 227 | +5.3% (+4.2%) | 152 | +9.6% (+8.1%) | +12.2% |
| forward 4 h | 12 | +1.6% (−7.1%) | 5 | +14.2% (−6.5%) | +16.8% |
| **gap 4.2 h (out of sample)** | 10 | **+8.1% (+1.2%)** | 5 | **+13.5% (+1.8%)** | +16.0% |
| live ~1.3 h | 8 | −6.8% | 3 | −14.7% | −12.1% |

G2 total: 223 trades, ≈ +4.2 SOL at 0.25 SOL stakes in ~51 h, but 3.65 SOL of it on the discovery
day. Positive in four periods, negative in the newest (tiny) one. **Not proven**: it needs the
per-day test on many more days. `config/trader.G2.paper.toml` runs it in paper mode on live data.

## 13. Proving it without a box: 15 archived days (Old Faithful)

The public RPCs are too slow for weeks of history (~6 slots/s). The ledger archive
(`files.old-faithful.net`, one 300–650 GB CAR file per epoch) streams at ~170 MB/s per process here, so
`carstream` (new) imports a day in ~40 min, decoding with the same `decode_tx` as the recorder.
Parity vs RPC data on 1,500 slots: identical blocks, launches, fundings, transfers and trades
(except non-SOL quote fields the old pilot2 decoder zeroed). ~40% of transactions use the new v1
layout (version byte 0x81); recent epochs contain no vote transactions.

**G / G2 on unseen days (pre-registered, nothing changed).** Continuous replay through the live
engine with measured live costs, book carried across days:

| unseen day (UTC) | G2 trades | G2 racing fees | G2 normal fees | G2 PnL @0.25 SOL | G racing fees |
|---|---|---|---|---|---|
| Oct 24 | 76 | −0.3% | +2.4% | −0.05 SOL | −1.1% |
| Oct 25 | 8 | −36.4% | −33.9% | −0.73 SOL | −10.0% |
| Oct 26 | 143 | −18.9% | −16.3% | −6.77 SOL | −16.1% |
| Oct 27 | 245 | −12.4% | −9.8% | −7.59 SOL | −9.9% |
| Oct 28 | 185 | −5.9% | −3.3% | −2.72 SOL | −4.3% |
| Oct 29 | 192 | +2.0% | +4.6% | +0.94 SOL | −8.0% |
| **6 days** | **849** | **−8.0%** | **−5.4%** | **−16.9 SOL** (max drawdown 19.0) | **−8.5%** (0 of 6 days) |

(Oct 23 = warm-up of the continuous replay over Oct 23 → Oct 30 20:45; Oct 30 is partial and
negative too.) G2 fails: 1 of 6 days profitable at racing fees, 2 of 6 at normal fees, 5-day losing
streak. The profitable in-sample days (Oct 30 evening → Nov 1) were one regime. Losses come in
operator bursts: e.g. one operator dumped on all 11 of his coins within 26 minutes (−72.5% average;
the −25% stop cannot protect when the dump lands before our sell).

**Follow the money over many days** (`wallets`: every bonding-curve position, after curve fees,
transaction fees and tips; leftovers marked as if all holders sold together). Oct 16–18:

| style | ROI |
|---|---|
| devs (creators) | +33% to +37% |
| create-slot bundles | +4% to +7% |
| buyers ≤ 2 slots after create | −5% to +0% |
| flash takers (< 10 s) | −4% to −0% |
| short holds (< 2 min) | −6% to −8% |
| long holds | −12% to −13% |

The ecosystem as a whole loses ~5% of what it spends; takers fund the operators. Taker skill
does persist: wallets profitable on two days are profitable on the third 60–73% of the time
(baseline 17%), but at +2% ROI. Entering where those winners buy, with our landing and generic exits,
loses 2.4–3.4% per trade (random moments: −3.6% to −5.5%): their edge is speed and exit execution.

**Holder rewards (new mechanic)**: 13% of launches route 0.3% of volume to holders. Over a coin's whole
life that is a median 1.7% (p90 3.8%) of the SOL in its curve: less than one round trip of fees.
The mayhem agent trades a median ~1 SOL per coin, so mayhem volume pays holders next to nothing.
Rejected.

**Where the consistent taker money is: the creation block (Oct 16–19, development days).**
Buys landing in the coin's creation slot but more than 8 transactions after the create (not the
dev's own bundle) earned +1.8% / +3.3% / +4.5% / +3.0% ROI on the four days, but only because of a
few wallets: without the top 20 wallets every day is negative, and the median such sniper loses
1–3%. The 58 snipers profitable on Oct 16 and 17 made +7.5% on Oct 18 (72% of them profitable),
mostly by letting the right positions run (hold ≥ 50 slots: +21%; ≤ 3 slots: −1%).
On exactly their Oct 18 coins our own create entry loses with every one of 170 exits
(fixed holds, TP / stop / trailing grid, dump reactions): −7% to −10% at the measured landing,
−7% even with 100% same-slot landing, −3.5% with same-slot entries, prompt sells and default costs.
Their edge is position at the top of the block and discretionary exits, i.e. execution, not a
selection rule we could learn. Identity-free flow model (LightGBM, 320k decision points, 3 days,
time split): no positive bucket out of sample (top 1%: −1.6% to −5.9%).


## 14. Why copy3 works and this does not — the mechanic is the edge (2026-11-02)

copy3 (HoodSniper) trades profitably, but **not on Solana**: it snipes the **Pons V2** launchpad on
**Robinhood Chain**, whose curve charges a snipe tax that decays **99% → 6.2% (+1 s) → 0.19% (+2 s) → 0**
over three seconds. That tax gate is the whole edge: almost nobody buys in the 99% second, so a bot
co-located in AWS Ohio (3 ms from the sequencer) that lands first in the +1 s block buys ahead of the
field at 6%, and the launch's tax-exempt "team" wallets flag which launches are operator plays. Its own
STATE.md shows even that edge fading (qualifying launches ~100/day → ~30/day, per-trade +10% → −2.2%,
the public feed IP-banned).

**That mechanic has no analog on Solana's busy launchpads.** Measured on the full Nov-1 archive day:
- pump.fun: no snipe tax; anyone buys from block 0. Earlier sections showed the first-second race is
  already saturated — ordinary snipers lose, ~1% of bots win on raw speed and exit execution.
- Meteora **Dynamic Bonding Curve** (the launchpad family with decaying-fee tooling, `ucore::dbc`):
  **2,788 launches, 109/h, and 99.7% charge a flat fee (median 0.25%) from second one.** Only 9
  launches (0.3%) used a decaying early fee. There is no tax window to be first into.

So copy3's recipe (be first into the +1 s block, ride the exempt team, exit in seconds) is **structural
to Pons V2**, not a selection rule we can port. Porting its *rules* to pump.fun confirms this: operator
cool-off, loss-streak pause and trailing/dump exits all stay negative on unseen days (§ frozen_candidates).
The one piece that transfers as signal is the **idle-team rule** — enter only when ≥ 1 tax-exempt-style
team wallet did *not* buy at launch (all buying at once = the bundle that dumps): it lifts the win rate
31% → 42% and more than halves the loss per trade, but pump.fun launches still lose on average.

**Conclusion.** The edge copy3 exploits is a chain+launchpad mechanic (Pons V2's snipe tax), reachable
only with a co-located latency stack, and already decaying. On Solana the equivalent mechanic does not
exist, which is consistent with every Solana result in this repo. The strategic lesson the user named —
"new mechanics create structural edges" — is exactly right, and it points off Solana (to a snipe-tax
launchpad) rather than to another pump.fun rule.
