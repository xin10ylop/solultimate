"""Round trip SOL -> X -> SOL via Jupiter for memecoins quoted in anchored tokens. A ratio above 1
(after a tx cost) is an atomic arbitrage across routes/venues."""
import json, time, urllib.request, urllib.parse, sys

WSOL = 'So11111111111111111111111111111111111111112'
LAMPORTS = int(float(sys.argv[1]) * 1e9) if len(sys.argv) > 1 else 500_000_000
M = [l.split()[0] for l in open('memes.txt')][:12]


def jup(inp, out, amount):
    q = urllib.parse.urlencode({'inputMint': inp, 'outputMint': out, 'amount': int(amount), 'slippageBps': 100})
    try:
        r = json.load(urllib.request.urlopen(urllib.request.Request(f'https://lite-api.jup.ag/swap/v1/quote?{q}', headers={'Accept': 'application/json'}), timeout=20))
        return int(r['outAmount']), [x['swapInfo'].get('label') for x in r.get('routePlan', [])]
    except Exception:
        return None, None


out = open('roundtrip.jsonl', 'a')
while True:
    t0 = time.time()
    for m in M:
        a, ra = jup(WSOL, m, LAMPORTS)
        time.sleep(1.5)
        if not a:
            continue
        b, rb = jup(m, WSOL, a)
        time.sleep(1.5)
        if not b:
            continue
        out.write(json.dumps({'ts': round(time.time(), 1), 'mint': m, 'ratio': b / LAMPORTS, 'route_in': ra, 'route_out': rb}) + '\n')
        out.flush()
    time.sleep(max(0, 120 - (time.time() - t0)))
