"""Hedged-inventory strategy on the live executable gaps.

Start each token with a hedged inventory (long X on Solana, short A on Hyperliquid, net delta 0).
At each poll: if buy_edge > th and inventory < max, buy one unit (S USD) on Solana and add to the
HL short; if sell_edge > th and inventory > 0, sell one unit on Solana and reduce the HL short.
Each trade earns its edge (already net of pool fees, price impact and HL taker fee). One trade
per side per cooldown so a single dislocation is not counted repeatedly."""
import json, sys
import numpy as np
import pandas as pd

S = 500.0
R = [json.loads(l) for l in open('poll.jsonl')]
D = pd.DataFrame(R)
span_h = (D.ts.max() - D.ts.min()) / 3600
print(f'polls {len(D)} over {span_h:.2f} h')
pd.set_option('display.width', 220)
summ = D.groupby('sym').agg(n=('ts', 'size'), buy_med=('buy_edge', 'median'), buy_p90=('buy_edge', lambda x: x.quantile(.9)), buy_pos=('buy_edge', lambda x: (x > 0).mean()),
                            sell_med=('sell_edge', 'median'), sell_p90=('sell_edge', lambda x: x.quantile(.9)), sell_pos=('sell_edge', lambda x: (x > 0).mean()))
print((summ * [1, 100, 100, 1, 100, 100, 1]).round(3).to_string())
for th in (0.0, 0.001, 0.002, 0.004):
    tot = {}
    for sym, g in D.sort_values('ts').groupby('sym'):
        inv, max_inv, cd = 2, 4, 300
        last_b = last_s = -1e9
        pnl, nt = 0.0, 0
        for r in g.itertuples():
            be, se = getattr(r, 'buy_edge', np.nan), getattr(r, 'sell_edge', np.nan)
            if be == be and be > th and inv < max_inv and r.ts - last_b > cd:
                pnl += be * S; inv += 1; nt += 1; last_b = r.ts
            if se == se and se > th and inv > 0 and r.ts - last_s > cd:
                pnl += se * S; inv -= 1; nt += 1; last_s = r.ts
        tot[sym] = (nt, round(pnl, 2), inv)
    print(f'threshold {th:.3%}: per token (trades, USD pnl on {S:.0f} USD units, end inventory):', tot,
          ' total USD/day', round(sum(v[1] for v in tot.values()) / max(span_h, 1e-9) * 24, 1))
