"""Funding harvest walk-forward: each day pick the top-k coins by trailing funding, hold a
delta-neutral position (spot long + perp short) for the next 24 h, earn realized funding.
Costs per position change: spot round trip (entry+exit) and HL taker both legs."""
import json, sys
import numpy as np
import pandas as pd

F = json.load(open('funding30d.json'))
S = {c: pd.Series({pd.Timestamp(t, unit='ms').floor('h'): r for t, r, p in v}) for c, v in F.items() if len(v) > 300}
D = pd.DataFrame(S).sort_index()
print('coins', D.shape[1], 'hours', D.shape[0], D.index.min(), '->', D.index.max())
ann = D.mean() * 24 * 365
print('30d mean funding APR: median {:.1%}  p90 {:.1%}  max {:.1%} ({})'.format(ann.median(), ann.quantile(.9), ann.max(), ann.idxmax()))
top = ann.sort_values(ascending=False).head(15)
print('top coins by 30d mean APR:', {k: f'{v:.0%}' for k, v in top.items()})
days = pd.date_range(D.index.min().ceil('D') + pd.Timedelta(days=3), D.index.max().floor('D') - pd.Timedelta(days=1), freq='D')
for k in (1, 3, 5):
    for spot_rt in (0.004, 0.01):
        held, pnl_days = set(), []
        for d in days:
            trail = D.loc[d - pd.Timedelta(hours=72): d - pd.Timedelta(hours=1)].mean()
            pick = set(trail[trail > 0].sort_values(ascending=False).head(k).index)
            nxt = D.loc[d: d + pd.Timedelta(hours=23)]
            realized = nxt[list(pick)].sum() if pick else pd.Series(dtype=float)
            changes = len(pick - held) + len(held - pick)
            cost = changes * (spot_rt / 2 + 0.00045) / max(k, 1)   # half the round trip on each entry or exit, per unit of portfolio
            pnl_days.append(realized.mean() - cost if pick else -cost)
            held = pick
        p = np.array(pnl_days)
        print(f'top-{k}, spot round trip {spot_rt:.1%}: days {len(p)}  mean daily {p.mean():.3%}  APR {p.mean() * 365:.0%}  positive days {(p > 0).mean():.0%}  worst day {p.min():.3%}')
D.to_pickle('funding_hourly.pkl')
