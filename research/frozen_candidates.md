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
