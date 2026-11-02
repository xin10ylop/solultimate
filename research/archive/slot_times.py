"""Slot -> unix time anchors (getBlockTime on a grid), for per-UTC-day scoring of replays."""
import json, sys, time, urllib.request
RPC = 'https://api.mainnet-beta.solana.com'
a, b, step = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
out = open(sys.argv[4], 'w')
def call(m, p):
    req = urllib.request.Request(RPC, data=json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': m, 'params': p}).encode(),
                                 headers={'Content-Type': 'application/json', 'User-Agent': 'curl/8.5.0'})
    return json.load(urllib.request.urlopen(req, timeout=20))
s = a
while s <= b:
    for k in range(20):
        try:
            r = call('getBlockTime', [s + k])
        except Exception:
            time.sleep(2); continue
        if r.get('result'):
            out.write(f'{s + k},{r["result"]}\n'); out.flush(); break
        time.sleep(0.15)
    s += step
    time.sleep(0.15)
