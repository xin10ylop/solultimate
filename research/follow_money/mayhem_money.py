"""Follow the money inside mayhem coins: who is the agent, what it does, who wins against it."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load

d = sys.argv[1]
name = os.path.basename(d.rstrip('/'))
t, c, k = load(d)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
mc = c[(c['flags'] & 1 != 0) & (c['flags'] & 2 != 0)]
isM = np.zeros(max(t['mint'].max(), c['mint'].max()) + 1, bool)
isM[mc['mint']] = True
t = t[isM[t['mint']] & (t['tok'] > 0)]
print(f'{name}: mayhem launches {len(mc)}  trades {len(t)}  coins traded {len(np.unique(t["mint"]))}')
buy = t['buy'].astype(bool)
sol = t['sol'] / 1e9
fee = t['fee'] / 1e9
df = pd.DataFrame({'mint': t['mint'], 'user': t['user'], 'signer': t['signer'], 'slot': t['slot'], 'tx': t['tx'], 'buy': buy,
                   'sol': sol, 'fee': fee, 'tok': t['tok'].astype(np.float64), 'vsol': t['vsol'] / 1e9, 'vtok': t['vtok'].astype(np.float64)})
df['cash'] = np.where(buy, -(df.sol + df.fee), df.sol - df.fee)
df['dtok'] = np.where(buy, df.tok, -df.tok)
df['spot'] = df.vsol / df.vtok
# the agent: wallet present in the most mayhem coins
cov = df.groupby('user')['mint'].nunique().sort_values(ascending=False)
print('wallets by mayhem coins touched:')
for u, n in cov.head(8).items():
    g = df[df.user == u]
    print(f'   {wal[u]}  coins {n}  trades {len(g)}  buys {g.buy.mean():.2f}  vol {g.sol.sum():.1f} SOL  cash {g.cash.sum():+.1f}')
agent = set(cov.index[cov >= 0.5 * len(mc)].tolist())
print('agent wallets (>=50% coverage):', [wal[a][:8] for a in agent])
lastspot = df.groupby('mint')['spot'].last()
pm = df.groupby(['user', 'mint']).agg(cash=('cash', 'sum'), tok=('dtok', 'sum'), spent=('cash', lambda x: -x[x < 0].sum()),
                                       first=('slot', 'first'), last=('slot', 'last')).reset_index()
pm['tok'] = pm.tok.clip(lower=0)
pm['mark'] = pm.tok * lastspot.reindex(pm.mint).values * 0.5
pm['pnl'] = pm.cash + pm.mark
pm['hold_s'] = (pm['last'] - pm['first']) * 0.211
pm['agent'] = pm.user.isin(agent)
print('agent total pnl (cash+0.5*mark):', round(pm[pm.agent].pnl.sum(), 1), 'cash only', round(pm[pm.agent].cash.sum(), 1))
w = pm[~pm.agent].groupby('user').agg(coins=('mint', 'size'), pnl=('pnl', 'sum'), cash=('cash', 'sum'), spent=('spent', 'sum'),
                                       hold=('hold_s', 'median'), win=('pnl', lambda x: (x > 0).mean()))
w['addr'] = [wal[i] for i in w.index]
w.to_csv(f'mw_{name}.csv')
print('non-agent traders: n', len(w), 'total pnl', round(w.pnl.sum(), 1), 'spent', round(w.spent.sum(), 1), 'roi', round(w.pnl.sum() / w.spent.sum(), 4))
for lo, hi in ((0, 10), (10, 120), (120, 1e9)):
    s = pm[(~pm.agent) & (pm.hold_s >= lo) & (pm.hold_s < hi)]
    print(f'   positions hold {lo}-{hi}s: n {len(s)} roi {s.pnl.sum() / max(s.spent.sum(), 1e-9):+.4f} win {(s.pnl > 0).mean():.3f}')
big = w[w.coins >= 20].sort_values('pnl', ascending=False)
print(big.head(15).round(3).to_string())
print('--- positions with real buys (spent>0) and fully closed (tok<=1% of bought)')
bt = df[df.buy].groupby(['user', 'mint'])['tok'].sum().rename('bought')
pm = pm.join(bt, on=['user', 'mint'])
cl = pm[(~pm.agent) & (pm.spent > 0) & (pm.tok <= 0.01 * pm.bought.fillna(0))]
for lo, hi in ((0, 1), (1, 3), (3, 10), (10, 30), (30, 120), (120, 1e9)):
    s = cl[(cl.hold_s >= lo) & (cl.hold_s < hi)]
    print(f'   closed hold {lo}-{hi}s: n {len(s):6d} wallets {s.user.nunique():5d} roi {s.pnl.sum() / max(s.spent.sum(), 1e-9):+.4f} win {(s.pnl > 0).mean():.3f} spent {s.spent.sum():.0f}')
ws = cl[cl.hold_s < 10].groupby('user').agg(n=('pnl', 'size'), pnl=('pnl', 'sum'), spent=('spent', 'sum'), win=('pnl', lambda x: (x > 0).mean()))
ws['roi'] = ws.pnl / ws.spent
ws['addr'] = [wal[i][:10] for i in ws.index]
print('top short-hold wallets:')
print(ws.sort_values('pnl', ascending=False).head(12).round(3).to_string())
print('short-hold wallets with >=50 positions: ', (ws.n >= 50).sum(), ' profitable', ((ws.n >= 50) & (ws.pnl > 0)).sum())
# conservative wallet-level: all mayhem positions, unsold tokens worth 0
wa = pm[(~pm.agent) & (pm.spent > 0)].groupby('user').agg(pos=('pnl', 'size'), cash=('cash', 'sum'), spent=('spent', 'sum'),
                                                          short=('hold_s', lambda x: (x < 10).mean()))
wa['addr'] = [wal[i] for i in wa.index]
wa.to_csv(f'mwc_{name}.csv')
