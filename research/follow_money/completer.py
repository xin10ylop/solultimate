"""Who completes the curve, and do completers profit? Completer = user of the last curve trade
(a buy) before the Complete event. PnL over the next 10 min = SOL received from their PumpSwap
sells - SOL paid for curve buys. Tokens bought beyond the curve (synthetic migration part) are not
in the TradeEvent; we detect them as pool sells exceeding curve-bought tokens."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load
from pools import load_pools

SL = 0.211
d = sys.argv[1]
t, c, k = load(d)
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
wal = open(os.path.join(d, 'wallets.txt')).read().split()
WSOL = mints.index('So11111111111111111111111111111111111111112')
cr = p[(p['kind'] == 0)]
pool_of = {int(r['mint']): int(r['pool']) for r in cr}
open_slot = {int(r['mint']): int(r['slot']) for r in cr}
tr = p[((p['kind'] == 1) | (p['kind'] == 2)) & (p['qmint'] == WSOL)]
TP = pd.DataFrame({'pool': tr['pool'], 'slot': tr['slot'].astype(np.int64), 'user': tr['user'], 'buy': tr['kind'] == 1,
                   'quote': tr['quote'] / 1e9, 'base': tr['base'].astype(np.float64)})
byp = {k_: g for k_, g in TP.groupby('pool')}
rows = []
for m, cs in zip(k['mint'], k['slot']):
    m = int(m)
    tm = t[(t['mint'] == m)]
    if len(tm) == 0 or m not in pool_of:
        continue
    last = tm[-1]
    if not last['buy']:
        continue
    u = int(last['user'])
    ub = tm[(tm['user'] == u) & (tm['buy'] == 1)]
    us = tm[(tm['user'] == u) & (tm['buy'] == 0)]
    cost = (ub['sol'].sum() + ub['fee'].sum()) / 1e9
    curve_tok = ub['tok'].sum() - us['tok'].sum()
    curve_back = (us['sol'].sum() - us['fee'].sum()) / 1e9
    g = byp.get(pool_of[m])
    if g is None:
        continue
    w = g[(g.user == u) & (g.slot <= open_slot[m] + int(600 / SL))]
    sold = w[~w.buy]
    bought = w[w.buy]
    proceeds = sold.quote.sum()
    pool_buy = bought.quote.sum()
    rows.append(dict(mint=mints[m][:8], completer=wal[u][:8], n_curve_buys=len(ub), last_buy_sol=last['sol'] / 1e9,
                     cost=cost, curve_back=curve_back, pool_proceeds=proceeds, pool_buys=pool_buy,
                     tok_curve=curve_tok / 1e6, tok_sold_pool=sold.base.sum() / 1e6,
                     pnl=curve_back + proceeds - cost - pool_buy, open_delay=open_slot[m] - int(cs)))
R = pd.DataFrame(rows)
pd.set_option('display.width', 220)
print('graduations with completer data', len(R))
R['synthetic'] = R.tok_sold_pool > R.tok_curve * 1.02
print('completers who bought in one shot (1 curve buy):', (R.n_curve_buys == 1).mean().round(3), ' with synthetic part (sold more than curve tokens):', R.synthetic.mean().round(3))
print('completer PnL over 10 min (SOL; synthetic-part cost missing, so those are overstated):')
for name, x in (('all', R), ('no synthetic part', R[~R.synthetic])):
    print(f'  {name:18s} n {len(x):4d} mean {x.pnl.mean():+.3f}  median {x.pnl.median():+.3f}  win {(x.pnl > 0).mean():.2f}  sum {x.pnl.sum():+.1f}')
print('repeat completers:', R.completer.value_counts().head(8).to_dict())
print(R.sort_values('pnl').head(5)[['mint', 'completer', 'n_curve_buys', 'last_buy_sol', 'cost', 'pool_proceeds', 'pnl']].round(3).to_string())
print(R.sort_values('pnl').tail(5)[['mint', 'completer', 'n_curve_buys', 'last_buy_sol', 'cost', 'pool_proceeds', 'pnl']].round(3).to_string())
