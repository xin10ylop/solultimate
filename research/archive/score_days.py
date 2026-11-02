"""Per-UTC-day scoring of a continuous replay (pre-registered archive test, research/frozen_candidates.md).

usage: score_days.py trades.csv slot_times.csv [first_scored_slot] [last_scored_slot] [variant]
Racing fees = the replay's own costs (--live-costs). Normal fees = replace fixed costs by 0.0026 SOL.
"""
import sys
import numpy as np
import pandas as pd

t = pd.read_csv(sys.argv[1])
anc = pd.read_csv(sys.argv[2], header=None, names=['slot', 'ts']).sort_values('slot')
lo = int(sys.argv[3]) if len(sys.argv) > 3 else 0
hi = int(sys.argv[4]) if len(sys.argv) > 4 else 10**12
var = sys.argv[5] if len(sys.argv) > 5 else 'lse_bundle'
t = t[(t.variant == var) & (t.decision_slot >= lo) & (t.decision_slot < hi)].copy()
t['ts'] = np.interp(t.decision_slot, anc.slot, anc.ts)
t['day'] = pd.to_datetime(t.ts, unit='s', utc=True).dt.strftime('%Y-%m-%d')
t['rr'] = t.ret                                              # racing (live costs)
t['rn'] = (t.pnl_sol + t.fixed_costs_sol - 0.0026) / t.stake_sol  # normal fees
t['pn'] = t.pnl_sol + t.fixed_costs_sol - 0.0026
# complete days only: both day boundaries inside [lo, hi)
t0 = np.interp(lo, anc.slot, anc.ts) if lo > 0 else anc.ts.min()
t1 = np.interp(min(hi, anc.slot.max()), anc.slot, anc.ts)
d0 = pd.Timestamp(t0, unit='s', tz='UTC').ceil('D')
d1 = pd.Timestamp(t1, unit='s', tz='UTC').floor('D')
days = pd.date_range(d0, d1 - pd.Timedelta(days=1), freq='D').strftime('%Y-%m-%d')

def wo2(x):
    x = np.sort(np.asarray(x))
    return x[:-2].mean() if len(x) > 4 else float('nan')

rows = []
for d in days:
    x = t[t.day == d]
    rows.append(dict(day=d, n=len(x), mean_racing=x.rr.mean() if len(x) else np.nan, wo2_racing=wo2(x.rr),
                     mean_normal=x.rn.mean() if len(x) else np.nan, wo2_normal=wo2(x.rn),
                     pnl_racing=x.pnl_sol.sum(), pnl_normal=x.pn.sum(), win=(x.rr > 0).mean() if len(x) else np.nan))
D = pd.DataFrame(rows)
pd.set_option('display.width', 200)
print(D.round(4).to_string(index=False))
w = t.sort_values('decision_slot')
if len(w):
    hrs = (w.ts.max() - w.ts.min()) / 3600
    print(f'\nwhole window [{lo}, {hi}): {len(w)} trades over {hrs:.1f} h | racing mean {w.rr.mean():+.4f} w/o best 2 {wo2(w.rr):+.4f} '
          f'win {(w.rr > 0).mean():.0%} PnL {w.pnl_sol.sum():+.2f} SOL | normal mean {w.rn.mean():+.4f} w/o best 2 {wo2(w.rn):+.4f} PnL {w.pn.sum():+.2f} SOL')
x = t[t.day.isin(days)].sort_values('decision_slot')
if len(x) == 0:
    sys.exit()
prof = (D.pnl_racing > 0)
streak = best = 0
for p in prof:
    streak = streak + 1 if not p else 0
    best = max(best, streak)
eq = x.pnl_sol.cumsum().values
dd = (np.maximum.accumulate(np.r_[0, eq]) - np.r_[0, eq]).max()
print(f'\nscored days {len(D)}  trades {len(x)} ({len(x) / max(len(D), 1):.0f}/day)')
print(f'racing fees : mean {x.rr.mean():+.4f}  w/o best 2 {wo2(x.rr):+.4f}  win {(x.rr > 0).mean():.0%}  PnL {x.pnl_sol.sum():+.2f} SOL  '
      f'profitable days {prof.sum()}/{len(D)} ({prof.mean():.0%})  worst day {D.pnl_racing.min():+.2f} SOL  longest losing streak {best} d  max drawdown {dd:.2f} SOL')
pn = D.pnl_normal > 0
print(f'normal fees : mean {x.rn.mean():+.4f}  w/o best 2 {wo2(x.rn):+.4f}  PnL {x.pn.sum():+.2f} SOL  profitable days {pn.sum()}/{len(D)} ({pn.mean():.0%})')
