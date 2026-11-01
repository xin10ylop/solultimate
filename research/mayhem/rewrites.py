"""Where do mayhem virtual-SOL rewrites happen, and can they be predicted?"""
import json, sys, struct, collections
import numpy as np
import pandas as pd

E = [json.loads(l) for l in open(sys.argv[1])]
mm = set(e['mint'] for e in E if e['e'] == 'M')
upd = [e for e in E if e['e'] == 'U']
print('events', len(E), 'mayhem mints', len(mm), 'UpdateMayhemVirtualParams events', len(upd))
T = [e for e in E if e['e'] == 'T' and e['mint'] in mm]
M = {(e['slot'], e['tx']): e for e in E if e['e'] == 'M'}
by = collections.defaultdict(list)
for e in T:
    by[e['mint']].append(e)
rows = []
for mint, s in by.items():
    s.sort(key=lambda e: (e['slot'], e['tx'], e['k']))
    for a, b in zip(s[:-1], s[1:]):
        pre_vs = b['vs'] - b['sol'] if b['buy'] else b['vs'] + b['sol']
        pre_vt = b['vt'] + b['tok'] if b['buy'] else b['vt'] - b['tok']
        if pre_vt <= 0 or a['vt'] <= 0 or a['vs'] <= 0:
            continue
        agent_b = (b['slot'], b['tx']) in M
        rows.append(dict(mint=mint, slot=b['slot'], agent=agent_b, same_tx=(a['slot'], a['tx']) == (b['slot'], b['tx']),
                         dvs=(pre_vs - a['vs']) / 1e9, dvt=pre_vt - a['vt'], vs_prev=a['vs'] / 1e9, rs_prev=a['rs'] / 1e9, rs_pre=(b['rs'] - b['sol'] if b['buy'] else b['rs'] + b['sol']) / 1e9,
                         jump=(pre_vs / pre_vt) / (a['vs'] / a['vt']) - 1, buy=b['buy'], gap=b['slot'] - a['slot']))
R = pd.DataFrame(rows)
R['rw'] = (R.dvs.abs() > 1e-9) | (R.dvt != 0)
print(f'consecutive trade pairs on mayhem coins {len(R)}; with a rewrite in between {R.rw.sum()} ({R.rw.mean():.1%})')
print('rewrites by who trades next: agent tx', int(R[R.rw & R.agent].shape[0]), ' non-agent tx', int(R[R.rw & ~R.agent].shape[0]))
print('non-agent next trade: rewrite share', round(R[~R.agent].rw.mean(), 4), ' agent next trade: rewrite share', round(R[R.agent].rw.mean(), 4))
print('virtual tokens changed in rewrites:', int((R.rw & (R.dvt != 0)).sum()), ' real SOL changed between trades:', int((R.rs_pre - R.rs_prev).abs().gt(1e-9).sum()))
rw = R[R.rw]
print('jump quantiles (agent rewrites):', np.round(rw[rw.agent].jump.quantile([.05, .25, .5, .75, .95]).values, 4))
# is the rewrite direction tied to the agent's trade direction in the same tx?
print('agent rewrites: P(jump>0 | agent buys)', round((rw[rw.agent & (rw.buy == 1)].jump > 0).mean(), 3), ' P(jump>0 | agent sells)', round((rw[rw.agent & (rw.buy == 0)].jump > 0).mean(), 3))
# autocorrelation of successive rewrite jumps per mint
ac = []
for mint, g in rw.groupby('mint'):
    j = g.jump.values
    if len(j) > 10:
        ac.append(np.corrcoef(j[:-1], j[1:])[0, 1])
print('per-mint lag-1 autocorr of jumps: median', round(float(np.nanmedian(ac)), 3), ' n mints', len(ac))
# price level mean reversion: does log price after rewrite revert toward a level?
R.to_pickle('rewrites.pkl')
