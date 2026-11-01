"""Do sells cluster at preset take-profit / stop-loss multiples of each wallet's entry?

For every sell we compute the wallet's PnL multiple on the position at the moment of the sell,
using the spot price *before* the sell (what a bot's trigger sees) versus the wallet's average
entry spot price. Mechanical TP/SL presets show up as spikes in the histogram.
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from tape import load, clean

d = sys.argv[1]
t, c, k = load(d)
t, c = clean(t, c)
n = len(t)
buy = t['buy'].astype(bool)
tok = t['tok'].astype(np.float64)
vs = t['vsol'].astype(np.float64)
vt = t['vtok'].astype(np.float64)
# spot after the trade and before it
post = vs / vt
pre_vs = np.where(buy, vs - t['sol'], vs + t['sol'])
pre_vt = np.where(buy, vt + tok, vt - tok)
pre = pre_vs / pre_vt

# per (mint,user) running position, in execution order (tape order is execution order)
key = t['mint'].astype(np.uint64) << 32 | t['user'].astype(np.uint64)
order = np.lexsort((np.arange(n), key))  # group by key, stable within
ks = key[order]
starts = np.r_[0, np.flatnonzero(ks[1:] != ks[:-1]) + 1]
ends = np.r_[starts[1:], n]

pos_tok = np.zeros(n)
avg_entry_spot = np.full(n, np.nan)   # avg of post-buy spot weighted by tokens
avg_entry_exec = np.full(n, np.nan)
first_buy_slot = np.zeros(n)
frac = np.full(n, np.nan)
nbuys = np.zeros(n)
sol = t['sol'].astype(np.float64)
for s, e in zip(starts, ends):
    held = 0.0
    wspot = 0.0
    cost = 0.0
    fb = -1
    nb = 0
    for j in order[s:e]:
        if buy[j]:
            held += tok[j]
            wspot += tok[j] * post[j]
            cost += sol[j]
            nb += 1
            if fb < 0:
                fb = t['slot'][j]
        else:
            if held > 0:
                pos_tok[j] = held
                avg_entry_spot[j] = wspot / held
                avg_entry_exec[j] = cost / held
                first_buy_slot[j] = fb
                frac[j] = min(tok[j] / held, 1.0)
                nbuys[j] = nb
                r = max(held - tok[j], 0.0) / held
                wspot *= r
                cost *= r
                held = max(held - tok[j], 0.0)
sell = (~buy) & np.isfinite(avg_entry_spot)
mult_spot = pre[sell] / avg_entry_spot[sell]
mult_exec = (sol[sell] / tok[sell]) / avg_entry_exec[sell]
fr = frac[sell]
age = (t['slot'][sell] - first_buy_slot[sell]) * 0.211
nb = nbuys[sell]
print(f'{d}: trades {n} sells-with-entry {sell.sum()}')

def hist(x, label, lo=-2.5, hi=3.0, w=0.02):
    x = np.log(x[np.isfinite(x) & (x > 0)])
    b = np.arange(lo, hi + w, w)
    h, _ = np.histogram(x, b)
    # local spike score: count / mean of the 10 neighbouring bins on each side (excl. +/-1)
    sm = np.convolve(h, np.ones(21) / 21, mode='same')
    sc = h / np.maximum(sm, 1)
    top = np.argsort(-sc)[:25]
    print(f'--- {label}: n={len(x)}  top spikes (multiple, count, spike ratio)')
    for i in sorted(top, key=lambda i: b[i]):
        if h[i] >= 50:
            print(f'   x{np.exp(b[i] + w / 2):6.3f}  n={h[i]:6d}  spike {sc[i]:4.2f}')

full = fr > 0.98
hist(mult_spot, 'all sells, spot-before / avg entry spot')
hist(mult_spot[full], 'full exits (>=98% of position), spot multiple')
hist(mult_spot[~full], 'partial sells, spot multiple')
hist(mult_exec[full], 'full exits, exec-price multiple')
# fraction sold spikes (e.g. sell 50% at TP1)
fb = np.arange(0, 1.0001, 0.01)
h, _ = np.histogram(fr, fb)
print('--- fraction of position sold, top bins:')
for i in np.argsort(-h)[:12]:
    print(f'   {fb[i]:.2f}-{fb[i+1]:.2f}: {h[i]}')
np.savez_compressed(d.rstrip('/') + '_sells.npz', mult_spot=mult_spot, mult_exec=mult_exec, frac=fr, age=age, nb=nb,
                    idx=np.flatnonzero(sell))
