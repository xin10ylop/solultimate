"""Who occupies the transaction right after a create: insiders (one creator) or independent snipers (many creators)?"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np, pandas as pd
from tape import load, clean
d = sys.argv[1]
t, c, k = load(d); t, c = clean(t, c)
wal = open(os.path.join(d, 'wallets.txt')).read().split()
cs = {int(m): (int(s), int(x), int(dv)) for m, s, x, dv in zip(c['mint'], c['slot'], c['tx'], c['dev'])}
m = np.array([cs.get(int(x), (-1, -1, -1))[0] for x in t['mint']]) == t['slot']
g = t[m & (t['buy'] == 1)]
rows = []
for r in g:
    s, x, dv = cs[int(r['mint'])]
    if int(r['tx']) == x + 1 and int(r['user']) != dv:
        rows.append((int(r['user']), int(r['signer']), dv, int(r['mint']), r['tip'] / 1e9, r['cu'], r['sol'] / 1e9))
D = pd.DataFrame(rows, columns=['user', 'signer', 'dev', 'mint', 'tip', 'cu', 'sol'])
# a dev's "operator" proxy: the dev wallet itself (creators rotate wallets, so this undercounts insiders)
per = D.groupby('signer').agg(n=('mint', 'nunique'), devs=('dev', 'nunique'), tip=('tip', 'median'), sol=('sol', 'median'))
print(os.path.basename(d.rstrip('/')), 'gap-1 positions', len(D), 'distinct signers', len(per))
print('signers by number of distinct creators they appear with:')
print(pd.cut(per.devs, [0, 1, 2, 5, 20, 1e9]).value_counts().sort_index().to_string())
top = per.sort_values('n', ascending=False).head(10)
top['addr'] = [wal[i][:10] for i in top.index]
print(top.to_string())
print('share of gap-1 positions by signers seen with >=5 different creators:', round(D.signer.map(per.devs).ge(5).mean(), 3))
