"""Wallets that are net profitable (after fees, priority and tips) in every period."""
import pandas as pd
import numpy as np

P = ['pilot2', 'pilot', 'forward']
ws = {p: pd.read_csv(f'w_{p}.csv').set_index('addr') for p in P}
cols = ['coins', 'trades', 'net', 'spent', 'roi', 'win', 'med_age_s', 'b0', 'dev', 'med_hold_s', 'med_vsol', 'tip', 'txfee']
m = None
for p in P:
    x = ws[p][cols].add_suffix('_' + p[:2] + p[-1])
    m = x if m is None else m.join(x, how='inner')
print('wallets active in all 3 periods:', len(m))
sfx = ['_pi2', '_pit', '_fod']
sfx = ['_' + p[:2] + p[-1] for p in P]
for minc in (5, 10, 20):
    act = m[np.all([m['coins' + s] >= minc for s in sfx], axis=0)]
    win = act[np.all([act['net' + s] > 0 for s in sfx], axis=0)]
    nondev = win[np.all([win['dev' + s] < 0.2 for s in sfx], axis=0)]
    taker = nondev[np.all([nondev['b0' + s] < 0.2 for s in sfx], axis=0)]
    print(f'min coins {minc}: active {len(act)}  winners all 3: {len(win)}  non-dev: {len(nondev)}  non-dev non-create-slot: {len(taker)}')
act = m[np.all([m['coins' + s] >= 10 for s in sfx], axis=0)]
win = act[np.all([act['net' + s] > 0 for s in sfx], axis=0)].copy()
win['net_tot'] = sum(win['net' + s] for s in sfx)
win['spent_tot'] = sum(win['spent' + s] for s in sfx)
win['roi_tot'] = win['net_tot'] / win['spent_tot']
show = ['coins_pit', 'net_pi2', 'net_pit', 'net_fod', 'roi_tot', 'win_pit', 'med_age_s_pit', 'b0_pit', 'dev_pit', 'med_hold_s_pit', 'med_vsol_pit', 'tip_pit', 'txfee_pit']
pd.set_option('display.width', 250)
print(win.sort_values('net_tot', ascending=False)[show].head(60).round(3).to_string())
win.to_csv('consistent_winners.csv')
