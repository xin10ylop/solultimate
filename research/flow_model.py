"""Identity-free flow model: does order-flow state predict profitable round trips
after realistic latency and all costs, out of sample and stably over time?

usage: python research/flow_model.py data/research/flow.csv [--stake 0.25] [--label h15]
Produces a text report and research/flow_model.json (LightGBM dump) when asked to --save.
"""
import argparse
import json
import math
import sys

import lightgbm as lgb
import numpy as np
import pandas as pd

ap = argparse.ArgumentParser()
ap.add_argument("csv")
ap.add_argument("--stake", type=float, default=0.25, help="SOL per trade for fixed-cost accounting")
ap.add_argument("--label", default="all")
ap.add_argument("--train-frac", type=float, default=0.6)
ap.add_argument("--save", default="")
a = ap.parse_args()

df = pd.read_csv(a.csv)
df = df.sort_values("slot").reset_index(drop=True)
labels = [c[2:] for c in df.columns if c.startswith("r_")]
feat_cols = [c for c in df.columns[3:] if not c[:3] in ("r_h", "fx_", "ex_", "fe_")]
stake_l = a.stake * 1e9
print(f"rows {len(df)}  coins {df.mint.nunique()}  hours {(df.time.max()-df.time.min())/3600:.1f}  features {len(feat_cols)}")

for l in labels:
    df[f"net_{l}"] = df[f"r_{l}"] - df[f"fx_{l}"] / stake_l

print("\nUnconditional (every decision point), net of curve fees, slippage and fixed tx costs:")
for l in labels:
    x = df[f"net_{l}"].dropna()
    print(f"  {l:4s} mean {x.mean():+.4f}  median {x.median():+.4f}  win {(x>0).mean():.1%}  refused {df[f'fe_{l}'].mean():.1%}")

cut = df.slot.quantile(a.train_frac)
tr = df[df.slot <= cut]
te = df[df.slot > cut].copy()
print(f"\ntrain rows {len(tr)} (≤ slot {int(cut)}), test rows {len(te)}")


def one_per_coin(sel, gap_slots):
    """Keep chronological picks with at most one open trade per coin (gap in slots)."""
    keep = []
    last = {}
    for i, r in sel.sort_values("slot").iterrows():
        if r.mint in last and r.slot - last[r.mint] < gap_slots:
            continue
        last[r.mint] = r.slot
        keep.append(i)
    return sel.loc[keep]


models = {}
for l in (labels if a.label == "all" else [a.label]):
    y = tr[f"net_{l}"].clip(-1, 3)
    m = tr[f"net_{l}"].notna()
    params = dict(objective="huber", alpha=0.3, learning_rate=0.03, num_leaves=15, min_data_in_leaf=300,
                  feature_fraction=0.8, bagging_fraction=0.8, bagging_freq=1, lambda_l2=10.0, verbose=-1, seed=1)
    ds = lgb.Dataset(tr.loc[m, feat_cols], y[m])
    model = lgb.train(params, ds, num_boost_round=300)
    models[l] = model
    te[f"p_{l}"] = model.predict(te[feat_cols])
    print(f"\n=== label {l}: out-of-sample (later {1-a.train_frac:.0%} of time) ===")
    hold = {"h5": 5, "h15": 15, "h30": 30, "h60": 60}.get(l, 30)
    gap = int(hold * 1000 / 211) + 5
    for q in (0.20, 0.10, 0.05, 0.02, 0.01):
        thr = te[f"p_{l}"].quantile(1 - q)
        sel = one_per_coin(te[te[f"p_{l}"] >= thr], gap)
        x = sel[f"net_{l}"].dropna()
        if len(x) < 10:
            continue
        hrs = ((sel.time - te.time.min()) // 3600).astype(int)
        by_h = sel.groupby(hrs)[f"net_{l}"].mean()
        boots = [x.sample(len(x), replace=True).mean() for _ in range(500)]
        print(f"  top {q:>4.0%}: trades {len(x):5d} mean {x.mean():+.4f} median {x.median():+.4f} win {(x>0).mean():.1%} "
              f"boot2.5% {np.percentile(boots, 2.5):+.4f} | hours positive {int((by_h>0).sum())}/{len(by_h)}")
    imp = sorted(zip(model.feature_importance("gain"), feat_cols), reverse=True)[:8]
    print("  top features:", ", ".join(f"{n}" for _, n in imp))

if a.save:
    out = {l: m.dump_model() for l, m in models.items()}
    out["_features"] = feat_cols
    json.dump(out, open(a.save, "w"))
    print("saved", a.save)
