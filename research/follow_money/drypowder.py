"""Operator dry powder: wallets funded by the launch's operator funders that have not traded the
coin yet at decision time. Do they buy later, and does price follow?

Decision time = end of the create slot (+ our landing: next slot). Everything used is known then.
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean, TR, CO

SL = 0.211
d = sys.argv[1]
name = os.path.basename(d.rstrip('/'))
# creates.bin may be 24- or 28-byte rows
csize = os.path.getsize(os.path.join(d, 'creates.bin'))
CR = np.dtype([('mint', '<u4'), ('dev', '<u4'), ('slot', '<u4'), ('tx', '<u4'), ('flags', '<u4'), ('bt', '<u4'), ('qmint', '<u4')])
c = np.fromfile(os.path.join(d, 'creates.bin'), dtype=CR)
t = np.fromfile(os.path.join(d, 'trades.bin'), dtype=TR)
t, c = clean(t, c)
FU = np.dtype([('frm', '<u4'), ('to', '<u4'), ('slot', '<u4'), ('tx', '<u4'), ('lam', '<u8')])
f = np.fromfile(os.path.join(d, 'funds.bin'), dtype=FU)
F = pd.DataFrame({'frm': f['frm'], 'to': f['to'], 'slot': f['slot'].astype(np.int64), 'sol': f['lam'] / 1e9})
# hubs: funders reaching many distinct wallets per hour of data (exchanges, disperse services)
hours = (t['slot'].max() - t['slot'].min()) * SL / 3600
fan = F.groupby('frm').to.nunique() / max(hours, 1)
hubs = set(fan[fan > 40].index)
F = F[~F.frm.isin(hubs)]
print(f'{name}: fundings {len(f)}  non-hub {len(F)}  hub funders excluded {len(hubs)}')
# last funding of each wallet before a given slot: index by recipient
F = F.sort_values('slot')
by_to = {k: (g.slot.values, g.frm.values) for k, g in F.groupby('to')}
by_frm = {k: (g.slot.values, g.to.values, g.sol.values) for k, g in F.groupby('frm')}
buy = t['buy'].astype(bool)
spot = t['vsol'].astype(np.float64) / t['vtok'].astype(np.float64)
order = np.argsort(t['mint'], kind='stable')
ms = t['mint'][order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], len(order)]
cinfo = {int(m): (int(s), int(dv)) for m, s, dv in zip(c['mint'], c['slot'], c['dev'])}
W6 = int(6 * 3600 / SL)
H = [15, 30, 60, 120, 300]
rows = []
for s, e in zip(st, en):
    ix = order[s:e]
    mint = int(t['mint'][ix[0]])
    if mint not in cinfo:
        continue
    cs, dev = cinfo[mint]
    sl = t['slot'][ix].astype(np.int64)
    us = t['user'][ix]
    bb = buy[ix]
    sp = spot[ix]
    sol = t['sol'][ix] / 1e9
    b0 = (sl == cs)
    seeds = set(us[b0 & bb].tolist()) | {dev}
    funders = set()
    for w in seeds:
        g = by_to.get(w)
        if g is None:
            continue
        sl_f, fr = g
        k = np.searchsorted(sl_f, cs, side='right') - 1
        if k >= 0 and sl_f[k] >= cs - W6:
            funders.add(int(fr[k]))
    # dry powder: wallets funded by these funders within 6 h before decision, not seeds, no trade yet
    t_dec = cs + 1
    traded_by_dec = set(us[sl <= cs].tolist())
    dp_w, dp_sol = set(), 0.0
    for fr in funders:
        sl_f, to, amt = by_frm[fr]
        msk = (sl_f <= cs) & (sl_f >= cs - W6)
        for w, a in zip(to[msk], amt[msk]):
            if w not in seeds and w not in traded_by_dec and w not in dp_w:
                dp_w.add(int(w))
                dp_sol += a
    # outcome: later buys by dry-powder wallets on this coin, and price path from our entry
    later = (sl > cs)
    dp_buys = np.isin(us, list(dp_w)) & bb & later
    i_entry = np.searchsorted(sl, t_dec + 1, side='left')  # first trade at/after our landing slot
    if i_entry >= len(sl):
        continue
    p0 = sp[i_entry - 1] if i_entry > 0 else sp[0]
    r = dict(mint=mint, b0_n=int((b0 & bb).sum()), b0_sol=float(sol[b0 & bb].sum()), funders=len(funders), dp_wallets=len(dp_w),
             dp_sol=dp_sol, dp_buyers=int(len(set(us[dp_buys].tolist()))), dp_buy_sol=float(sol[dp_buys].sum()),
             dp_buy_sol_60=float(sol[dp_buys & (sl <= cs + 60 / SL)].sum()))
    for h in H:
        k = np.searchsorted(sl, t_dec + 1 + int(h / SL), side='right') - 1
        r[f'f{h}'] = np.log(sp[max(k, 0)] / p0)
    rows.append(r)
D = pd.DataFrame(rows)
D.to_pickle(f'dp_{name}.pkl')
pd.set_option('display.width', 220)
print('launches', len(D), ' with operator funders found', (D.funders > 0).sum(), ' with dry powder > 0', (D.dp_wallets > 0).sum())
print('dry-powder wallets that later bought the coin: share of launches with any', round((D.dp_buyers > 0).mean(), 3))
D['dp_b'] = pd.cut(D.dp_sol, [-1, 0, 0.5, 2, 5, 15, 50, 1e9])
agg = D.groupby('dp_b', observed=True).agg(n=('mint', 'size'), dp_wallets=('dp_wallets', 'median'), dp_buyers=('dp_buyers', 'mean'),
                                           dp_buy_sol=('dp_buy_sol', 'mean'), f15=('f15', 'mean'), f60=('f60', 'mean'), f120=('f120', 'mean'), f300=('f300', 'mean'),
                                           up60=('f60', lambda x: (x > 0.03).mean()))
print('by operator dry powder (SOL) at decision time:')
print(agg.round(4).to_string())
