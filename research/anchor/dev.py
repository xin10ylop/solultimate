"""Deviation of Solana pool prices from the Hyperliquid anchor, persistence, and a hedged
'trade the gap' rule: enter when |dev| > entry threshold (minute close), exit when |dev| < exit or
after max_hold minutes. PnL per unit = |dev_entry| - |dev_exit| (sign-aware) minus costs:
DEX fee on both legs (several tiers), HL taker 0.045% on both legs, slippage = 0 here (size-free
upper bound; slippage is handled later with pool depth)."""
import json, glob, sys
import numpy as np
import pandas as pd

HL_FEE = 0.00045
rows = []
for f in sorted(glob.glob('ohlcv/*_*.json')):
    if f.split('/')[-1].startswith('HL_'):
        continue
    sym, short = f.split('/')[-1][:-5].split('_')
    coin = {'wXMR': 'XMR', 'DOGE': 'DOGE', 'WBTC': 'BTC', 'HYPE': 'HYPE'}[sym]
    g = json.load(open(f))
    hl = json.load(open(f'ohlcv/HL_{coin}.json'))
    H = pd.Series({int(c['t']) // 1000: float(c['c']) for c in hl})
    G = pd.DataFrame(g['rows'], columns=['ts', 'o', 'h', 'l', 'c', 'v']).drop_duplicates('ts').set_index('ts').sort_index()
    G = G[G.index.isin(H.index)]
    if len(G) < 100:
        print(sym, short, 'no overlap'); continue
    dev = G.c / H.reindex(G.index) - 1
    vol = G.v
    # persistence: lag-k autocorrelation on minutes that have consecutive candles
    s = dev.copy()
    ac1 = s.autocorr(1) if len(s) > 10 else np.nan
    ac10 = s.autocorr(10) if len(s) > 20 else np.nan
    # trade rule on the minute grid (only minutes with trades)
    res = {}
    for th in (0.005, 0.01, 0.02, 0.03):
        pnl, n, holds = [], 0, []
        i, idx, v = 0, s.index.values, s.values
        while i < len(v):
            if abs(v[i]) >= th:
                sgn = np.sign(v[i])
                j = i + 1
                while j < len(v) and idx[j] - idx[i] <= 60 * 60 and sgn * v[j] > th * 0.2:
                    j += 1
                if j >= len(v):
                    break
                pnl.append(sgn * (v[i] - v[j]))
                holds.append((idx[j] - idx[i]) / 60)
                i = j + 1
            else:
                i += 1
        res[th] = (len(pnl), np.mean(pnl) if pnl else np.nan, np.median(holds) if holds else np.nan, np.sum(pnl) if pnl else 0)
    days = (s.index.max() - s.index.min()) / 86400
    rows.append(dict(token=sym, pool=f"{short} {g['name']}", minutes=len(s), days=round(days, 2), vol_usd_day=round(vol.sum() / max(days, 1e-9)),
                     med_abs=round(s.abs().median(), 4), p90_abs=round(s.abs().quantile(.9), 4), p99_abs=round(s.abs().quantile(.99), 4),
                     share_gt1=round((s.abs() > 0.01).mean(), 3), share_gt2=round((s.abs() > 0.02).mean(), 3), ac1=round(ac1, 2), ac10=round(ac10, 2),
                     **{f'n@{th}': res[th][0] for th in res}, **{f'gross@{th}': round(res[th][1], 4) for th in res}, **{f'hold@{th}': res[th][2] for th in res}))
R = pd.DataFrame(rows)
pd.set_option('display.width', 250)
print(R[['token', 'pool', 'days', 'minutes', 'vol_usd_day', 'med_abs', 'p90_abs', 'p99_abs', 'share_gt1', 'share_gt2', 'ac1', 'ac10']].to_string(index=False))
print(R[['token', 'pool'] + [c for c in R.columns if c.startswith('n@') or c.startswith('gross@') or c.startswith('hold@')]].to_string(index=False))
print('net per trade = gross - 2*DEX fee - 2*0.045% (HL). E.g. with a 0.25% pool: subtract 0.59%; 0.05% pool: subtract 0.19%.')
