"""Upper bound: enter where the cohort entered, but with OUR landing and costs.

Entry lands either (a) `lag` trades after the trigger trade, or (b) at the first trade of the next
slot (state after the trigger's whole slot). Exact constant-product curve math with fee on each
side, our own impact, tx cost per leg. Sells land `sell_lag_slots` slots after the exit decision
(state after all trades up to that slot).
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

SL = 0.211
FEE = 0.0125          # protocol 0.95% + creator 0.30% per side
TXC = 0.00003         # base + modest priority per leg, SOL (non-racing)
STAKE = 0.25

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
vs = t['vsol'].astype(np.float64) / 1e9
vt = t['vtok'].astype(np.float64)
mint = t['mint']
slot = t['slot'].astype(np.int64)
user = t['user']
cs = np.full(mint.max() + 1, -1, np.int64)
cs[c['mint']] = c['slot']
order = np.argsort(mint, kind='stable')
ms = mint[order]
st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
en = np.r_[st[1:], n]

EXITS = [('hold', h, None, None) for h in (3, 6, 10, 20, 40)] + \
        [('tpsl', 30, tp, sl) for tp in (0.04, 0.06, 0.10) for sl in (-0.06, -0.10)] + \
        [('tpsl', 60, tp, sl) for tp in (0.06, 0.10, 0.20) for sl in (-0.10, -0.20)]


def buy_tokens(V, T, x):
    xn = x * (1 - FEE)
    return T * xn / (V + xn)


def sell_sol(V, T, q):
    return V * q / (T + q) * (1 - FEE)


res = {(mode, e): [] for mode in ('lag4', 'nextslot') for e in range(len(EXITS))}
for s, e in zip(st, en):
    ix = order[s:e]
    us = user[ix]
    if not grp[us].any():
        continue
    bb = buy[ix]
    sl = slot[ix]
    V = vs[ix]
    T = vt[ix]
    _, first = np.unique(us, return_index=True)
    trig = [j for j in first if grp[us[j]] and bb[j] and 3 <= (sl[j] - cs[mint[ix[0]]]) * SL <= 600]
    for j in trig:
        for mode in ('lag4', 'nextslot'):
            if mode == 'lag4':
                q = j + 4
            else:
                q = np.searchsorted(sl, sl[j], side='right') - 1  # last trade of trigger slot
            if q >= len(ix) - 1:
                continue
            V0, T0 = V[q], T[q]
            # our buy changes the curve: shift subsequent states by our tokens (k-invariant approx)
            tok = buy_tokens(V0, T0, STAKE)
            k0 = V0 * T0
            t_enter = sl[q] + (0 if mode == 'lag4' else 1)
            for ei, (kind, hmax, tp, slp) in enumerate(EXITS):
                exit_q = None
                for r in range(q + 1, len(ix)):
                    if sl[r] < t_enter:
                        continue
                    # value of our tokens at state r, with our tokens included in the curve
                    Tr = T[r] - tok
                    Vr = V[r] * T[r] / Tr if Tr > 0 else V[r]
                    val = sell_sol(Vr, Tr, tok)
                    ret = (val - STAKE) / STAKE
                    el = (sl[r] - t_enter) * SL
                    if kind == 'hold' and el >= hmax:
                        exit_q = r
                        break
                    if kind == 'tpsl' and (ret >= tp or ret <= slp or el >= hmax):
                        exit_q = r
                        break
                if exit_q is None:
                    exit_q = len(ix) - 1
                # sell lands 1 slot after the decision: use last state of decision slot + 1
                land = np.searchsorted(sl, sl[exit_q] + 1, side='right') - 1
                land = max(land, exit_q)
                Tr = T[land] - tok
                Vr = V[land] * T[land] / Tr if Tr > 0 else V[land]
                val = sell_sol(Vr, Tr, tok)
                res[(mode, ei)].append((val - STAKE - 2 * TXC) / STAKE)
print(os.path.basename(d.rstrip('/')))
print('mode      exit                     n     mean     median  win    w/o best2')
for (mode, ei), r in res.items():
    r = np.array(r)
    if len(r) == 0:
        continue
    kind, h, tp, slp = EXITS[ei]
    lab = f'{kind} {h}s' + (f' tp{tp:+.2f} sl{slp:+.2f}' if tp is not None else '')
    srt = np.sort(r)
    print(f'{mode:9s} {lab:24s} {len(r):5d}  {r.mean():+.4f}  {np.median(r):+.4f}  {(r > 0).mean():.3f}  {srt[:-2].mean():+.4f}')
