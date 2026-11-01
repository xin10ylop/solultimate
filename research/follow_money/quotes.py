"""Inventory of non-SOL quotes: which quote tokens coins launch against, and whether the same base
coin trades in more than one PumpSwap pool (different quotes) -> triangular arbitrage candidates."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load
from pools import load_pools

d = sys.argv[1]
t, c, k = load(d)
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
WSOL = 'So11111111111111111111111111111111111111112'
USDC = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
name = lambda i: 'SOL' if mints[i] == WSOL else ('USDC' if mints[i] == USDC else mints[i][:8] + ('…pump' if mints[i].endswith('pump') else ''))
# 1) curve launches by quote
cq = pd.Series([name(q) for q in c['qmint']]).value_counts()
print(f'curve launches {len(c)}: SOL {cq.get("SOL", 0)}, non-SOL {len(c) - cq.get("SOL", 0)}, distinct quote tokens {len(cq)}')
print('top quote tokens for curve launches:', cq.head(12).to_dict())
# curve trades volume by quote (in quote units, so count only)
qm_of = dict(zip(c['mint'].tolist(), c['qmint'].tolist()))
tq = pd.Series([name(qm_of[m]) if m in qm_of else '?' for m in t['mint']]).value_counts()
print('curve trades by quote:', tq.head(8).to_dict())
# 2) PumpSwap pools by quote
tr = p[(p['kind'] == 1) | (p['kind'] == 2)]
pq = pd.DataFrame({'pool': tr['pool'], 'mint': tr['mint'], 'q': tr['qmint']}).drop_duplicates('pool')
pq = pq[(pq.mint != 2**32 - 1) & (pq.q != 2**32 - 1)]
pq['qn'] = [name(q) for q in pq.q]
print(f'PumpSwap pools traded: {len(pq)}  by quote:', pq.qn.value_counts().head(10).to_dict())
# 3) same base coin in pools with different quotes
multi = pq.groupby('mint').filter(lambda g: g.q.nunique() >= 2)
print(f'base coins with pools in >= 2 different quotes on PumpSwap: {multi.mint.nunique()}')
for m, g in list(multi.groupby('mint'))[:15]:
    print('   ', mints[m][:12], sorted(g.qn.tolist()))
# 4) quote tokens of non-SOL pools: do they themselves have a SOL pool on PumpSwap?
nonsol_q = set(pq[pq.qn != 'SOL'].q.tolist())
has_sol_pool = set(pq[pq.qn == 'SOL'].mint.tolist())
print(f'non-SOL quote tokens used by PumpSwap pools: {len(nonsol_q)}; of them with a SOL pool on PumpSwap: {len(nonsol_q & has_sol_pool)}')
qcounts = pq[pq.qn != 'SOL'].groupby('qn').size().sort_values(ascending=False)
print('pools per non-SOL quote:', qcounts.head(10).to_dict())
