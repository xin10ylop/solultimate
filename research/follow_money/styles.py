"""Wallet-day PnL (from `wallets`) by trading style, and day-over-day consistency."""
import sys
import pandas as pd, numpy as np
d = pd.read_csv(sys.argv[1])
d['date'] = pd.to_datetime(d.day * 86400, unit='s').dt.strftime('%m-%d')
d['style'] = np.select([d.dev > 0.5 * d.n, d.b0 > 0.5 * d.n, d.fast_entry > 0.5 * d.n, d.hold_s < 10, d.hold_s < 120],
                       ['dev', 'create-slot', '<=2 slots after create', 'flash <10s', 'short <2m'], 'long')
g = d.groupby(['style', 'date']).agg(wallets=('wallet', 'size'), pos=('n', 'sum'), spent=('spent', 'sum'), pnl=('pnl', 'sum'))
g['roi'] = g.pnl / g.spent
g['prof_wallets'] = d.assign(p=d.pnl > 0).groupby(['style', 'date']).p.mean()
pd.set_option('display.width', 220)
print(g.round(3).to_string())
