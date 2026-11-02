"""Do consistent taker winners persist? Select on development days, measure on validation days.

usage: persistence.py wd.csv <first validation day yyyy-mm-dd> [last day exclusive]
Selection (development days only): non-dev, non-create-slot wallets active (>= MINPOS positions)
on >= MIND days, profitable on >= FRAC of their active days, positive total.
"""
import sys
import pandas as pd, numpy as np
d = pd.read_csv(sys.argv[1])
d['date'] = pd.to_datetime(d.day * 86400, unit='s')
v0 = pd.Timestamp(sys.argv[2])
v1 = pd.Timestamp(sys.argv[3]) if len(sys.argv) > 3 else pd.Timestamp('2100-01-01')
taker = (d.dev <= 0.2 * d.n) & (d.b0 <= 0.2 * d.n)
dev, val = d[(d.date < v0) & taker], d[(d.date >= v0) & (d.date < v1) & taker]
print(f'development days {dev.date.nunique()}  validation days {val.date.nunique()}')
pd.set_option('display.width', 250)
for MINPOS, MIND, FRAC in [(5, 3, 0.7), (10, 4, 0.75), (20, 5, 0.8), (5, 5, 1.0)]:
    a = dev[dev.n >= MINPOS].groupby('wallet').agg(days=('pnl', 'size'), prof=('pnl', lambda x: (x > 0).mean()), pnl=('pnl', 'sum'), spent=('spent', 'sum'))
    sel = a[(a.days >= MIND) & (a.prof >= FRAC) & (a.pnl > 0)]
    v = val[val.wallet.isin(sel.index)]
    vd = v.groupby('wallet').agg(vdays=('pnl', 'size'), vprof=('pnl', lambda x: (x > 0).mean()), vpnl=('pnl', 'sum'), vspent=('spent', 'sum'))
    j = sel.join(vd, how='left')
    act = j.dropna()
    # baseline: all active takers with same activity level in validation
    base = val[val.n >= MINPOS].groupby('wallet').pnl.sum()
    print(f'\nselect pos>={MINPOS}/day, >={MIND} days, profitable >={FRAC:.0%}: {len(sel)} wallets (dev roi {sel.pnl.sum() / sel.spent.sum():+.3f}); '
          f'active in validation {len(act)}; validation roi {act.vpnl.sum() / max(act.vspent.sum(), 1e-9):+.3f}, '
          f'pnl {act.vpnl.sum():+.0f} SOL, wallets profitable {(act.vpnl > 0).mean():.0%}, mean frac of days profitable {act.vprof.mean():.0%} '
          f'| baseline takers profitable {(base > 0).mean():.0%}')
    if len(act):
        x = val[val.wallet.isin(act.index)]
        print('   validation style of persisting winners: hold_s med %.0f, entry age_s med %.0f, entry mcap med %.0f, cu_price med %.0f, tip med %.5f, fast_entry frac %.2f, fast_exit frac %.2f, positions/day med %.0f'
              % (x.hold_s.median(), x.age_s.median(), x.mcap.median(), x.cu_price.median(), x.tip.median(), (x.fast_entry / x.n).mean(), (x.fast_exit / x.n).mean(), x.n.median()))
