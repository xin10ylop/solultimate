"""Leaderboard for the signal lab: which (signal, exit) pairs are profitable *and* stable?

usage: python research/signal_board.py data/research/sig.csv [--stake 0.25] [--min-n 40]
Net return includes curve fees, slippage, our impact and fixed tx costs at --stake.
Stability: mean in each time half, share of profitable hours, best hour's share of PnL.
"""
import argparse

import numpy as np
import pandas as pd

ap = argparse.ArgumentParser()
ap.add_argument("csv", nargs="+")
ap.add_argument("--stake", type=float, default=0.25)
ap.add_argument("--min-n", type=int, default=40)
ap.add_argument("--top", type=int, default=40)
ap.add_argument("--signal", default="")
a = ap.parse_args()

df = pd.concat([pd.read_csv(c) for c in a.csv], ignore_index=True)
df["ret"] = df.net - df.fixed / (a.stake * 1e9)
df = df[df.ret.notna()]
mid = df.time.median()
df["half"] = (df.time > mid).astype(int)
df["hour"] = ((df.time - df.time.min()) // 3600).astype(int)
if a.signal:
    df = df[df.signal == a.signal]

rows = []
for (s, e), g in df.groupby(["signal", "exit"]):
    if len(g) < a.min_n:
        continue
    h = g.groupby("half").ret.mean()
    hr = g.groupby("hour").ret.agg(["sum", "count"])
    hr = hr[hr["count"] >= 2]
    tot = g.ret.sum()
    rng = np.random.default_rng(0)
    boots = [rng.choice(g.ret.values, len(g)).mean() for _ in range(300)]
    rows.append(dict(signal=s, exit=e, n=len(g), per_day=len(g) / ((df.time.max() - df.time.min()) / 86400),
                     mean=g.ret.mean(), median=g.ret.median(), win=(g.ret > 0).mean(),
                     h1=h.get(0, np.nan), h2=h.get(1, np.nan), lb=np.percentile(boots, 2.5),
                     hours_pos=(hr["sum"] > 0).mean() if len(hr) else np.nan,
                     best_hour_share=(hr["sum"].max() / tot) if tot > 0 else np.nan,
                     refused=g.failed.mean()))
r = pd.DataFrame(rows)
r["stable"] = r[["h1", "h2"]].min(axis=1)
r = r.sort_values("stable", ascending=False)
pd.set_option("display.width", 250)
print(r.head(a.top).to_string(index=False, float_format=lambda v: f"{v:+.3f}"))
print(f"\n{len(r)} pairs with n ≥ {a.min_n}; both halves positive: {(r.stable > 0).sum()}")
