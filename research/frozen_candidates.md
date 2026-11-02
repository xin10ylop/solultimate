# Frozen before the holdout (decided on data/pilot only, 2026-11-01)

Exit for all: `x_tpnone_sl0.15_tr0.2-0.1_h15` (no fixed TP, trailing stop arms at +20% and
exits on a 10-point giveback, stop loss −15%, max hold 15 s, idle exit 15 s).

| id | signal | extra filter |
|---|---|---|
| A | `rm_b0_s0.3_f0.5_w7200_n2`: at first-candle close, create-slot holders ≥ 30% of supply, ≥ 50% of create-slot buyers freshly funded (≤ 2 h), ≥ 2 buyers | none |
| B | `rm_g0_s0.5_f0.5_w21600_n2`: at the end of the creator's bundle, holders ≥ 50%, ≥ 50% fresh (≤ 6 h), ≥ 2 buyers | none |
| C | A | dev linked to a known funding network (`how` ∈ funder, funder2, dev_reuse, exchange_amount, insider) |

Success on the holdout = mean net per trade > +2% at 0.25 SOL, and positive in both halves
of the holdout.

## D: Launch-Structure Engine (union, one entry per coin, first trigger wins)

| member | trigger | condition | exit |
|---|---|---|---|
| op_quality_create | end of create tx | operator has ≥ 3 resolved launches and ≥ 50% of them reached 2× after the first candle | hold 15 s, SL −15% |
| rm_g0 strict | end of creator's bundle (same slot) | create-slot holders ≥ 50% of supply, ≥ 50% of create-slot buyers fresh (≤ 6 h), ≥ 2 buyers | trailing (arm +20%, give back 10), SL −15%, max 15 s |
| rm_b0 broad | first-candle close | holders ≥ 30%, ≥ 50% fresh (≤ 2 h), ≥ 2 buyers | same trailing |
| b0_bigbundle_lowcrowd | first-candle close | create-slot SOL ≥ 5, ≤ 1 non-fresh create-slot buyer | same trailing |
| b0_fresh_bundle | first-candle close | ≥ 60% of ≥ 3 create-slot buyers fresh (≤ 2 h) | same trailing |

Entry price limit: +100% over the decision price. Discovery (data/pilot, 25.6 h): 793 trades,
+3.9%/trade (halves +3.9% / +3.7%), 19/26 hours positive, 310 operators, +7.7 SOL at 0.25 SOL.

## E: D + anti-farm (frozen before the holdout)

Same members and exits as D, plus:
* skip coins whose operator already has > 20 launches (FOMO's "farming": serial operators
  learn to dump on snipers; on discovery these had a 23% catastrophic-loss rate and ~0 mean);
* bundle-based members (run-mode, big bundle, fresh bundle) need ≥ 12 create-slot buyers
  (professional bundles; smaller ones carried no edge).

Discovery: 416 trades, +7.5%/trade (median +8.6%), 58% wins, halves +7.3% / +9.5%,
17/23 hours positive, 55 operators, +7.76 SOL at 0.25 SOL.

## Holdout result for A–E (data/pilot2, 15.8 h, Oct 30 20:45 → Oct 31 12:33 UTC)

E: −0.2%/trade (86 trades); D: −4.1%/trade (305). The discovery edge did not replicate,
not even in the same UTC hours (18–24 UTC: E +7.5% discovery vs −0.2% holdout). Rejected.

## F: pre-registered before the forward test (forward = Nov 1 14:10 → 18:10 UTC)

Selected because it is positive on BOTH days under realistic live conditions
(entry always ≥ 1 slot late, 3.5M lamports priority per attempt on both legs + tips,
sells 3–8 slots late), and its whole neighbourhood in the grid agrees:

* signal `rm_g0_s0.5_f0.7_w7200_n2`: at the end of the creator's bundle, create-slot holders
  ≥ 50% of supply, ≥ 70% of create-slot buyers freshly funded (≤ 2 h), ≥ 2 buyers
* exit `q5`: hold 5 s (SL −25%)
* discovery +1.3%/trade (141), holdout +1.6%/trade (38) at 0.25 SOL, realistic settings

Forward success = mean net > 0 under the same realistic settings, and the family average
(the 18 pairs positive on both days) > 0.

## G (frozen 2026-11-01 23:58 UTC, before the gap period 464188821–464259868 and live data)

Video "run mode" entry + quick exit, tuned on the quick-exit grid across holdout / discovery / forward.

* Entry: first-candle close (`rm_b0`): create-slot buyers ≥ 2 (n2) holding ≥ 30% of supply (s0.3),
  ≥ 30% of them fresh-funded within 1800 s before launch (f0.3, w1800). Same anti-farm and bundle
  size rules as the signal lab.
* Exit `k_h10_sl0.25_notp_off`: hold 10 s, stop −25%, no take-profit, no dump reaction.
* Realism: `--live-real` (measured live costs 3.5M lamports priority + 0.001 SOL tip per leg, 3–8 slot
  late sells), stake 0.25 SOL.
* Family for robustness: s0.3 × f{0.3, 0.5} × w1800 × n{2, 3} × hold {8, 10} × stop {0.15, 0.25}.

Results at freeze time (mean per trade / without best 2 / trades):
holdout +5.9% / +3.2% / 79, discovery +10.1% / +8.8% / 204, forward +5.4% / −4.8% / 11.

Pass rule for the gap period and each new day: trade-weighted mean > 0 and without-best-2 > 0
for the frozen pair, and ≥ 60% of the family positive.

## G2 (frozen 2026-11-02 ~00:35 UTC, before the gap period and live data)

G plus two rules from the videos, both consistent across holdout / discovery / forward:
* known operator only: launch attributed to an operator with ≥ 1 earlier launch (`how` ≠ 0);
  unknown operators lost in every period (−1.4% / −3.2% / −5.2%);
* first-candle cap: entry market cap ≤ 160 SOL (≈ $15K; FOMO's rule 2).
Exit unchanged: hold 10 s, stop −25%, no TP, no dump reaction.

Scoring (all costs, refused buys charged at their fee): racing fees 0.009 SOL / round trip and normal
fees 0.0026 SOL / round trip. At freeze, racing: holdout +3.6% (58), discovery +10.1% (151),
forward +15.2% (5); normal: +6.2% / +12.7% / +17.8%.

Pass rule for the gap period and each new day: mean > 0 at racing fees, without-best-2 > 0 at
normal fees, ≥ 10 trades per day.
