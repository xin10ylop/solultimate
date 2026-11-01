"""Front-run the boost crank, measured on the real tape.

For each burn after the first on a pool, the next burn time is *predicted* from the previous
gap (no look-ahead). We enter `lead` slots before the predicted burn at the pool's effective price
then (last trade state at or before that slot), and exit `lag` slots after the burn actually lands
(state from the first trade at/after that slot). Constant-product exact math on the effective
reserves, the pool's own fee tier on both legs, our own impact, and a fixed tx cost per leg.
If the burn does not happen within `lead + 20` slots of entry we exit at that point (miss).
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd

B = pd.read_pickle('boost_events.pkl')
T = pd.read_pickle('pool_trades.pkl')
STAKES = [0.1, 0.5, 2.0]
TXC = 0.0001  # SOL per leg (priority + base; we are not racing)
leads = [1, 2, 4]
lags = [1, 2, 4]

T = T.sort_values(['pool', 'slot', 'tx'])
T['Qpre'] = T.qres + T.vq
by_pool = {k: g for k, g in T.groupby('pool')}


def state_at(pool, slot, before=True):
    """Effective (Q, B) reserves at the start of `slot` (before=True) or after trades in it."""
    g = by_pool.get(pool)
    if g is None:
        return None
    s = g.slot.values
    i = np.searchsorted(s, slot, side='left' if before else 'right')
    if i >= len(g):
        return None
    r = g.iloc[i]
    return r.Qpre, r.base_res, int(r.slot), r.fee_bps


rows = []
for pool, g in B.groupby('pool'):
    g = g.sort_values('slot')
    sl = g.slot.values
    for j in range(1, len(g) - 1):
        gap = sl[j] - sl[j - 1]
        pred = sl[j] + gap  # predicted next burn
        actual = sl[j + 1]
        for lead in leads:
            entry_slot = pred - lead
            st = state_at(pool, entry_slot, before=True)
            if st is None:
                continue
            Q0, B0, s0, fee_bps = st
            if s0 > actual:  # no trade between entry and burn: use burn pre-state
                b = g.iloc[j + 1]
                Q0, B0 = b.Q - b.used, b.base_after + 0
            f = fee_bps / 1e4
            # ex-ante size of the next burn: same spend fraction applied to the vault left after burn j
            pu = g.iloc[j].frac * g.iloc[j].rem
            pred_imp = ((Q0 + pu) / Q0) ** 2 - 1
            hit = actual <= entry_slot + lead + 20 and actual >= entry_slot
            for lag in lags:
                exit_slot = (actual if hit else entry_slot + lead + 20) + lag
                st2 = state_at(pool, exit_slot, before=True)
                if st2 is None:
                    continue
                Q1, B1 = st2[0], st2[1]
                for x in STAKES:
                    xn = x * (1 - f)
                    tok = B0 * xn / (Q0 + xn)
                    # our tokens are still in the market: shift exit state by our buy (k-invariant approx)
                    k1 = Q1 * B1
                    B1s = B1 - tok
                    Q1s = k1 / B1s if B1s > 0 else Q1
                    out = Q1s * tok / (B1s + tok) * (1 - f)
                    rows.append((pool, lead, lag, x, hit, (out - x - 2 * TXC) / x, fee_bps, g.iloc[j + 1].used, g.iloc[j + 1].Q, pred_imp))
R = pd.DataFrame(rows, columns=['pool', 'lead', 'lag', 'stake', 'hit', 'ret', 'fee_bps', 'used', 'Q', 'pred_imp'])
R['impact'] = (R.Q / (R.Q - R.used)) ** 2 - 1
R['rt'] = 2 * R.fee_bps / 1e4
pd.set_option('display.width', 250)
print('cycles', R.groupby(['lead', 'lag', 'stake']).size().iloc[0], ' hit rate (burn within window)', R.hit.mean().round(3))
print(R.groupby(['lead', 'lag', 'stake']).agg(mean=('ret', 'mean'), med=('ret', 'median'), win=('ret', lambda x: (x > 0).mean())).round(4).to_string())
# selective: only pools/cycles where the *predicted* impact (from previous burn) beats fees
for m in (0.0, 0.005, 0.01, 0.02):
    s = R[R.pred_imp > R.rt + m]
    print(f'--- ex-ante selective: predicted impact of the next burn > round-trip fee + {m:.3f}')
    print(s.groupby(['lead', 'lag', 'stake']).agg(n=('ret', 'size'), pools=('pool', 'nunique'), mean=('ret', 'mean'), med=('ret', 'median'), win=('ret', lambda x: (x > 0).mean())).round(4).to_string())
s = R[R.pred_imp > R.rt]
print(s.groupby(['lead', 'lag', 'stake']).agg(n=('ret', 'size'), mean=('ret', 'mean'), med=('ret', 'median'), win=('ret', lambda x: (x > 0).mean())).round(4).to_string())
R.to_pickle('boostpath.pkl')
