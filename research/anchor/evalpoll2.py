"""Live executable gaps by token and size: how often each side is positive, how long positive
spells last, and the hedged-inventory capture (USD/day) with capital tied up."""
import json, sys
import numpy as np
import pandas as pd

f = sys.argv[1] if len(sys.argv) > 1 else 'poll.jsonl'
D = pd.DataFrame([json.loads(l) for l in open(f)])
if 'size' not in D:
    D['size'] = 500.0
span_h = (D.ts.max() - D.ts.min()) / 3600
print(f'{f}: polls {len(D)} over {span_h:.2f} h')
pd.set_option('display.width', 230)
g = D.groupby(['sym', 'size'])
summ = g.agg(n=('ts', 'size'), buy_med=('buy_edge', 'median'), buy_max=('buy_edge', 'max'), buy_pos=('buy_edge', lambda x: (x > 0).mean()),
             sell_med=('sell_edge', 'median'), sell_max=('sell_edge', 'max'), sell_pos=('sell_edge', lambda x: (x > 0).mean()),
             spread_med=('buy_edge', lambda x: np.nan))
summ['round_trip_med'] = g.apply(lambda x: (x.buy_edge + x.sell_edge).median(), include_groups=False)
cols = ['buy_med', 'buy_max', 'sell_med', 'sell_max', 'round_trip_med']
summ[cols] *= 100
print(summ.drop(columns=['spread_med']).round(3).to_string())
# hedged inventory capture per (token, size): start with 2 units, max 4, cooldown 5 min per side
print('\nhedged-inventory capture (threshold 0.10% per side):')
out = []
for (sym, size), x in D.sort_values('ts').groupby(['sym', 'size']):
    inv, mx, cd, lb, ls, pnl, nt = 2, 4, 300, -1e9, -1e9, 0.0, 0
    for r in x.itertuples():
        be, se = r.buy_edge, r.sell_edge
        if be == be and be > 0.001 and inv < mx and r.ts - lb > cd:
            pnl += be * size; inv += 1; nt += 1; lb = r.ts
        if se == se and se > 0.001 and inv > 0 and r.ts - ls > cd:
            pnl += se * size; inv -= 1; nt += 1; ls = r.ts
    capital = size * 4 * 1.25  # max inventory on Solana + ~25% HL margin
    day = pnl / max(span_h, 1e-9) * 24
    out.append((sym, size, nt, round(pnl, 2), inv, round(day, 1), round(day / capital * 100, 2)))
print(pd.DataFrame(out, columns=['sym', 'size', 'trades', 'usd', 'end_inv', 'usd_per_day', 'pct_capital_per_day']).to_string(index=False))
