"""Is the consistent takers' edge in *selection*? Forward spot returns after their first buys vs
all other wallets' first buys matched on coin age and curve level."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
W = sys.argv[2].split(',')
t, c, k = load(d)
t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
grp = np.zeros(len(wal), bool)
for i, a in enumerate(wal):
    if any(a.startswith(w) for w in W):
        grp[i] = True
n = len(t)
buy = t['buy'].astype(bool)
spot = t['vsol'].astype(np.float64) / t['vtok'].astype(np.float64)
mint = t['mint']
slot = t['slot'].astype(np.int64)
user = t['user']
cs = np.full(mint.max() + 1, -1, np.int64)
cs[c['mint']] = c['slot']
dev = np.full(mint.max() + 1, -1, np.int64)
dev[c['mint']] = c['dev']
order = np.argsort(mint, kind='stable')
ms = mint[order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], n]
H = [5, 15, 30, 60, 120, 300]
out = []
for s, e in zip(st, en):
    ix = order[s:e]
    us = user[ix]
    bb = buy[ix]
    sl = slot[ix]
    sp = spot[ix]
    m = mint[ix[0]]
    age = (sl - cs[m]) * 0.211
    # first buy per wallet, ages 3..600 s, exclude the dev
    _, first = np.unique(us, return_index=True)
    first = first[bb[first] & (age[first] >= 3) & (age[first] <= 600) & (us[first] != dev[m])]
    for j in first:
        r = [sp[j]]
        for h in H:
            q = np.searchsorted(sl, sl[j] + int(h / 0.211), side='right') - 1
            r.append(sp[q] / sp[j])
        vs = t['vsol'][ix[j]] / 1e9
        out.append((grp[us[j]], age[j], vs, *r[1:]))
df = pd.DataFrame(out, columns=['grp', 'age', 'vsol'] + [f'f{h}' for h in H])
df['ab'] = pd.cut(df.age, [3, 10, 30, 60, 120, 300, 600])
df['vb'] = pd.cut(df.vsol, [30, 35, 40, 50, 60, 80, 116])
print(f'{os.path.basename(d)}: first buys {len(df)}  group {df.grp.sum()}')
# matched comparison: within each (age, vsol) cell, group mean of log forward return minus others' mean
res = []
for h in H:
    col = f'f{h}'
    df['l'] = np.log(df[col].clip(1e-3))
    g = df.groupby(['ab', 'vb', 'grp'], observed=True)['l'].agg(['mean', 'size']).unstack('grp')
    g = g.dropna()
    w = g[('size', True)]
    diff = (g[('mean', True)] - g[('mean', False)])
    res.append((h, float((diff * w).sum() / w.sum()), float(df[df.grp]['l'].mean()), float(df[~df.grp]['l'].mean()),
                float((df[df.grp][col] > 1).mean()), float((df[~df.grp][col] > 1).mean())))
print('horizon  matched excess log-ret  group mean  others mean  P(up) group  P(up) others')
for h, dx, gm, om, pg, po in res:
    print(f'{h:5d}s   {dx:+.4f}             {gm:+.4f}     {om:+.4f}      {pg:.3f}        {po:.3f}')
