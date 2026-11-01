"""Cross-pool arbitrage on PumpSwap, measured on real pool states.

Cycles start and end in SOL:
  2-cycles  SOL -> A -> SOL through two different pools of the same pair
  3-cycles  SOL -> X -> A -> SOL (A/X pool plus a SOL pool for each of X and A)
State of a pool after event i = reserves reported by event i+1 (pre-trade), so states are exact
between a pool's first and last event and unknown after the last one (never evaluated there).
Each hop: constant product on effective reserves (base raw, quote = vault + virtual) with that
pool's taker fee (median non-zero fee of its trades). The optimal input is searched numerically.
An opportunity's lifetime = slots until any pool in the cycle changes again.
"""
import sys, os, itertools, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from pools import load_pools

d = sys.argv[1]
TXC = 0.0002   # SOL: base + priority for one atomic tx
MIN_PROFIT = 0.001
p = load_pools(d)
mints = open(os.path.join(d, 'mints.txt')).read().split()
WSOL = mints.index('So11111111111111111111111111111111111111112')
UNK = 2**32 - 1
ev = p[(p['kind'] >= 1) & (p['kind'] <= 3) & (p['mint'] != UNK) & (p['qmint'] != UNK)]
# pool table
P = pd.DataFrame({'pool': ev['pool'], 'base': ev['mint'], 'quote': ev['qmint'], 'fee': ev['fee_bps'], 'kind': ev['kind']})
fee = P[(P.kind <= 2) & (P.fee > 0)].groupby('pool')['fee'].median()
meta = P.drop_duplicates('pool').set_index('pool')[['base', 'quote']]
meta['fee'] = fee.reindex(meta.index).fillna(125) / 1e4
meta = meta[meta.base != meta.quote]
# adjacency: token -> list of (pool, other token)
adj = {}
for pl, r in meta.iterrows():
    adj.setdefault(r.base, []).append((pl, r.quote))
    adj.setdefault(r.quote, []).append((pl, r.base))
cycles = []
# 2-cycles: SOL-A pools, two of them
for A, lst in [(a, [pl for pl, o in adj.get(WSOL, []) if o == a]) for a in set(o for _, o in adj.get(WSOL, []))]:
    for p1, p2 in itertools.permutations(lst, 2):
        cycles.append(((p1, WSOL, A), (p2, A, WSOL)))
# 3-cycles: SOL -> X -> A -> SOL
sol_pools = {}
for pl, o in adj.get(WSOL, []):
    sol_pools.setdefault(o, []).append(pl)
for X, xpools in sol_pools.items():
    for pxa, A in adj.get(X, []):
        if A == WSOL or A == X or A not in sol_pools:
            continue
        for p1 in xpools:
            for p3 in sol_pools[A]:
                if len({p1, pxa, p3}) == 3:
                    cycles.append(((p1, WSOL, X), (pxa, X, A), (p3, A, WSOL)))
print(f'pools {len(meta)}  2-cycles {sum(len(c) == 2 for c in cycles)}  3-cycles {sum(len(c) == 3 for c in cycles)}')
rel = sorted(set(h[0] for c in cycles for h in c))
by_pool_cycles = {}
for ci, c in enumerate(cycles):
    for h in c:
        by_pool_cycles.setdefault(h[0], []).append(ci)
# per-pool event list with pre-states
E = pd.DataFrame({'pool': ev['pool'], 'slot': ev['slot'].astype(np.int64), 'tx': ev['tx'], 'evi': ev['ev'], 'kind': ev['kind'],
                  'B': ev['base_res'].astype(np.float64), 'Q': ev['quote_res'].astype(np.float64) + ev['vq'].astype(np.float64),
                  'base': ev['base'].astype(np.float64), 'quote': ev['quote'].astype(np.float64)})
E = E[E.pool.isin(set(rel))].sort_values(['slot', 'tx', 'evi']).reset_index(drop=True)
# pre-state of a boost burn from its after-state
bm = E.kind == 3
E.loc[bm, 'B'] = E.loc[bm, 'B'] + E.loc[bm, 'base']
E.loc[bm, 'Q'] = E.loc[bm, 'Q'] - E.loc[bm, 'quote']
E['nxt'] = E.groupby('pool').cumcount(ascending=False)  # events remaining after this one
grp = E.groupby('pool')
E['B_after'] = grp['B'].shift(-1)
E['Q_after'] = grp['Q'].shift(-1)
E['next_slot'] = grp['slot'].shift(-1)
print('relevant pool events', len(E))


