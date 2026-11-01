"""Dissect the consistent non-insider takers: position-level stats, context at entry, and whether
others' flow reacts to their buys (follower effect = not replicable)."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
name = os.path.basename(d.rstrip('/'))
cw = pd.read_csv('consistent_winners.csv')
sel = cw[(cw['coins_pit'] >= 200) & (cw['dev_pit'] < 0.2) & (cw['b0_pit'] < 0.2)]
targets = sel['addr'].tolist()
t, c, k = load(d)
t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
widx = {a: i for i, a in enumerate(wal)}
tid = {widx[a]: a for a in targets if a in widx}
n = len(t)
buy = t['buy'].astype(bool)
sol = t['sol'] / 1e9
cs = np.full(t['mint'].max() + 1, -1, np.int64)
cs[c['mint']] = c['slot']
dev = np.full(t['mint'].max() + 1, -1, np.int64)
dev[c['mint']] = c['dev']
mint = t['mint']
slot = t['slot'].astype(np.int64)
user = t['user']
vsol = t['vsol'] / 1e9
# per-mint index lists (tape order = execution order)
order = np.argsort(mint, kind='stable')
ms = mint[order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], n]
rows = []
for s, e in zip(st, en):
    ix = order[s:e]
    us = user[ix]
    hit = np.isin(us, list(tid.keys()))
    if not hit.any():
        continue
    m = mint[ix[0]]
    sl = slot[ix]
    bb = buy[ix]
    so = sol[ix]
    vs = vsol[ix]
    for w in np.unique(us[hit]):
        p = np.flatnonzero(us == w)
        fb = p[bb[p]]
        if len(fb) == 0:
            continue
        j0 = fb[0]
        t0 = sl[j0]
        # context before entry
        def flow(a, b, excl=True):
            q = (sl >= t0 + a) & (sl < t0 + b) & (us != w)
            q[j0] = False
            return np.sum(np.where(bb[q], so[q], -so[q])), int(np.sum(q & bb)), int(np.sum(q & ~bb))
        pre30 = flow(-142, 0)      # ~30 s before
        pre10 = flow(-47, 0)
        post5 = flow(1, 6)         # next 5 slots, others only
        post30 = flow(1, 143)
        cost = np.sum(so[p][bb[p]])
        proceeds = np.sum(so[p][~bb[p]])
        sells = p[~bb[p]]
        exit_slot = sl[sells[-1]] if len(sells) else -1
        rows.append(dict(wallet=tid[w][:8], mint=m, age_s=(t0 - cs[m]) * 0.211, vsol_in=vs[j0], size=so[j0],
                         nbuy=int(bb[p].sum()), nsell=int((~bb[p]).sum()), cost=cost, proceeds=proceeds,
                         hold_s=(exit_slot - t0) * 0.211 if exit_slot >= 0 else np.nan,
                         vsol_out=vs[sells[-1]] if len(sells) else np.nan,
                         pre30_net=pre30[0], pre30_nb=pre30[1], pre30_ns=pre30[2], pre10_net=pre10[0],
                         post5_net=post5[0], post5_nb=post5[1], post30_net=post30[0],
                         dev_sold_before=bool(np.any((us == dev[m]) & ~bb & (sl <= t0))),
                         same_slot_rank=int(np.sum((sl == t0) & (np.arange(len(ix)) < j0))),
                         ntrades_before=int(j0), peak_vs_before=float(vs[:j0 + 1].max())))
df = pd.DataFrame(rows)
df['ret'] = (df['proceeds'] - df['cost']) / df['cost']
df.to_csv(f'dive_{name}.csv', index=False)
pd.set_option('display.width', 250)
agg = df.groupby('wallet').agg(pos=('ret', 'size'), med_ret=('ret', 'median'), mean_ret=('ret', 'mean'),
                                win=('ret', lambda x: (x > 0).mean()), age=('age_s', 'median'), vsol_in=('vsol_in', 'median'),
                                size=('size', 'median'), nbuy=('nbuy', 'median'), hold=('hold_s', 'median'),
                                pre30=('pre30_net', 'median'), pre10=('pre10_net', 'median'), post5=('post5_net', 'median'),
                                post30=('post30_net', 'median'), devsold=('dev_sold_before', 'mean'),
                                drawdown_in=('peak_vs_before', 'median'))
print(name)
print(agg.round(3).to_string())
