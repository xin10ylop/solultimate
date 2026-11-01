"""Boost buy-and-burn crank on PumpSwap: cadence, spend rule, impact vs fees, front-running signature."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from pools import load_pools, KIND

d = sys.argv[1]
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
wal = open(os.path.join(d, 'wallets.txt')).read().split()
print(f'pool events {len(p)}: ' + ', '.join(f'{KIND[k]} {(p["kind"] == k).sum()}' for k in range(5)))
bb = p[p['kind'] == 3]
tr = p[(p['kind'] == 1) | (p['kind'] == 2)]
print('boost authorities:', {wal[u][:10]: int(n) for u, n in zip(*np.unique(bb['user'], return_counts=True))})
B = pd.DataFrame({'pool': bb['pool'], 'mint': bb['mint'], 'slot': bb['slot'].astype(np.int64), 'tx': bb['tx'], 'used': bb['quote'] / 1e9,
                  'rem': bb['extra'] / 1e9, 'rq': bb['quote_res'] / 1e9, 'vq': bb['vq'] / 1e9, 'base_after': bb['base_res'].astype(np.float64)})
B['frac'] = B.used / (B.used + B.rem)
B['Q'] = B.rq + B.vq  # effective quote reserves after the burn
B['impact'] = (B.Q / (B.Q - B.used)) ** 2 - 1  # price ratio after/before for a buy of `used` (base removed)
B = B.sort_values(['pool', 'slot'])
B['gap'] = B.groupby('pool')['slot'].diff()
# fee tier per pool from trades
T = pd.DataFrame({'pool': tr['pool'], 'slot': tr['slot'].astype(np.int64), 'tx': tr['tx'], 'buy': tr['kind'] == 1, 'quote': tr['quote'] / 1e9,
                  'fee_bps': tr['fee_bps'], 'user': tr['user'], 'base_res': tr['base_res'].astype(np.float64), 'qres': tr['quote_res'] / 1e9,
                  'vq': tr['vq'] / 1e9})
fee = T.groupby('pool')['fee_bps'].median()
g = B.groupby('pool').agg(n=('used', 'size'), spent=('used', 'sum'), med_gap=('gap', 'median'), gap_iqr=('gap', lambda x: x.quantile(.75) - x.quantile(.25)),
                          frac=('frac', 'median'), impact=('impact', 'median'), Q=('Q', 'median'), first=('slot', 'min'), last=('slot', 'max'))
g['fee_bps'] = fee.reindex(g.index)
g['rt_cost'] = 2 * g.fee_bps / 1e4
g['edge_per_cycle'] = g.impact - g.rt_cost
g['mint'] = [mints[B[B.pool == i].mint.iloc[0]][:12] if B[B.pool == i].mint.iloc[0] < len(mints) else '?' for i in g.index]
g['hours'] = (g['last'] - g['first']) * 0.211 / 3600
pd.set_option('display.width', 250)
print(f'boosted pools with burns: {len(g)}   total SOL spent by the crank: {B.used.sum():.1f}')
print('cadence (slots between burns on the same pool): median', B.gap.median(), ' p10/p90', B.gap.quantile(.1), B.gap.quantile(.9))
print(g.sort_values('spent', ascending=False).head(30).round(4).to_string())
print('pools where median impact per burn > round-trip fee:', (g.edge_per_cycle > 0).sum(), ' spent there', g[g.edge_per_cycle > 0].spent.sum().round(1))
B.to_pickle('boost_events.pkl')
T.to_pickle('pool_trades.pkl')
