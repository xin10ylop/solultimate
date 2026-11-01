"""Exit behaviour of the consistent slow takers: sell-spot / entry-spot multiple, the coin's best
and worst price during the hold, and how close to the in-hold peak they sold."""
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
pref = {}
for i, a in enumerate(wal):
    for w in W:
        if a.startswith(w):
            pref[i] = w
n = len(t)
buy = t['buy'].astype(bool)
tok = t['tok'].astype(np.float64)
vs = t['vsol'].astype(np.float64)
vt = t['vtok'].astype(np.float64)
spot = vs / vt
pre_vs = np.where(buy, vs - t['sol'], vs + t['sol'])
pre_vt = np.where(buy, vt + tok, vt - tok)
prespot = pre_vs / pre_vt
mint = t['mint']
slot = t['slot'].astype(np.int64)
user = t['user']
order = np.argsort(mint, kind='stable')
ms = mint[order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], n]
keys = np.array(list(pref.keys()))
rows = []
for s, e in zip(st, en):
    ix = order[s:e]
    us = user[ix]
    hit = np.isin(us, keys)
    if not hit.any():
        continue
    sp = spot[ix]
    for w in np.unique(us[hit]):
        p = np.flatnonzero(us == w)
        b = p[buy[ix][p]]
        sl_ = p[~buy[ix][p]]
        if len(b) == 0 or len(sl_) == 0:
            continue
        j0, j1 = b[0], sl_[-1]
        entry = sp[j0]
        path = sp[j0:j1 + 1]
        first_sell = sl_[0]
        rows.append(dict(w=pref[w], first_sell_mult=prespot[ix][first_sell] / entry,
                         last_sell_mult=prespot[ix][j1] / entry, max_in_hold=path.max() / entry,
                         min_in_hold=path.min() / entry, hold_s=(slot[ix][j1] - slot[ix][j0]) * 0.211,
                         first_sell_s=(slot[ix][first_sell] - slot[ix][j0]) * 0.211,
                         trades_between=int(first_sell - j0), nb=len(b), ns=len(sl_)))
df = pd.DataFrame(rows)
pd.set_option('display.width', 250)
for w, g in df.groupby('w'):
    print(f'== {w}: positions {len(g)}  buys/pos {g.nb.median():.0f} sells/pos {g.ns.median():.0f}')
    q = [.1, .25, .5, .75, .9]
    print('  first-sell multiple q:', np.round(g.first_sell_mult.quantile(q).values, 3))
    print('  max price in hold   q:', np.round(g.max_in_hold.quantile(q).values, 3))
    print('  min price in hold   q:', np.round(g.min_in_hold.quantile(q).values, 3))
    print('  first sell after s  q:', np.round(g.first_sell_s.quantile(q).values, 1), ' trades between q:', g.trades_between.quantile(q).values)
    h, b = np.histogram(np.log(g.first_sell_mult.clip(0.2, 5)), bins=np.arange(-0.5, 0.51, 0.025))
    top = np.argsort(-h)[:6]
    print('  most common first-sell multiples:', [(round(float(np.exp(b[i] + 0.0125)), 3), int(h[i])) for i in sorted(top)])
