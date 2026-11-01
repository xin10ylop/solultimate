"""What happens when a coin graduates: last curve price -> PumpSwap pool open -> first minutes."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load
from pools import load_pools

d = sys.argv[1]
t, c, k = load(d)
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
SL = 0.211
cr = p[p['kind'] == 0]
tr = p[(p['kind'] == 1) | (p['kind'] == 2)]
comp = {int(m): int(s) for m, s in zip(k['mint'], k['slot'])}
print(f'completions {len(comp)}  pool creations {len(cr)}  (pump mints among them: {sum(1 for r in cr if mints[r["mint"]].endswith("pump"))})')
# last curve trade per completed mint
tt = t[np.isin(t['mint'], list(comp.keys()))]
last_curve = {}
for r in tt:
    last_curve[int(r['mint'])] = (int(r['slot']), r['vsol'] / r['vtok'])  # lamports per base unit
H = [0, 5, 15, 30, 60, 120, 300, 600]
rows = []
trp = pd.DataFrame({'pool': tr['pool'], 'slot': tr['slot'].astype(np.int64), 'tx': tr['tx'], 'buy': tr['kind'] == 1,
                    'quote': tr['quote'] / 1e9, 'Q': (tr['quote_res'].astype(np.float64) + tr['vq']) / 1e9,
                    'B': tr['base_res'].astype(np.float64), 'user': tr['user'], 'fee_bps': tr['fee_bps']})
trp = trp.sort_values(['pool', 'slot', 'tx'])
byp = {k_: g for k_, g in trp.groupby('pool')}
for r in cr:
    m = int(r['mint'])
    if m not in comp or m not in last_curve:
        continue
    pool = int(r['pool'])
    s0 = int(r['slot'])
    lc_slot, lc_px = last_curve[m]
    open_px = (r['quote_res'] / r['base_res']) / 1.0  # lamports per base unit
    g = byp.get(pool)
    if g is None or len(g) == 0:
        continue
    px = (g.Q.values * 1e9) / g.B.values  # pre-trade price
    sl = g.slot.values
    row = dict(mint=m, pool=pool, delay_slots=s0 - comp[m], open_vs_curve=open_px / lc_px - 1, n10m=int(np.sum(sl <= s0 + 600 / SL)),
               vol10m=float(g.quote.values[sl <= s0 + 600 / SL].sum()), fee_bps=int(np.median(g.fee_bps.values[:20])))
    for h in H:
        i = np.searchsorted(sl, s0 + int(h / SL), side='left')
        row[f'p{h}'] = px[i] / open_px - 1 if i < len(px) else np.nan
    # net buy flow in the first 30 s / 60 s
    for h in (5, 30, 60):
        msk = sl <= s0 + h / SL
        row[f'net{h}'] = float(np.sum(np.where(g.buy.values[msk], g.quote.values[msk], -g.quote.values[msk])))
    rows.append(row)
G = pd.DataFrame(rows)
pd.set_option('display.width', 250)
print(f'graduations with curve + pool data: {len(G)}')
print('slots from completion to pool creation:', G.delay_slots.describe().round(1).to_dict())
print('pool open price vs last curve price:', G.open_vs_curve.describe().round(4).to_dict())
q = [.1, .25, .5, .75, .9]
for h in H[1:]:
    col = G[f'p{h}'].dropna()
    print(f'  price {h:4d}s after open: mean {col.mean():+.4f} median {col.median():+.4f} q {np.round(col.quantile(q).values, 3)}  P(up) {(col > 0).mean():.2f}')
for h in (5, 30, 60):
    print(f'  net buy flow first {h}s (SOL): median {G[f"net{h}"].median():+.2f} mean {G[f"net{h}"].mean():+.2f}')
print('volume first 10 min (SOL): median', G.vol10m.median().round(1), ' fee bps median', G.fee_bps.median())
G.to_pickle('graduations.pkl')
