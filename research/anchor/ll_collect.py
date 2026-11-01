"""Raydium LaunchLab (Bonk.fun, StonkFun, ...) trades and pool creations over a slot range."""
import json, sys, urllib.request, concurrent.futures as cf, time, struct

B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58d(s):
    n = 0
    for ch in s:
        n = n * 58 + B58.index(ch)
    b = n.to_bytes((n.bit_length() + 7) // 8, 'big')
    return b'\0' * (len(s) - len(s.lstrip('1'))) + b
def b58e(b):
    n = int.from_bytes(b, 'big'); s = ''
    while n:
        n, r = divmod(n, 58); s = B58[r] + s
    return '1' * (len(b) - len(b.lstrip(b'\0'))) + s

LL = 'LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj'
TAG = bytes.fromhex('e445a52e51cb9a1d')
TRADE = bytes([189, 219, 127, 211, 78, 230, 97, 238])
CREATE = bytes([151, 215, 226, 9, 118, 161, 115, 174])
TRADE_IX = {bytes([250, 234, 13, 123, 213, 156, 19, 236]), bytes([24, 211, 116, 40, 105, 3, 153, 56]),
            bytes([149, 39, 222, 155, 211, 124, 152, 26]), bytes([95, 200, 71, 34, 8, 9, 11, 166])}
INIT_IX = {bytes([67, 153, 175, 39, 218, 16, 38, 32]), bytes([37, 190, 126, 222, 44, 154, 171, 17])}
URL = 'https://solana-rpc.publicnode.com'
start, end = int(sys.argv[1]), int(sys.argv[2])


def get(slot):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getBlock", "params": [slot, {"encoding": "json", "maxSupportedTransactionVersion": 1, "transactionDetails": "full", "rewards": False}]}).encode()
    for a in range(8):
        try:
            r = json.load(urllib.request.urlopen(urllib.request.Request(URL, data=body, headers={'Content-Type': 'application/json', 'User-Agent': 'curl/8.5.0'}), timeout=60))
            if 'result' in r:
                return slot, r['result']
            if 'skipped' in json.dumps(r.get('error', '')) or 'not available' in json.dumps(r.get('error', '')):
                return slot, None
        except Exception:
            pass
        time.sleep(0.5 * 2 ** a)
    return slot, None


out = open(f'll_{start}_{end}.jsonl', 'w')
n = 0
with cf.ThreadPoolExecutor(20) as ex:
    for slot, blk in ex.map(get, range(start, end)):
        if not blk:
            continue
        for i, tx in enumerate(blk['transactions']):
            if tx['meta']['err'] is not None:
                continue
            m = tx['transaction']['message']
            la = tx['meta'].get('loadedAddresses') or {}
            keys = m['accountKeys'] + la.get('writable', []) + la.get('readonly', [])
            if LL not in keys:
                continue
            ixs = [ix for ix in m['instructions']] + [ix for g in (tx['meta'].get('innerInstructions') or []) for ix in g['instructions']]
            pools = {}
            for ix in ixs:
                if keys[ix['programIdIndex']] != LL:
                    continue
                d = b58d(ix['data'])
                a = [keys[x] for x in ix['accounts']]
                if d[:8] in TRADE_IX and len(a) >= 11:
                    pools[a[4]] = (a[0], a[3], a[9], a[10])
                elif d[:8] in INIT_IX and len(a) >= 8:
                    pools[a[5]] = (a[1], a[3], a[6], a[7])
            for ix in ixs:
                if keys[ix['programIdIndex']] != LL:
                    continue
                d = b58d(ix['data'])
                if d[:8] != TAG or len(d) < 16:
                    continue
                disc, pl = d[8:16], d[16:]
                if disc == TRADE and len(pl) >= 32 + 8 * 13 + 3:
                    ps = b58e(pl[:32])
                    v = struct.unpack_from('<13Q', pl, 32)
                    rec = {'e': 'T', 'pool': ps, 'tbs': v[0], 'vb': v[1], 'vq': v[2], 'rb0': v[3], 'rq0': v[4], 'rb1': v[5], 'rq1': v[6],
                           'ain': v[7], 'aout': v[8], 'pfee': v[9], 'platfee': v[10], 'cfee': v[11], 'sfee': v[12], 'dir': pl[32 + 104], 'status': pl[32 + 105]}
                elif disc == CREATE:
                    ps = b58e(pl[:32])
                    rec = {'e': 'C', 'pool': ps, 'creator': b58e(pl[32:64])}
                else:
                    continue
                u, plat, base, quote = pools.get(ps, (None, None, None, None))
                rec.update({'slot': slot, 'tx': i, 'user': u, 'platform': plat, 'mint': base, 'quote': quote, 'signer': keys[0]})
                out.write(json.dumps(rec) + '\n')
                n += 1
print('launchlab events', n)
