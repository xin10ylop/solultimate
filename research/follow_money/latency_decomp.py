"""How much of the consistent cohort's profit survives OUR latency, if we copied their decisions
exactly (their entry moment and their exit moment)? Upper bound on what learning their logic can give.

For each cohort position (first buy j, first sell s): we enter `lag` trades after j (or at the next
slot) and exit at the state 1 slot after their sell. Exact curve math with our impact, fees 1.25%/side,
tx cost 0.0001 SOL/leg (non-racing) or measured live costs (racing)."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
W = sys.argv[2].split(',')
STAKE = 0.25
FEE = 0.0125
t, c, k = load(d)
t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
grp = np.zeros(len(wal), bool)
for i, a in enumerate(wal):
    if any(a.startswith(w) for w in W):
        grp[i] = True
buy = t['buy'].astype(bool)
vs = t['vsol'].astype(np.float64) / 1e9
vt = t['vtok'].astype(np.float64)
tok_amt = t['tok'].astype(np.float64)
sol_amt = t['sol'].astype(np.float64) / 1e9
order = np.argsort(t['mint'], kind='stable')
ms = t['mint'][order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], len(order)]
LAGS = ['own', 0, 2, 4, 8, 'next']
rows = []
for s, e in zip(st, en):
    ix = order[s:e]
    us = t['user'][ix]
    if not grp[us].any():
        continue
    bb = buy[ix]
    sl = t['slot'][ix].astype(np.int64)
    V, T = vs[ix], vt[ix]
    for w in np.unique(us[grp[us]]):
        p = np.flatnonzero(us == w)
        b = p[bb[p]]
        sls = p[~bb[p]]
        if len(b) == 0 or len(sls) == 0:
            continue
        j, x = b[0], sls[0]
        if x <= j:
            continue
        land = np.searchsorted(sl, sl[x] + 1, side='right') - 1
        land = max(land, x)
        for lag in LAGS:
            if lag == 'own':
                # their own fill: pre-state of their buy
                V0 = V[j] - sol_amt[ix][j]
                T0 = T[j] + tok_amt[ix][j]
            elif lag == 'next':
                q = np.searchsorted(sl, sl[j], side='right') - 1
                if q >= x:
                    continue
                V0, T0 = V[q], T[q]
            else:
                q = j + lag
                if q >= x:
                    continue
                V0, T0 = V[q], T[q]
            xn = STAKE * (1 - FEE)
            tk = T0 * xn / (V0 + xn)
            V1, T1 = V[land], T[land]
            T1s = T1 - tk
            if T1s <= 0:
                continue
            V1s = V1 * T1 / T1s
            out = V1s * tk / (T1s + tk) * (1 - FEE)
            rows.append((str(lag), (out - STAKE - 0.0002) / STAKE, (out - STAKE - 0.009) / STAKE))
R = pd.DataFrame(rows, columns=['lag', 'ret_cheap', 'ret_racing'])
print(os.path.basename(d.rstrip('/')), 'positions', R[R.lag == 'own'].shape[0])
print(R.groupby('lag', sort=False).agg(n=('ret_cheap', 'size'), mean=('ret_cheap', 'mean'), median=('ret_cheap', 'median'),
                                       win=('ret_cheap', lambda x: (x > 0).mean()), mean_racing_costs=('ret_racing', 'mean')).round(4).to_string())
