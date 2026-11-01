"""Walk-forward test of the identity-free flow model: the consistency check.

For every period k (a day on real 30-day data; an hour on a short pilot):
  train on the trailing `--train-periods` periods before k only,
  choose the entry threshold on that training data only,
  trade period k (one open trade per coin, at most --max-open at once),
and report the PnL of every period. A consistent edge is positive in most periods,
not carried by one.

usage: python research/walkforward.py data/research/flow.csv --period 86400 --label h60
"""
import argparse

import lightgbm as lgb
import numpy as np
import pandas as pd

ap = argparse.ArgumentParser()
ap.add_argument("csv")
ap.add_argument("--label", default="h60")
ap.add_argument("--period", type=int, default=86400, help="seconds per walk-forward period")
ap.add_argument("--train-periods", type=int, default=14)
ap.add_argument("--min-train-periods", type=int, default=3)
ap.add_argument("--stake", type=float, default=0.25)
ap.add_argument("--top-q", type=float, default=0.02, help="enter on the top q of training predictions")
ap.add_argument("--max-open", type=int, default=3)
ap.add_argument("--base-only", action="store_true", help="drop the wallet-derived features")
a = ap.parse_args()

df = pd.read_csv(a.csv).sort_values("slot").reset_index(drop=True)
feat_cols = [c for c in df.columns[3:] if c[:2] not in ("r_", "fx", "ex", "fe")]
WALLET = ["how_linked", "fund_kind", "fund_sol", "fund_age_s", "op_created_before", "op_resolved", "op_mean_ln_peak", "op_frac_2x"]
if a.base_only:
    feat_cols = [c for c in feat_cols if c not in WALLET]
L = a.label
df["net"] = df[f"r_{L}"] - df[f"fx_{L}"] / (a.stake * 1e9)
df = df[df.net.notna()]
df["period"] = ((df.time - df.time.min()) // a.period).astype(int)
hold_slots = {"h5": 5, "h15": 15, "h30": 30, "h60": 60}[L] * 1000 // 211 + 5
params = dict(objective="huber", alpha=0.3, learning_rate=0.03, num_leaves=15, min_data_in_leaf=300,
              feature_fraction=0.8, bagging_fraction=0.8, bagging_freq=1, lambda_l2=10.0, verbose=-1, seed=1)


def trade(sel):
    """Chronological: one open trade per coin, at most max_open concurrent."""
    out, open_until, coin_until = [], [], {}
    for _, r in sel.sort_values("slot").iterrows():
        open_until = [u for u in open_until if u > r.slot]
        if coin_until.get(r.mint, 0) > r.slot or len(open_until) >= a.max_open:
            continue
        coin_until[r.mint] = r.slot + hold_slots
        open_until.append(r.slot + hold_slots)
        out.append(r.net)
    return np.array(out)


rows = []
periods = sorted(df.period.unique())
for k in periods:
    if k < a.min_train_periods:
        continue
    tr = df[(df.period < k) & (df.period >= k - a.train_periods)]
    te = df[df.period == k]
    if len(tr) < 2000 or len(te) == 0:
        continue
    m = lgb.train(params, lgb.Dataset(tr[feat_cols], tr.net.clip(-1, 3)), num_boost_round=300)
    thr = np.quantile(m.predict(tr[feat_cols]), 1 - a.top_q)
    p = m.predict(te[feat_cols])
    x = trade(te[p >= thr])
    rows.append((k, len(te), len(x), x.mean() if len(x) else np.nan, (x > 0).mean() if len(x) else np.nan, x.sum() * a.stake))

r = pd.DataFrame(rows, columns=["period", "decisions", "trades", "mean_ret", "win", "pnl_sol"])
print(r.to_string(index=False, float_format=lambda v: f"{v:+.4f}"))
traded = r[r.trades > 0]
allx = traded.pnl_sol.sum()
print(f"\nperiods traded {len(traded)}  profitable {(traded.pnl_sol > 0).sum()}  total PnL {allx:+.3f} SOL  "
      f"mean ret/trade {np.nansum(traded.mean_ret * traded.trades) / max(traded.trades.sum(), 1):+.4f}")
if len(traded):
    best = traded.pnl_sol.max()
    print(f"best period share of PnL: {best / allx:.2f}" if allx > 0 else "no positive total")
