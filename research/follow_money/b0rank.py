"""Create-slot buyers: realized position PnL (fees incl.) by rank inside the create slot,
split into dev-bundled (same tx as create or adjacent tx with same signer) and independent."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
t, c, k = load(d)
t, c = clean(t, c)
n = len(t)
buy = t['buy'].astype(bool)
sol = t['sol'].astype(np.float64) / 1e9
fee = t['fee'].astype(np.float64) / 1e9
tok = t['tok'].astype(np.float64)
vs = t['vsol'].astype(np.float64) / 1e9
vt = t['vtok'].astype(np.float64)
mint = t['mint']
slot = t['slot'].astype(np.int64)
txi = t['tx'].astype(np.int64)
user = t['user']
signer = t['signer']
cs = np.full(mint.max() + 1, -1, np.int64)
ctx = np.full(mint.max() + 1, -1, np.int64)
dev = np.full(mint.max() + 1, -1, np.int64)
cs[c['mint']] = c['slot']
ctx[c['mint']] = c['tx']
dev[c['mint']] = c['dev']
order = np.argsort(mint, kind='stable')
ms = mint[order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], n]
rows = []
for s, e in zip(st, en):
    ix = order[s:e]
    m = mint[ix[0]]
    sl = slot[ix]
    b0 = np.flatnonzero(sl == cs[m])
    if len(b0) == 0:
        continue
    us = user[ix]
    bb = buy[ix]
    tx = txi[ix]
    sg = signer[ix]
    dev_sg = set(sg[(us == dev[m])].tolist()) | {dev[m]}
    # final liquidation value per token for unsold holdings (pro-rata net SOL in curve)
    netsol = np.sum(np.where(bb, sol[ix], -sol[ix]))
    seen = set()
    rank = 0
    for j in b0:
        if not bb[j] or us[j] == dev[m]:
            rank += 1
            continue
        w = us[j]
        if w in seen:
            rank += 1
            continue
        seen.add(w)
        p = us == w
        cost = np.sum((sol[ix] + fee[ix])[p & bb])
        proceeds = np.sum((sol[ix] - fee[ix])[p & ~bb])
        held = max(np.sum(tok[ix][p & bb]) - np.sum(tok[ix][p & ~bb]), 0)
        outst = max(np.sum(np.where(bb, tok[ix], -tok[ix])), 1)
        mark = held * min(netsol / outst, vs[ix][-1] * 1e9 / vt[ix][-1] / 1e9)
        bundled = (tx[j] == ctx[m]) or (sg[j] in dev_sg) or (abs(tx[j] - ctx[m]) <= 4 and tx[j] > ctx[m] and np.sum((sl == sl[j]) & (tx < tx[j]) & (tx > ctx[m])) <= 4)
        rows.append(dict(mint=m, rank=rank, same_tx=tx[j] == ctx[m], tx_gap=tx[j] - ctx[m], bundled=bundled,
                         cost=cost, pnl=proceeds + mark - cost))
        rank += 1
df = pd.DataFrame(rows)
df['ret'] = df.pnl / df.cost
df['rb'] = pd.cut(df['rank'], [-1, 0, 2, 5, 10, 20, 1000])
print(os.path.basename(d.rstrip('/')), 'create-slot non-dev buyer positions', len(df))
print(df.groupby(['same_tx']).agg(n=('ret', 'size'), roi=('pnl', 'sum'), cost=('cost', 'sum'), win=('ret', lambda x: (x > 0).mean())).assign(roi=lambda x: x.roi / x.cost).round(3).to_string())
nd = df[~df.same_tx]
print('not in create tx, by trades ahead in the create slot:')
print(nd.groupby('rb', observed=True).agg(n=('ret', 'size'), pnl=('pnl', 'sum'), cost=('cost', 'sum'), win=('ret', lambda x: (x > 0).mean()), med=('ret', 'median')).assign(roi=lambda x: x.pnl / x.cost).round(3).to_string())
nd2 = nd.copy()
nd2['gb'] = pd.cut(nd2.tx_gap, [0, 1, 2, 4, 8, 16, 64, 100000])
print('not in create tx, by tx-index gap after the create tx:')
print(nd2.groupby('gb', observed=True).agg(n=('ret', 'size'), pnl=('pnl', 'sum'), cost=('cost', 'sum'), win=('ret', lambda x: (x > 0).mean()), med=('ret', 'median')).assign(roi=lambda x: x.pnl / x.cost).round(3).to_string())
