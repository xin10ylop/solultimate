"""Direction of the agent's next action vs the price just BEFORE it acts (no look-ahead)."""
import json, sys, collections
import numpy as np, pandas as pd
E = [json.loads(l) for l in open(sys.argv[1])]
mm = set(e['mint'] for e in E if e['e'] == 'M')
M = set((e['slot'], e['tx'], e['mint']) for e in E if e['e'] == 'M')
T = [e for e in E if e['e'] == 'T' and e['mint'] in mm]
T.sort(key=lambda e: (e['mint'], e['slot'], e['tx'], e['k']))
rows = []
last = {}
hist = collections.defaultdict(list)   # log price after each trade (any trader)
agent_hist = collections.defaultdict(list)
for e in T:
    m = e['mint']
    is_agent = (e['slot'], e['tx'], m) in M
    if is_agent and m in last:
        lp_before = last[m]
        h = hist[m][-20:]
        ah = agent_hist[m][-20:]
        rows.append(dict(mint=m, buy=e['buy'], dev20=lp_before - np.mean(h) if len(h) >= 5 else np.nan,
                         dev_agent=lp_before - np.mean(ah) if len(ah) >= 5 else np.nan,
                         since_agent=lp_before - agent_hist[m][-1] if ah else np.nan,
                         n_between=e.get('_nb', 0)))
    lp = np.log(e['vs'] / e['vt']) if e['vt'] > 0 and e['vs'] > 0 else np.nan
    last[m] = lp
    hist[m].append(lp)
    if is_agent:
        agent_hist[m].append(lp)
D = pd.DataFrame(rows)
print('agent actions with prior price', len(D), ' P(buy)', round(D.buy.mean(), 4))
for col in ('dev20', 'dev_agent', 'since_agent'):
    x = D.dropna(subset=[col])
    print(f'P(buy) by {col} (price before the agent acts, deciles):')
    print(x.groupby(pd.qcut(x[col], 10, duplicates='drop'), observed=True).buy.agg(['size', 'mean']).round(3).to_string())
