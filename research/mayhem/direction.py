"""Can the mayhem agent's next direction (buy = price jump up) be predicted from what is visible
before it acts? Uses the agent's own events per coin, in order."""
import json, sys, struct, collections
import numpy as np
import pandas as pd

E = [json.loads(l) for l in open(sys.argv[1])]
Tm = {}
for e in E:
    if e['e'] == 'T':
        Tm.setdefault((e['slot'], e['tx']), []).append(e)
rows = []
for e in E:
    if e['e'] != 'M':
        continue
    p = bytes.fromhex(e['raw'])
    if len(p) != 295:
        continue
    t = [x for x in Tm.get((e['slot'], e['tx']), []) if x['mint'] == e['mint']]
    if not t:
        continue
    t = t[0]
    i128 = lambda o: int.from_bytes(p[o:o + 16], 'little', signed=True)
    r = dict(mint=e['mint'], slot=e['slot'], tx=e['tx'], dir=p[0], buy=t['buy'], sol=t['sol'] / 1e9, vs=t['vs'] / 1e9, vt=t['vt'],
             rs=t['rs'] / 1e9, netsol=i128(97) / 1e9, nettok=i128(113) / 1e6, now=struct.unpack_from('<q', p, 161)[0], end=struct.unpack_from('<q', p, 169)[0],
             f81=struct.unpack_from('<Q', p, 81)[0], f89=struct.unpack_from('<Q', p, 89)[0], f145=struct.unpack_from('<Q', p, 145)[0], raw=p)
    rows.append(r)
D = pd.DataFrame(rows).sort_values(['mint', 'slot', 'tx']).reset_index(drop=True)
print('agent events', len(D), 'mints', D.mint.nunique(), ' dir==buy flag agreement:', round(((D.dir == 0) == (D.buy == 1)).mean(), 4))
D['prev_buy'] = D.groupby('mint').buy.shift(1)
D['prev2_buy'] = D.groupby('mint').buy.shift(2)
D['gap'] = D.groupby('mint').slot.diff()
d = D.dropna(subset=['prev_buy'])
print('P(buy) overall', round(D.buy.mean(), 4))
print('P(buy | prev buy)', round(d[d.prev_buy == 1].buy.mean(), 4), ' P(buy | prev sell)', round(d[d.prev_buy == 0].buy.mean(), 4))
dd = d.dropna(subset=['prev2_buy'])
print(dd.groupby(['prev2_buy', 'prev_buy']).buy.agg(['size', 'mean']).round(4).to_string())
# inventory dependence: agent's net SOL / net tokens before acting (= previous event's values)
D['p_netsol'] = D.groupby('mint').netsol.shift(1)
D['p_nettok'] = D.groupby('mint').nettok.shift(1)
x = D.dropna(subset=['p_netsol'])
x['q'] = pd.qcut(x.p_netsol, 10, duplicates='drop')
print('P(buy) by agent net SOL before acting (deciles):')
print(x.groupby('q', observed=True).buy.agg(['size', 'mean']).round(3).to_string())
# time-to-end and age
x['left_h'] = (x.end - x.now) / 3600
print('P(buy) by hours left in mayhem window:', x.groupby(pd.cut(x.left_h, [0, 1, 6, 12, 18, 23, 24.1]), observed=True).buy.agg(['size', 'mean']).round(3).to_dict())
# price relative to the coin's recent mean (mean reversion of the agent?)
D['lp'] = np.log(D.vs / D.vt)
D['lp_dev'] = D.lp - D.groupby('mint').lp.transform(lambda s: s.rolling(20, min_periods=5).mean().shift(1))
y = D.dropna(subset=['lp_dev'])
print('P(buy) by price deviation from its 20-event mean (deciles):')
print(y.groupby(pd.qcut(y.lp_dev, 10, duplicates='drop'), observed=True).buy.agg(['size', 'mean']).round(3).to_string())
# raw-byte scan: which single byte of the PREVIOUS event best predicts the next direction?
prev_raw = D.groupby('mint').raw.shift(1)
ok = prev_raw.notna()
yb = D.buy[ok].values
best = []
for o in range(295):
    v = np.array([r[o] for r in prev_raw[ok]])
    for bit in range(8):
        b = (v >> bit) & 1
        if b.std() == 0:
            continue
        acc = max((b == yb).mean(), (b != yb).mean())
        best.append((acc, o, bit))
best.sort(reverse=True)
print('best single bit of previous event predicting next direction:', best[:5])
own = []
for o in range(295):
    v = np.array([r[o] for r in D.raw])
    for bit in range(8):
        b = (v >> bit) & 1
        if b.std() == 0 or o == 0:
            continue
        own.append((max((b == D.buy.values).mean(), (b != D.buy.values).mean()), o, bit))
own.sort(reverse=True)
print('best single bit of the SAME event matching its direction (excluding byte 0):', own[:5])
D.drop(columns=['raw']).to_pickle('agent_events.pkl')
