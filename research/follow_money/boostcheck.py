"""Verify the boost front-run: decompose price moves around each burn and look for existing front-runners."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd

B = pd.read_pickle('boost_events.pkl').sort_values(['pool', 'slot'])
T = pd.read_pickle('pool_trades.pkl').sort_values(['pool', 'slot', 'tx'])
T['Qpre'] = T.qres + T.vq
byp = {k: g for k, g in T.groupby('pool')}
LEAD, LAG = 2, 1
rows = []
pairs = []
for pool, g in B.groupby('pool'):
    tr = byp.get(pool)
    if tr is None:
        continue
    ts, txs = tr.slot.values, tr.tx.values
    Q, Bb = tr.Qpre.values, tr.base_res.values
    sl = g.slot.values
    for j in range(1, len(g) - 1):
        b = g.iloc[j + 1]
        pred = sl[j] + (sl[j] - sl[j - 1])
        actual = int(b.slot)
        i0 = np.searchsorted(ts, pred - LEAD, side='left')
        i1 = np.searchsorted(ts, actual + LAG, side='left')
        if i0 >= len(ts) or i1 >= len(ts):
            continue
        p_entry = Q[i0] / Bb[i0]
        burned = b.used  # quote used
        Qpost, Bpost = b.Q, b.base_after
        # base burned = tokens bought: Bpre - Bpost with k-invariant pre-state
        Qpre = Qpost - b.used
        Bpre = Qpost * Bpost / Qpre
        p_pre, p_post = Qpre / Bpre, Qpost / Bpost
        p_exit = Q[i1] / Bb[i1]
        if ts[i0] > actual:
            p_entry = p_pre
        rows.append(dict(pool=pool, err=actual - pred, pre=np.log(p_pre / p_entry), jump=np.log(p_post / p_pre),
                         post=np.log(p_exit / p_post), total=np.log(p_exit / p_entry), used=b.used, Q=b.Q, k=j + 1))
        # trades by others in the window around this burn
        w = tr[(tr.slot >= actual - 4) & (tr.slot <= actual + 4)]
        for r in w.itertuples():
            before = (r.slot < actual) or (r.slot == actual and r.tx < b.tx)
            pairs.append((pool, actual, r.user, r.buy, before, r.quote))
R = pd.DataFrame(rows)
print('cycles', len(R), ' burn timing error vs prediction (slots):', R.err.describe().round(2).to_dict())
print('mean log moves: pre-burn drift %+.4f  burn jump %+.4f  post-burn drift %+.4f  total %+.4f' % (R.pre.mean(), R.jump.mean(), R.post.mean(), R.total.mean()))
print('median          pre %+.4f  jump %+.4f  post %+.4f  total %+.4f' % (R.pre.median(), R.jump.median(), R.post.median(), R.total.median()))
print('burn jump by burn index in the program:')
R['kb'] = pd.cut(R.k, [0, 3, 6, 10, 15, 20, 40])
print(R.groupby('kb', observed=True).agg(n=('jump', 'size'), jump=('jump', 'mean'), total=('total', 'mean'), used=('used', 'mean'), Q=('Q', 'median')).round(4).to_string())
P = pd.DataFrame(pairs, columns=['pool', 'burn', 'user', 'buy', 'before', 'quote'])
# front-runner signature: buy before the burn and sell after it, same wallet, same burn
fb = P[P.buy & P.before].groupby(['pool', 'burn', 'user']).size()
sa = P[(~P.buy) & (~P.before)].groupby(['pool', 'burn', 'user']).size()
both = fb.index.intersection(sa.index)
fr = pd.Series([u for (_, _, u) in both]).value_counts()
print(f'burns with a buy-before & sell-after wallet: {len(set((p, b) for (p, b, _) in both))} of {R.shape[0]}')
print('wallets doing it most often:', fr.head(10).to_dict())
R.to_pickle('boostcheck.pkl')
