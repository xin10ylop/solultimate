"""Graduation boost program: per SOL-paired graduation, the crank's buy schedule and a
'ride the boost' simulation (enter after pool open with our landing, exit after T seconds).

State of the pool at any slot = pre-trade reserves of the first trade at/after that slot
(effective quote = vault + virtual). Exact constant-product math, the pool's fee tier on both
legs, our own impact, tx cost per leg."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from pools import load_pools

d = sys.argv[1]
SL = 0.211
TXC = 0.0001
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
WSOL = 'So11111111111111111111111111111111111111112'
wsol = mints.index(WSOL) if WSOL in mints else -1
cr = p[(p['kind'] == 0) & (p['qmint'] == wsol)]
cr = cr[[mints[m].endswith('pump') for m in cr['mint']]]
print(f'SOL-paired pump graduations (pool creations): {len(cr)}; init_boost events {(p["kind"] == 4).sum()}')
ev = pd.DataFrame({'kind': p['kind'], 'pool': p['pool'], 'slot': p['slot'].astype(np.int64), 'tx': p['tx'], 'ev': p['ev'],
                   'quote': p['quote'] / 1e9, 'qres': p['quote_res'] / 1e9, 'vq': p['vq'] / 1e9, 'B': p['base_res'].astype(np.float64),
                   'fee_bps': p['fee_bps'], 'rem': p['extra'] / 1e9, 'user': p['user']})
ev = ev[ev.pool.isin(set(cr['pool'].tolist()))].sort_values(['pool', 'slot', 'tx', 'ev'])
byp = {k: g for k, g in ev.groupby('pool')}
rows, sims = [], []
HOLD = [15, 30, 60, 120, 240, 360]
LAG = [1, 3]
STAKE = [0.25, 1.0]
for r in cr:
    pool = int(r['pool'])
    s0 = int(r['slot'])
    g = byp.get(pool)
    if g is None:
        continue
    tr = g[(g.kind == 1) | (g.kind == 2)]
    bb = g[g.kind == 3]
    if len(tr) < 3:
        continue
    fee_bps = int(np.median(tr.fee_bps.values[:30]))
    vault0 = (bb.quote.iloc[0] + bb.rem.iloc[0]) if len(bb) else 0.0
    Qopen = r['quote_res'] / 1e9 + (tr.vq.iloc[0] if len(tr) else 0)
    rows.append(dict(pool=pool, mint=mints[r['mint']][:10], fee_bps=fee_bps, burns=len(bb), vault0=vault0, spent=bb.quote.sum(),
                     first_burn_s=(bb.slot.iloc[0] - s0) * SL if len(bb) else np.nan,
                     last_burn_s=(bb.slot.iloc[-1] - s0) * SL if len(bb) else np.nan,
                     med_gap=bb.slot.diff().median() if len(bb) > 1 else np.nan, Q0=r['quote_res'] / 1e9, vq0=tr.vq.iloc[0],
                     buy_vol_6m=tr[(tr.kind == 1) & (tr.slot <= s0 + 360 / SL)].quote.sum(),
                     sell_vol_6m=tr[(tr.kind == 2) & (tr.slot <= s0 + 360 / SL)].quote.sum()))
    ts = tr.slot.values
    Qs = (tr.qres + tr.vq).values
    Bs = tr.B.values
    f = fee_bps / 1e4

    def st(slot):
        i = np.searchsorted(ts, slot, side='left')
        return (Qs[i], Bs[i]) if i < len(ts) else None
    for lag in LAG:
        a = st(s0 + lag)
        if a is None:
            continue
        Q0, B0 = a
        for h in HOLD:
            b = st(s0 + lag + int(h / SL))
            if b is None:
                b = (Qs[-1], Bs[-1])
            Q1, B1 = b
            for x in STAKE:
                xn = x * (1 - f)
                tok = B0 * xn / (Q0 + xn)
                k1 = Q1 * B1
                B1s = B1 - tok
                if B1s <= 0:
                    continue
                Q1s = k1 / B1s
                out = Q1s * tok / (B1s + tok) * (1 - f)
                sims.append((pool, lag, h, x, (out - x - 2 * TXC) / x, len(bb)))
G = pd.DataFrame(rows)
S = pd.DataFrame(sims, columns=['pool', 'lag', 'hold', 'stake', 'ret', 'burns'])
pd.set_option('display.width', 250)
print(f'graduations analysed: {len(G)}   with a boost program (>=1 burn): {(G.burns > 0).sum()}')
print(G[['fee_bps', 'burns', 'vault0', 'spent', 'first_burn_s', 'last_burn_s', 'med_gap', 'Q0', 'vq0', 'buy_vol_6m', 'sell_vol_6m']].describe().round(2).to_string())
print('--- ride from pool open (entry lag slots), exit after hold seconds')
agg = S.groupby(['lag', 'hold', 'stake']).agg(n=('ret', 'size'), mean=('ret', 'mean'), med=('ret', 'median'), win=('ret', lambda x: (x > 0).mean()),
                                              wo2=('ret', lambda x: np.sort(x.values)[:-2].mean()))
print(agg.round(4).to_string())
G.to_pickle('gradboost.pkl')
S.to_pickle('gradboost_sims.pkl')
