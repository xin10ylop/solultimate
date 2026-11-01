"""Reserve rewrites on mayhem coins: discontinuities between consecutive curve trades
(pre-state of trade i != post-state of trade i-1). When in the coin's life do they happen, how big
are they, and what does price do around them?"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load

SL = 0.211
d = sys.argv[1]
import numpy as _np
t, c, k = load(d) if os.path.getsize(os.path.join(d, 'creates.bin')) % 28 == 0 and os.path.getsize(os.path.join(d, 'creates.bin')) % 24 != 0 else (None, None, None)
if t is None:
    from tape import TR, CO
    CR24 = _np.dtype([('mint', '<u4'), ('dev', '<u4'), ('slot', '<u4'), ('tx', '<u4'), ('flags', '<u4'), ('bt', '<u4')])
    t = _np.fromfile(os.path.join(d, 'trades.bin'), dtype=TR)
    c = _np.fromfile(os.path.join(d, 'creates.bin'), dtype=CR24)
    k = _np.fromfile(os.path.join(d, 'completes.bin'), dtype=CO)
mc = c[(c['flags'] & 1) != 0]
cs = dict(zip(mc['mint'].tolist(), mc['slot'].tolist()))
m = np.isin(t['mint'], list(cs.keys())) & (t['tok'] > 0)
t = t[m]
buy = t['buy'].astype(bool)
vs = t['vsol'].astype(np.float64)
vt = t['vtok'].astype(np.float64)
sol = t['sol'].astype(np.float64)
tok = t['tok'].astype(np.float64)
pre_vs = np.where(buy, vs - sol, vs + sol)
pre_vt = np.where(buy, vt + tok, vt - tok)
order = np.lexsort((np.arange(len(t)), t['mint']))
rows = []
mint = t['mint'][order]
st = np.r_[0, np.flatnonzero(mint[1:] != mint[:-1]) + 1]
en = np.r_[st[1:], len(order)]
ntr = 0
for s, e in zip(st, en):
    ix = order[s:e]
    mm = int(t['mint'][ix[0]])
    c0 = cs[mm]
    ntr += len(ix)
    for a, b in zip(ix[:-1], ix[1:]):
        if vs[a] == pre_vs[b] and vt[a] == pre_vt[b]:
            continue
        p0 = vs[a] / vt[a]
        p1 = pre_vs[b] / pre_vt[b]
        rows.append(dict(mint=mm, slot=int(t['slot'][b]), age_h=(int(t['slot'][b]) - c0) * SL / 3600,
                         jump=p1 / p0 - 1, dvsol=(pre_vs[b] - vs[a]) / 1e9, dvtok=(pre_vt[b] - vt[a]) / vt[a], gap_slots=int(t['slot'][b]) - int(t['slot'][a])))
B = pd.DataFrame(rows)
print(f'{os.path.basename(d.rstrip("/"))}: mayhem coins {len(cs)}, trades {ntr}, discontinuities {len(B)} ({len(B) / max(ntr, 1):.4%} of trades)')
if len(B):
    print('age at discontinuity (h):', np.round(B.age_h.quantile([.01, .1, .25, .5, .75, .9, .99]).values, 3))
    B['ab'] = pd.cut(B.age_h, [-0.01, 0.01, 0.1, 1, 6, 12, 23, 23.9, 23.99, 24.01, 24.1, 30, 1000])
    print(B.groupby('ab', observed=True).agg(n=('jump', 'size'), coins=('mint', 'nunique'), med_jump=('jump', 'median'),
                                             mean_jump=('jump', 'mean'), up=('jump', lambda x: (x > 0).mean()),
                                             dvtok=('dvtok', 'median'), dvsol=('dvsol', 'median')).round(4).to_string())
    print('jump size quantiles:', np.round(B.jump.quantile([.01, .1, .25, .5, .75, .9, .99]).values, 4))
B.to_pickle(f'mbreaks_{os.path.basename(d.rstrip("/"))}.pkl')
