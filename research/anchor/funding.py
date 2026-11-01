"""30 days of hourly Hyperliquid funding for every perp (main dex)."""
import json, time, urllib.request

def post(body):
    for a in range(5):
        try:
            req = urllib.request.Request('https://api.hyperliquid.xyz/info', data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
            return json.load(urllib.request.urlopen(req, timeout=30))
        except Exception:
            time.sleep(2 * (a + 1))
    return None

meta = post({'type': 'metaAndAssetCtxs'})
coins = [u['name'] for u in meta[0]['universe'] if not u.get('isDelisted')]
now = int(time.time() * 1000)
start = now - 31 * 86400 * 1000
out = {}
for i, c in enumerate(coins):
    rows, t0 = [], start
    while t0 < now:
        r = post({'type': 'fundingHistory', 'coin': c, 'startTime': t0})
        if not r:
            break
        rows += r
        last = max(x['time'] for x in r)
        if last <= t0 or len(r) < 400:
            break
        t0 = last + 1
    out[c] = [(x['time'], float(x['fundingRate']), float(x.get('premium', 0))) for x in rows]
    time.sleep(0.25)
json.dump(out, open('funding30d.json', 'w'))
print('coins', len(out), 'points', sum(len(v) for v in out.values()))
