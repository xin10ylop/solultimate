"""Score a continuous replay's trades per period (by decision slot), racing vs normal fees."""
import sys
import numpy as np
import pandas as pd

t = pd.read_csv(sys.argv[1])
t = t[t.variant == 'lse_bundle']
P = [('holdout', 463424017, 463690000), ('discovery', 463690000, 464121648), ('forward', 464121648, 464188821),
     ('gap (out of sample)', 464188821, 464259868), ('live', 464259868, 10**12)]
H = {'holdout': 15.8, 'discovery': 25.6, 'forward': 4.0, 'gap (out of sample)': 4.2}
for name, a, b in P:
    x = t[(t.decision_slot >= a) & (t.decision_slot < b)]
    if len(x) == 0:
        continue
    r = x.ret.values
    stake = x.stake_sol.values
    rc = (x.pnl_sol + x.fixed_costs_sol - 0.0026).values / stake
    srt, src = np.sort(r), np.sort(rc)
    hrs = H.get(name, (x.decision_slot.max() - x.decision_slot.min()) * 0.211 / 3600)
    print(f'{name:22s} trades {len(x):4d} ({len(x) / hrs * 24:4.0f}/day)  racing fees: mean {r.mean():+.4f} w/o best 2 {srt[:-2].mean() if len(r) > 4 else float("nan"):+.4f} win {(r > 0).mean():.0%} PnL {x.pnl_sol.sum():+.3f} SOL | normal fees: mean {rc.mean():+.4f} w/o best 2 {src[:-2].mean() if len(rc) > 4 else float("nan"):+.4f}')
