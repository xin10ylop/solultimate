"""Trapped real SOL in mayhem curves: profit of buying out the curve at the current (rewritten)
virtual price, completing it, and selling those tokens into the PumpSwap pool that opens with the
curve's real SOL.

At a curve state (vs, vt, rs, rt) after a trade:
  cost to buy the rt real tokens left:  C = vs * rt / (vt - rt) / (1 - f)
  pool after migration:                  quote = rs + C*(1-f) - migration_fee, base = B_pool
  sell rt tokens into it:               out = quote * rt / (B_pool + rt) * (1 - f_pool)
B_pool is measured from mayhem pool creations (206.9M tokens); migration fee ~0 for these pools
(checked against pool creations). Profit = out - C."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load

d = sys.argv[1]
t, c, k = load(d)
may = set(c[(c['flags'] & 1) != 0]['mint'].tolist())
t = t[np.isin(t['mint'], list(may)) & (t['tok'] > 0)]
F = 0.0125
FP = 0.0125
B_POOL = 206_900_000 * 1e6
MIGR = 0.0
vs = t['vsol'].astype(np.float64)
vt = t['vtok'].astype(np.float64)
rs_ = None
# real reserves are in the trade tape? the binary tape stores vsol/vtok only; reconstruct real SOL from cumulative net flow
order = np.lexsort((np.arange(len(t)), t['mint']))
t = t[order]
vs, vt = vs[order], vt[order]
sol = t['sol'].astype(np.float64)
tok = t['tok'].astype(np.float64)
buy = t['buy'].astype(bool)
mint = t['mint']
st = np.r_[0, np.flatnonzero(mint[1:] != mint[:-1]) + 1]
en = np.r_[st[1:], len(t)]
rows = []
for s, e in zip(st, en):
    flow = np.cumsum(np.where(buy[s:e], sol[s:e], -sol[s:e]))      # real SOL in the curve (net of fees)
    sold = np.cumsum(np.where(buy[s:e], tok[s:e], -tok[s:e]))      # tokens sold by the curve
    rt = 793_100_000 * 1e6 - sold                                     # real tokens left (standard initial real reserves)
    v_s, v_t = vs[s:e], vt[s:e]
    ok = (rt > 0) & (v_t > rt)
    C = np.where(ok, v_s * rt / np.maximum(v_t - rt, 1) / (1 - F), np.inf)
    quote = flow + C * (1 - F) - MIGR * 1e9
    out = quote * rt / (B_POOL + rt) * (1 - FP)
    prof = (out - C) / 1e9
    i = int(np.argmax(np.where(ok, prof, -np.inf)))
    rows.append(dict(mint=int(mint[s]), n=e - s, best_profit=prof[i], cost=C[i] / 1e9, real_sol=flow[i] / 1e9, vsol=v_s[i] / 1e9,
                     rt_frac=rt[i] / (793_100_000 * 1e6), share_pos=float(np.mean(prof[ok] > 0.01)) if ok.any() else 0.0,
                     slot=int(t['slot'][s + i]), completed=int(mint[s]) in set(k['mint'].tolist())))
R = pd.DataFrame(rows)
pd.set_option('display.width', 220)
print(f'mayhem coins {len(R)}; coins where buying out the curve would have netted > 0.05 SOL at some point: {(R.best_profit > 0.05).sum()}')
for th in (0.01, 0.05, 0.2, 1.0):
    s = R[R.best_profit > th]
    print(f'  best profit > {th} SOL: coins {len(s)}  sum of best profits {s.best_profit.sum():.2f} SOL  median cost {s.cost.median():.3f} SOL')
print(R.sort_values('best_profit', ascending=False).head(15).round(4).to_string())
R.to_pickle('trapped.pkl')
