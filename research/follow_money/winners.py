"""Follow the money: per-wallet realized PnL (fees, priority and tips included) per period,
and the behaviour profile of the wallets that win.

Position accounting per (wallet, coin): buys cost sol+fee, sells return sol-fee. Tokens still
held at the end of the period are marked at the coin's last spot price minus 30% haircut
(they are usually illiquid); coins that completed are marked at the final curve price.
Tx costs (base+priority fee and tips) are charged once per transaction to the signer.
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
name = os.path.basename(d.rstrip('/'))
t, c, k = load(d)
t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
n = len(t)
buy = t['buy'].astype(bool)
sol = t['sol'].astype(np.float64) / 1e9
fee = t['fee'].astype(np.float64) / 1e9
tok = t['tok'].astype(np.float64)
cash = np.where(buy, -(sol + fee), sol - fee)
dtok = np.where(buy, tok, -tok)

# creation info per mint
cslot = np.full(t['mint'].max() + 1, -1, np.int64)
ctx = np.zeros(t['mint'].max() + 1, np.int64)
dev = np.full(t['mint'].max() + 1, -1, np.int64)
cslot[c['mint']] = c['slot']
ctx[c['mint']] = c['tx']
dev[c['mint']] = c['dev']
age = t['slot'].astype(np.int64) - cslot[t['mint']]

df = pd.DataFrame({'mint': t['mint'], 'user': t['user'], 'signer': t['signer'], 'slot': t['slot'], 'tx': t['tx'],
                   'buy': buy, 'cash': cash, 'dtok': dtok, 'age': age, 'vsol': t['vsol'] / 1e9,
                   'cu': t['cu'], 'tip': t['tip'] / 1e9, 'txfee': t['txfee'] / 1e9, 'shape': t['shape']})
df['spentc'] = np.where(buy, -cash, 0.0)
df['is_dev'] = df['user'].values == dev[df['mint'].values]
df['b0'] = df['age'] == 0
# last spot per mint
last = df.groupby('mint').tail(1)
spot = (t['vsol'] / t['vtok'].astype(np.float64))  # lamports per token unit
df['spot'] = spot / 1e9
lastspot = df.groupby('mint')['spot'].last()
completed = set(k['mint'].tolist())

# per (user, mint)
g = df.groupby(['user', 'mint'])
pm = g.agg(cash=('cash', 'sum'), tok=('dtok', 'sum'), nb=('buy', 'sum'), n=('buy', 'size'),
           first_age=('age', 'first'), first_slot=('slot', 'first'), last_slot=('slot', 'last'),
           first_vsol=('vsol', 'first'), dev=('is_dev', 'max'), b0=('b0', 'max'),
           spent=('spentc', 'sum')).reset_index()
pm['tok'] = pm['tok'].clip(lower=0)
net_sol = pd.Series(np.where(buy, sol, -sol), index=df.index).groupby(df['mint']).sum().clip(lower=0)
outst = pm.groupby('mint')['tok'].sum().clip(lower=1)   # per-wallet holdings after clip (transfers!)
liq = np.minimum(net_sol.reindex(outst.index) / outst, lastspot.reindex(outst.index))
pm['mark'] = pm['tok'] * liq.reindex(pm['mint']).values
pm['pnl'] = pm['cash'] + pm['mark']
pm['hold_s'] = (pm['last_slot'] - pm['first_slot']) * 0.211
pm['pos'] = pm['pnl'] > 0

# tx costs per signer, once per tx
tx = df.drop_duplicates(['slot', 'tx'])[['signer', 'txfee', 'tip']]
txc = tx.groupby('signer').agg(txfee=('txfee', 'sum'), tip=('tip', 'sum'), ntx=('txfee', 'size'))

w = pm.groupby('user').agg(coins=('mint', 'size'), pnl=('pnl', 'sum'), spent=('spent', 'sum'),
                           win=('pos', 'mean'), med_age_s=('first_age', 'median'),
                           b0=('b0', 'mean'), dev=('dev', 'mean'), med_hold_s=('hold_s', 'median'),
                           med_vsol=('first_vsol', 'median'), trades=('n', 'sum'))
w = w.join(txc, how='left').fillna({'txfee': 0, 'tip': 0, 'ntx': 0})
w['med_age_s'] *= 0.211
w['net'] = w['pnl'] - w['txfee'] - w['tip']
w['roi'] = w['net'] / w['spent'].clip(lower=1e-9)
w['addr'] = [wal[i] for i in w.index]
w.to_csv(f'w_{name}.csv')
print(f'{name}: wallets {len(w)}  total net {w.net.sum():.1f} SOL  spent {w.spent.sum():.0f} SOL')
# who wins in aggregate, by archetype
w['netpos'] = w['net'] > 0
w['arch'] = np.select(
    [w.dev > 0.5, w.b0 > 0.5, w.med_hold_s < 10, w.med_hold_s < 120],
    ['dev', 'create-slot', 'flash<10s', 'short<2m'], default='long')
a = w.groupby('arch').agg(wallets=('net', 'size'), net=('net', 'sum'), spent=('spent', 'sum'),
                          pos=('netpos', 'mean'))
a['roi'] = a['net'] / a['spent']
print(a.round(3).to_string())
big = w[(w.coins >= 20)]
print(f'wallets with >=20 coins: {len(big)}, net {big.net.sum():.1f}, profitable {(big.net>0).mean():.2%}')
print(big.sort_values('net', ascending=False).head(25)[['addr', 'coins', 'trades', 'net', 'roi', 'win', 'med_age_s', 'b0', 'dev', 'med_hold_s', 'med_vsol', 'tip', 'txfee']].round(3).to_string())
