"""Collect every mayhem-program transaction in a slot range: top-level ix data/accounts, inner pump
CPIs, pump events (TradeEvent, UpdateMayhemVirtualParamsEvent) and mayhem-program events."""
import json, sys, urllib.request, concurrent.futures as cf, time

B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58d(s):
    n = 0
    for ch in s:
        n = n * 58 + B58.index(ch)
    b = n.to_bytes((n.bit_length() + 7) // 8, 'big')
    return b'\0' * (len(s) - len(s.lstrip('1'))) + b

MH = 'MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e'
PUMP = '6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'
URL = sys.argv[3] if len(sys.argv) > 3 else 'https://solana-rpc.publicnode.com'
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


out = open(f'mh_{start}_{end}.jsonl', 'w')
n = 0
with cf.ThreadPoolExecutor(24) as ex:
    for slot, blk in ex.map(get, range(start, end)):
        if not blk:
            continue
        for i, tx in enumerate(blk['transactions']):
            m = tx['transaction']['message']
            la = tx['meta'].get('loadedAddresses') or {}
            keys = m['accountKeys'] + la.get('writable', []) + la.get('readonly', [])
            if MH not in keys:
                continue
            rec = {'slot': slot, 'tx': i, 'bt': blk.get('blockTime'), 'sig': tx['transaction']['signatures'][0], 'err': tx['meta']['err'] is not None,
                   'signer': keys[0], 'top': [], 'inner': []}
            for ix in m['instructions']:
                p = keys[ix['programIdIndex']]
                if p in (MH, PUMP):
                    rec['top'].append({'p': p[:4], 'd': b58d(ix['data']).hex(), 'a': [keys[x] for x in ix['accounts']]})
            for g in tx['meta'].get('innerInstructions') or []:
                for ix in g['instructions']:
                    p = keys[ix['programIdIndex']]
                    if p in (MH, PUMP):
                        rec['inner'].append({'p': p[:4], 'd': b58d(ix['data']).hex(), 'a': [keys[x] for x in ix['accounts']][:24]})
            out.write(json.dumps(rec) + '\n')
            n += 1
print('mayhem txs', n)
