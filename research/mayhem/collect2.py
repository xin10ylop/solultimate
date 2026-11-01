"""Full window: every pump TradeEvent, every MayhemTradeEvent, every UpdateMayhemVirtualParamsEvent,
plus the top-level instruction list of each tx that touches pump or mayhem. One JSON line per event."""
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

MH = 'MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e'
PUMP = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'
TAG = bytes.fromhex('e445a52e51cb9a1d')
TRADE = bytes.fromhex('bddb7fd34ee661ee')
UPD = bytes([117, 123, 228, 182, 161, 168, 220, 214])
MTE = bytes.fromhex('31487b2d6e40b085')
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


out = open(f'full_{start}_{end}.jsonl', 'w')
n = 0
with cf.ThreadPoolExecutor(24) as ex:
    for slot, blk in ex.map(get, range(start, end)):
        if not blk:
            continue
        for i, tx in enumerate(blk['transactions']):
            if tx['meta']['err'] is not None:
                continue
            m = tx['transaction']['message']
            la = tx['meta'].get('loadedAddresses') or {}
            keys = m['accountKeys'] + la.get('writable', []) + la.get('readonly', [])
            if PUMP not in keys:
                continue
            tops = [b58d(ix['data'])[:8].hex() for ix in m['instructions'] if keys[ix['programIdIndex']] in (MH, PUMP)]
            k = 0
            for g in tx['meta'].get('innerInstructions') or []:
                for ix in g['instructions']:
                    p = keys[ix['programIdIndex']]
                    if p not in (MH, PUMP):
                        continue
                    d = b58d(ix['data'])
                    if d[:8] != TAG:
                        continue
                    disc, pl = d[8:16], d[16:]
                    rec = None
                    if p == PUMP and disc == TRADE:
                        sol, tok = struct.unpack_from('<QQ', pl, 32)
                        ts, vs, vt, rs, rt = struct.unpack_from('<qQQQQ', pl, 81)
                        rec = {'e': 'T', 'mint': b58e(pl[:32]), 'buy': pl[48], 'user': b58e(pl[49:81]), 'sol': sol, 'tok': tok, 'vs': vs, 'vt': vt, 'rs': rs, 'rt': rt}
                    elif p == PUMP and disc == UPD:
                        ts = struct.unpack_from('<q', pl, 0)[0]
                        vt0, vs0, vt1, vs1, rt, rs = struct.unpack_from('<QQQQQQ', pl, 40)
                        rec = {'e': 'U', 'mint': b58e(pl[8:40]), 'vt0': vt0, 'vs0': vs0, 'vt1': vt1, 'vs1': vs1, 'rt': rt, 'rs': rs}
                    elif p == MH and disc == MTE:
                        rec = {'e': 'M', 'mint': b58e(pl[33:65]), 'raw': pl.hex()}
                    if rec:
                        rec.update({'slot': slot, 'tx': i, 'k': k, 'signer': keys[0][:12], 'tops': tops})
                        out.write(json.dumps(rec) + '\n')
                        n += 1
                        k += 1
print('events', n)
