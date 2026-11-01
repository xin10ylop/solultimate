"""Minute candles for Solana pools of anchored tokens (GeckoTerminal) and the anchor (Hyperliquid)."""
import json, sys, time, urllib.request, os

UA = {'Accept': 'application/json', 'User-Agent': 'Mozilla/5.0'}
def get(url, body=None):
    for a in range(6):
        try:
            req = urllib.request.Request(url, data=json.dumps(body).encode() if body else None, headers={**UA, 'Content-Type': 'application/json'})
            return json.load(urllib.request.urlopen(req, timeout=60))
        except Exception as e:
            time.sleep(3 * (a + 1))
    return None

# token symbol -> (pool addresses, HL coin)
POOLS = {
    'wXMR': (['GgoFghiA6Ca7', 'BFKBQT4yFZPt'], 'XMR'),
    'DOGE': (['4n9rkpKkPUGF', '9zuA8cjRTpTk', '7s9GwyhHTmsz'], 'DOGE'),
    'WBTC': (['B5EwJVDuAauz', '4kdxjt8pKEW4'], 'BTC'),
    'HYPE': (['31KrYUDzgEQh', '6oQ9wVex4mKZ', 'ANCx141SujgV'], 'HYPE'),
}
pages = int(sys.argv[1]) if len(sys.argv) > 1 else 8
os.makedirs('ohlcv', exist_ok=True)
full = {}
for sym in POOLS:
    d = json.load(open(f'pools_{sym}.json'))['data']
    for p in d:
        full[p['attributes']['address'][:12]] = (p['attributes']['address'], p['attributes']['name'])
now = int(time.time())
for sym, (pls, coin) in POOLS.items():
    for short in pls:
        addr, name = full[short]
        out, before = [], now
        for pg in range(pages):
            r = get(f'https://api.geckoterminal.com/api/v2/networks/solana/pools/{addr}/ohlcv/minute?aggregate=1&limit=1000&currency=usd&before_timestamp={before}')
            time.sleep(2.2)
            if not r:
                break
            rows = r['data']['attributes']['ohlcv_list']
            if not rows:
                break
            out += rows
            before = min(x[0] for x in rows)
        # base token must be the anchored token; GT returns price of the pool's base token
        json.dump({'pool': addr, 'name': name, 'rows': out}, open(f'ohlcv/{sym}_{short}.json', 'w'))
        print(sym, short, name, len(out), 'candles', flush=True)
    # anchor: Hyperliquid 1m candles over the same span (5000 per request)
    hl = []
    end = now * 1000
    start = (now - pages * 1000 * 60 - 3600) * 1000
    t0 = start
    while t0 < end:
        t1 = min(t0 + 5000 * 60_000, end)
        r = get('https://api.hyperliquid.xyz/info', {'type': 'candleSnapshot', 'req': {'coin': coin, 'interval': '1m', 'startTime': t0, 'endTime': t1}})
        if r:
            hl += r
        t0 = t1
    json.dump(hl, open(f'ohlcv/HL_{coin}.json', 'w'))
    print('HL', coin, len(hl), flush=True)
