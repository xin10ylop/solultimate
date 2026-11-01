"""Follow the money per quote-token segment (SOL, PUMP, USDC, pump coins, other tokens).
ROI = PnL / spent, computed in each coin's own quote units, pooled within a segment by converting
each coin's flows to 'units of initial virtual quote' (so segments with different quote tokens
are comparable). Unsold tokens marked at min(pro-rata real quote, spot)."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load

d = sys.argv[1]
t, c, k = load(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
SYS = '11111111111111111111111111111111'
WSOL = 'So11111111111111111111111111111111111111112'
USDC = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
PUMPT = 'pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn'
def seg(q):
    a = mints[q]
    if a in (SYS, WSOL):
        return 'SOL'
    if a == USDC:
        return 'USDC'
    if a == PUMPT:
        return 'PUMP'
    return 'pump coin' if a.endswith('pump') else 'other token'
c = c[(c['flags'] & 1) == 0]  # no mayhem
cseg = {int(m): seg(int(q)) for m, q in zip(c['mint'], c['qmint'])}
cdev = {int(m): int(dv) for m, dv in zip(c['mint'], c['dev'])}
cslot = {int(m): int(s) for m, s in zip(c['mint'], c['slot'])}
m = np.isin(t['mint'], list(cseg.keys())) & (t['tok'] > 0) & (t['vtok'] > 0)
t = t[m]
df = pd.DataFrame({'mint': t['mint'].astype(np.int64), 'user': t['user'], 'slot': t['slot'].astype(np.int64), 'buy': t['buy'].astype(bool),
                   'q': t['sol'].astype(np.float64), 'fee': t['fee'].astype(np.float64), 'tok': t['tok'].astype(np.float64),
                   'vs': t['vsol'].astype(np.float64), 'vt': t['vtok'].astype(np.float64)})
# normalise each coin's quote units by its first-trade virtual quote reserve level (initial vq ~ 30 SOL-equivalent)
first_vs = df.groupby('mint').vs.transform('first')
scale = first_vs / 30.0
df['q'] /= scale
df['fee'] /= scale
df['vs'] /= scale
df['cash'] = np.where(df.buy, -(df.q + df.fee), df.q - df.fee)
df['spent'] = np.where(df.buy, df.q + df.fee, 0.0)
df['dtok'] = np.where(df.buy, df.tok, -df.tok)
df['seg'] = df.mint.map(cseg)
df['is_dev'] = df.user.values == df.mint.map(cdev).values
df['b0'] = df.slot.values == df.mint.map(cslot).values
pm = df.groupby(['seg', 'mint', 'user']).agg(cash=('cash', 'sum'), spent=('spent', 'sum'), tok=('dtok', 'sum'), first=('slot', 'first'),
                                             last=('slot', 'last'), dev=('is_dev', 'max'), b0=('b0', 'max')).reset_index()
pm['tok'] = pm.tok.clip(lower=0)
lastspot = df.groupby('mint').apply(lambda g: g.vs.iloc[-1] / g.vt.iloc[-1], include_groups=False)
net_q = df.groupby('mint').apply(lambda g: np.where(g.buy, g.q, -g.q).sum(), include_groups=False).clip(lower=0)
outst = pm.groupby('mint').tok.sum().clip(lower=1)
liq = np.minimum(net_q.reindex(outst.index) / outst, lastspot.reindex(outst.index))
pm['pnl'] = pm.cash + pm.tok * liq.reindex(pm.mint).values
pm['hold'] = (pm['last'] - pm['first']) * 0.211
pm['arch'] = np.select([pm.dev, pm.b0, pm.hold < 10, pm.hold < 120], ['dev', 'create-slot', 'flash<10s', 'short<2m'], default='long')
pm = pm[pm.spent > 0]
pd.set_option('display.width', 220)
print(os.path.basename(d.rstrip('/')), ' coins per segment:', pd.Series(list(cseg.values())).value_counts().to_dict())
tab = pm.groupby(['seg', 'arch']).agg(positions=('pnl', 'size'), pnl=('pnl', 'sum'), spent=('spent', 'sum'), win=('pnl', lambda x: (x > 0).mean()))
tab['roi'] = tab.pnl / tab.spent
print(tab[['positions', 'roi', 'win']].round(3).unstack('arch').to_string())
tk = pm[pm.arch.isin(['flash<10s', 'short<2m', 'long'])].groupby('seg').agg(positions=('pnl', 'size'), pnl=('pnl', 'sum'), spent=('spent', 'sum'))
tk['taker_roi'] = tk.pnl / tk.spent
print('all takers by segment:'); print(tk.round(3).to_string())
pm.to_pickle(f'seg_{os.path.basename(d.rstrip("/"))}.pkl')