def hop(x, pl, frm, to, st):
    B, Q = st[pl]
    base, quote, f = meta.at[pl, 'base'], meta.at[pl, 'quote'], meta.at[pl, 'fee']
    xn = x * (1 - f)
    if frm == base:   # sell base for quote
        return Q * xn / (B + xn)
    return B * xn / (Q + xn)  # buy base with quote


def cycle_out(x, c, st):
    for pl, frm, to in c:
        x = hop(x, pl, frm, to, st)
    return x


def best(c, st):
    # marginal check
    m = 1.0
    for pl, frm, to in c:
        B, Q = st[pl]
        f = meta.at[pl, 'fee']
        m *= (Q / B if frm == meta.at[pl, 'base'] else B / Q) * (1 - f)
    if m <= 1.0:
        return 0.0, 0.0, m
    lo, hi = 1e6, 50e9   # lamports 0.001 .. 50 SOL
    for _ in range(60):
        a = lo + (hi - lo) * 0.382
        b = lo + (hi - lo) * 0.618
        if cycle_out(a, c, st) - a < cycle_out(b, c, st) - b:
            lo = a
        else:
            hi = b
    x = (lo + hi) / 2
    return (cycle_out(x, c, st) - x) / 1e9, x / 1e9, m


state = {}
known = {}
next_change = {}
opps = []
gaps = []
cur_slot = None
changed = set()


def evaluate(slot):
    seen = set()
    for pl in changed:
        for ci in by_pool_cycles.get(pl, []):
            if ci in seen:
                continue
            seen.add(ci)
            c = cycles[ci]
            if not all(known.get(h[0], False) for h in c):
                continue
            prof, x, m = best(c, state)
            g = 1.0
            fsum = 0.0
            for pl, frm, to in c:
                B, Q = state[pl]
                g *= (Q / B if frm == meta.at[pl, 'base'] else B / Q)
                fsum += meta.at[pl, 'fee']
            gaps.append((len(c), g - 1, fsum))
            if prof - TXC >= MIN_PROFIT:
                life = min(next_change[h[0]] for h in c) - slot
                opps.append((slot, ci, len(c), prof - TXC, x, m, life))


for r in E.itertuples(index=False):
    if cur_slot is not None and r.slot != cur_slot:
        evaluate(cur_slot)
        changed.clear()
    cur_slot = r.slot
    if r.nxt == 0 or math.isnan(r.B_after):
        known[r.pool] = False
    else:
        state[r.pool] = (r.B_after, r.Q_after)
        known[r.pool] = r.B_after > 0 and r.Q_after > 0
        next_change[r.pool] = r.next_slot
    changed.add(r.pool)
evaluate(cur_slot)
O = pd.DataFrame(opps, columns=['slot', 'cycle', 'hops', 'profit', 'size', 'marg', 'life'])
hours = (E.slot.max() - E.slot.min()) * 0.211 / 3600
pd.set_option('display.width', 220)
print(f'span {hours:.2f} h; opportunities (profit >= {MIN_PROFIT} SOL after {TXC} SOL tx cost): {len(O)}')
if len(O):
    for lifemin in (1, 2, 3, 5):
        s = O[O.life >= lifemin]
        print(f'  still open >= {lifemin} slot(s): n {len(s):6d}  profit/h {s.profit.sum() / hours:8.3f} SOL  median profit {s.profit.median():.4f}  median size {s['size'].median():.2f} SOL  cycles {s.cycle.nunique()}')
    print(O.groupby('hops').agg(n=('profit', 'size'), profit=('profit', 'sum'), med=('profit', 'median')).round(4).to_string())
    top = O.groupby('cycle').agg(n=('profit', 'size'), profit=('profit', 'sum'), life=('life', 'median')).sort_values('profit', ascending=False).head(12)
    for ci, r in top.iterrows():
        path = ' -> '.join(['SOL'] + [mints[int(h[2])][:6] for h in cycles[int(ci)]])
        print(f'   {path:40s} n {int(r.n):5d} profit {r.profit:9.3f} SOL  median life {r.life:.0f} slots  fees {[round(meta.at[h[0], "fee"] * 1e4) for h in cycles[int(ci)]]}')
O.to_pickle('arb_opps.pkl')

G = pd.DataFrame(gaps, columns=['hops', 'gap', 'fees'])
print('evaluations', len(G), ' fee per cycle median', G.fees.median().round(4))
for h in (2, 3):
    g = G[G.hops == h]
    print(f'  {h}-hop: gross gap quantiles (before fees)', np.round(g.gap.quantile([.5, .9, .99, .999]).values, 4),
          ' share gap > fees:', round((g.gap > g.fees).mean(), 5))
fz = meta.fee.round(4).value_counts().head(8)
print('pool taker fee distribution (fraction):', fz.to_dict())
