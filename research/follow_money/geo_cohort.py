"""Does position / profit depend on where the slot leader is?
(1) Same-slot reaction: for big buys (>= 1 SOL), how many other trades follow in the same slot,
    by leader distance from Frankfurt (a proxy for how many bots can react inside the slot).
(2) The consistent cohort: share of their entries by leader region vs share of slots, and the
    return of their own fills by region."""
import sys, os, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

d = sys.argv[1]
W = sys.argv[2].split(',')
G = '/tmp/claude-0/-home-user-solultimate/b40a3b32-0fa6-5294-ba18-7475a0b95594/scratchpad/geo/'
leaders = json.load(open(G + 'slot_leaders.json'))
geo = json.load(open(G + 'leader_geo.json'))
base = int(open(os.path.join(d, 'base_slot.txt')).read())
def region(slot):
    l = leaders.get(str(base + int(slot)))
    g = geo.get(l, {}) if l else {}
    km = g.get('km_fra')
    if km is None:
        return 'unknown'
    return 'near (<500 km)' if km < 500 else 'mid (500-2000)' if km < 2000 else 'far (>2000)'
t, c, k = load(d)
t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
grp = np.zeros(len(wal), bool)
for i, a in enumerate(wal):
    if any(a.startswith(w) for w in W):
        grp[i] = True
slots = np.unique(t['slot'])
reg_of = {s: region(s) for s in slots}
R = np.array([reg_of[s] for s in t['slot']])
sol = t['sol'] / 1e9
buy = t['buy'].astype(bool)
# (1) same-slot followers after big buys
order = np.lexsort((t['tx'], t['slot']))
ts, tx, tm = t['slot'][order], t['tx'][order], t['mint'][order]
big = np.flatnonzero((buy[order]) & (sol[order] >= 1.0))
rows = []
for i in big:
    j = i + 1
    n = 0
    while j < len(order) and ts[j] == ts[i]:
        if tm[j] == tm[i] and tx[j] != tx[i]:
            n += 1
        j += 1
    rows.append((R[order][i], n))
F = pd.DataFrame(rows, columns=['region', 'followers'])
print(os.path.basename(d.rstrip('/')), 'big buys', len(F))
print(F.groupby('region').followers.agg(['size', 'mean', lambda x: (x > 0).mean()]).rename(columns={'<lambda_0>': 'P(any follower same slot)'}).round(3).to_string())
# (2) cohort entries by region
slot_share = pd.Series([reg_of[s] for s in slots]).value_counts(normalize=True)
coh = pd.Series(R[grp[t['user']] & buy]).value_counts(normalize=True)
allb = pd.Series(R[buy]).value_counts(normalize=True)
print(pd.DataFrame({'share of slots': slot_share, 'share of all buys': allb, 'share of cohort buys': coh}).round(3).to_string())
