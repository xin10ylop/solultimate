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
