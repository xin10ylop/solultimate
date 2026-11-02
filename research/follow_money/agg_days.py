"""Stream a signal-lab CSV: per (signal, exit, UTC day) n / sum of net (incl. fixed costs) / wins."""
import sys
import pandas as pd
src, dst = sys.argv[1], sys.argv[2]
acc = None
for ch in pd.read_csv(src, usecols=['signal', 'exit', 'time', 'net', 'fixed', 'failed'], chunksize=2_000_000):
    ch = ch[ch.failed == 0]
    ch['net'] = ch.net - ch.fixed / 2.5e8
    ch['day'] = pd.to_datetime(ch.time, unit='s').dt.strftime('%m-%d')
    ch['win'] = ch.net > 0
    g = ch.groupby(['signal', 'exit', 'day']).agg(n=('net', 'size'), s=('net', 'sum'), w=('win', 'sum'))
    acc = g if acc is None else acc.add(g, fill_value=0)
acc.to_pickle(dst)
print('groups', len(acc))
