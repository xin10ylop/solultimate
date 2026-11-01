"""Pre-trade on-chain state at every first buy (ages 3..600 s, non-dev), with forward returns.
Shared by the cohort comparison and the imitation model."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import pandas as pd
from tape import load, clean

SL = 0.211
H = [5, 15, 30, 60, 120]


def build(d, W):
    t, c, k = load(d)
    t, c = clean(t, c)
    wal = open(os.path.join(d, 'wallets.txt')).read().split()
    grp = np.zeros(len(wal), bool)
    for i, a in enumerate(wal):
        if any(a.startswith(w) for w in W):
            grp[i] = True
    n = len(t)
    buy = t['buy'].astype(bool)
    sol = t['sol'].astype(np.float64) / 1e9
    tok = t['tok'].astype(np.float64)
    vs = t['vsol'].astype(np.float64)
    vt = t['vtok'].astype(np.float64)
    spot = vs / vt
    pre_vs = np.where(buy, vs - t['sol'], vs + t['sol'])
    pre_vt = np.where(buy, vt + tok, vt - tok)
    prespot = pre_vs / pre_vt
    mint = t['mint']
    slot = t['slot'].astype(np.int64)
    user = t['user']
    cs = np.full(mint.max() + 1, -1, np.int64)
    cs[c['mint']] = c['slot']
    dev = np.full(mint.max() + 1, -1, np.int64)
    dev[c['mint']] = c['dev']
    order = np.argsort(mint, kind='stable')
    ms = mint[order]
    st = np.r_[0, np.flatnonzero(ms[1:] != ms[:-1]) + 1]
    en = np.r_[st[1:], n]
    out = []
    for s, e in zip(st, en):
        ix = order[s:e]
        if len(ix) < 5:
            continue
        us = user[ix]
        bb = buy[ix]
        sl = slot[ix]
        sp = spot[ix]
        ps = prespot[ix]
        so = sol[ix]
        to = tok[ix]
        m = mint[ix[0]]
        age = (sl - cs[m]) * SL
        sgn = np.where(bb, so, -so)
        cum = np.cumsum(sgn)
        cummax_sp = np.maximum.accumulate(sp)
        isdev = us == dev[m]
        devsold_cum = np.cumsum(isdev & ~bb)
        nb_cum = np.cumsum(bb)
        ns_cum = np.cumsum(~bb)
        _, first = np.unique(us, return_index=True)
        isfirst = np.zeros(len(ix), bool)
        isfirst[first] = True
        newbuyers_cum = np.cumsum(isfirst & bb)
        sel = first[bb[first] & (age[first] >= 3) & (age[first] <= 600) & ~isdev[first]]
        for j in sel:
            if j < 2:
                continue
            p = j - 1  # state just before this trade
            def win(sec):
                lo = np.searchsorted(sl, sl[j] - int(sec / SL), side='left')
                return lo
            l5, l15, l30, l60 = win(5), win(15), win(30), win(60)
            def net(lo):
                return cum[p] - (cum[lo - 1] if lo > 0 else 0.0)
            def cnt(arr, lo):
                return arr[p] - (arr[lo - 1] if lo > 0 else 0)
            peak = cummax_sp[p]
            pk30 = sp[l30:j].max() if j > l30 else sp[p]
            fw = []
            for h in H:
                q = np.searchsorted(sl, sl[j] + int(h / SL), side='right') - 1
                fw.append(np.log(sp[q] / ps[j]))  # from the pre-trade spot (what a follower would see)
            out.append(dict(
                grp=grp[us[j]], mint=m, slot=sl[j], age=age[j], vsol=pre_vs[ix[j]] / 1e9,
                size=so[j], dd_peak=ps[j] / peak - 1, dd_30=ps[j] / pk30 - 1,
                net5=net(l5), net15=net(l15), net30=net(l30), net60=net(l60),
                nb30=cnt(nb_cum, l30), ns30=cnt(ns_cum, l30), nnew30=cnt(newbuyers_cum, l30),
                ntr=p + 1, nnew=newbuyers_cum[p], dev_sold=devsold_cum[p] > 0,
                prev_buy=bool(bb[p]), prev_size=so[p], gap_s=(sl[j] - sl[p]) * SL,
                same_slot=int(np.sum(sl[:j] == sl[j])),
                ret_since_launch=np.log(ps[j] / sp[0]),
                **{f'f{h}': v for h, v in zip(H, fw)}))
    return pd.DataFrame(out)


if __name__ == '__main__':
    d = sys.argv[1]
    W = sys.argv[2].split(',')
    df = build(d, W)
    name = os.path.basename(d.rstrip('/'))
    df.to_pickle(f'state_{name}.pkl')
    pd.set_option('display.width', 250)
    cols = ['age', 'vsol', 'size', 'dd_peak', 'dd_30', 'net5', 'net15', 'net30', 'net60', 'nb30', 'ns30', 'nnew30', 'ntr', 'nnew',
            'dev_sold', 'prev_buy', 'prev_size', 'gap_s', 'same_slot', 'ret_since_launch', 'f5', 'f30', 'f120']
    print(name, len(df), df.grp.sum())
    print(df.groupby('grp')[cols].median().T.round(3).to_string())
    print(df.groupby('grp')[['dev_sold', 'prev_buy']].mean().T.round(3).to_string())
