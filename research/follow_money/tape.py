"""Load the binary tape written by `lab tape`."""
import numpy as np
import os

TR = np.dtype([
    ('mint', '<u4'), ('user', '<u4'), ('signer', '<u4'), ('slot', '<u4'), ('tx', '<u4'), ('ev', '<u2'),
    ('buy', 'u1'), ('flags', 'u1'), ('sol', '<u8'), ('tok', '<u8'), ('vsol', '<u8'), ('vtok', '<u8'),
    ('fee', '<u8'), ('cu', '<u8'), ('tip', '<u4'), ('shape', '<u4'), ('txfee', '<u4'), ('culim', '<u4')])
CR = np.dtype([('mint', '<u4'), ('dev', '<u4'), ('slot', '<u4'), ('tx', '<u4'), ('flags', '<u4'), ('bt', '<u4'), ('qmint', '<u4')])
CO = np.dtype([('mint', '<u4'), ('slot', '<u4')])


def load(d):
    t = np.fromfile(os.path.join(d, 'trades.bin'), dtype=TR)
    c = np.fromfile(os.path.join(d, 'creates.bin'), dtype=CR)
    k = np.fromfile(os.path.join(d, 'completes.bin'), dtype=CO)
    return t, c, k


def clean(t, c):
    """SOL-quoted, non-mayhem trades on coins whose create we saw (full history)."""
    ok_c = c[(c['flags'] & 1 == 0) & (c['flags'] & 2 != 0)]
    seen = np.zeros(max(t['mint'].max(), c['mint'].max()) + 1, bool)
    seen[ok_c['mint']] = True
    m = (t['flags'] == 2) & seen[t['mint']] & (t['tok'] > 0)
    return t[m], ok_c
