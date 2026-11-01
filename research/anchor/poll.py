"""Executable Solana-vs-Hyperliquid gaps for anchored tokens, polled live.

Every round, for each token X with Hyperliquid anchor A and a fixed USD size S:
  buy edge  = HL_bid * qtyX_bought / S - 1 - HL_FEE         (buy X on Solana with USDC, short A on HL)
  sell edge = USDC_out / (HL_ask * qtyX_sold) - 1 - HL_FEE  (sell X on Solana for USDC, cover/long A on HL)
Solana legs are real Jupiter quotes (route, pool fees and price impact included).
"""
import json, time, urllib.request, sys, os, urllib.parse

USDC = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
HL_FEE = 0.00045
SIZES = [float(x) for x in (sys.argv[1] if len(sys.argv) > 1 else '500').split(',')]
TOK = {  # mint: (symbol, HL coin, dex)
    'WXMRyRZhsa19ety5erZhHg4N3xj3EVN92u94422teJp': ('wXMR', 'XMR', None),
    'DoGEV7LASBkQbibMc5k5vKnTZoMg423GpJ5QtJEGfm7R': ('DOGE', 'DOGE', None),
    '3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh': ('WBTC', 'BTC', None),
    '98sMhvDwXj1RQi5c5Mndm3vPe9cBqPrbLaufMXFNMh5g': ('HYPE', 'HYPE', None),
    'pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn': ('PUMP', 'PUMP', None),
    'AMD8XwJXgQ9WV45Wyj9yFLejxzf2J6VM1PJY8bJEjeES': ('AMD', 'xyz:AMD', 'xyz'),
    'Xsa62P5mvPszXL1krVUnU5ar38bBSVcWAB6fmPCo5Zu': ('METAx', 'xyz:META', 'xyz'),
    'Xsc9qvGR1efVDFGLrVsmkzv3qi45LTBjeUKSPmx9qEh': ('NVDAx', 'xyz:NVDA', 'xyz'),
    'SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb': ('SPCX', 'xyz:SPCX', 'xyz'),
    '9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump': ('FARTCOIN', 'FARTCOIN', None),
    '6p6xgHyF7AeE6TZkSmFsko444wqoP15icUSqi2jfGiPN': ('TRUMP', 'TRUMP', None),
    'Dz9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk': ('USELESS', 'USELESS', None),
    'DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263': ('BONK', 'kBONK', None),
    '2zMMhcVQEXDtdE6vsFS7S7D5oUodfJHE8vd1gnBouauv': ('PENGU', 'PENGU', None),
    'JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN': ('JUP', 'JUP', None),
    'HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3': ('PYTH', 'PYTH', None),
    'J3NKxxXZcnNiMjKw9hYb2K4LUxgwB6t1FtPtQVsv3KFr': ('SPX', 'SPX', None),
    'FUAfBo2jgks6gB4Z4LfZkqSZgzNucisEHqnNebaRxM1P': ('MELANIA', 'MELANIA', None),
    'EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm': ('WIF', 'WIF', None),
}
MUL = {'kBONK': 1000.0}
UA = {'Accept': 'application/json', 'User-Agent': 'Mozilla/5.0', 'Content-Type': 'application/json'}


def get(url, body=None):
    try:
        req = urllib.request.Request(url, data=json.dumps(body).encode() if body else None, headers=UA)
        return json.load(urllib.request.urlopen(req, timeout=20))
    except Exception:
        return None


def jup(inp, out, amount):
    q = urllib.parse.urlencode({'inputMint': inp, 'outputMint': out, 'amount': int(amount), 'slippageBps': 100, 'restrictIntermediateTokens': 'true'})
    r = get(f'https://lite-api.jup.ag/swap/v1/quote?{q}')
    if not r or 'outAmount' not in r:
        return None, None
    return int(r['outAmount']), [x['swapInfo'].get('label') for x in r.get('routePlan', [])]


def book(coin, dex):
    body = {'type': 'l2Book', 'coin': coin}
    r = get('https://api.hyperliquid.xyz/info', body)
    if not r or 'levels' not in r:
        return None, None
    bids, asks = r['levels']
    return (float(bids[0]['px']) if bids else None), (float(asks[0]['px']) if asks else None)


# decimals from mint accounts
dec = {}
for m in TOK:
    r = get('https://solana-rpc.publicnode.com', {'jsonrpc': '2.0', 'id': 1, 'method': 'getAccountInfo', 'params': [m, {'encoding': 'jsonParsed'}]})
    try:
        dec[m] = int(r['result']['value']['data']['parsed']['info']['decimals'])
    except Exception:
        dec[m] = None
    time.sleep(0.3)
print('decimals', {TOK[m][0]: d for m, d in dec.items()}, flush=True)
out = open('poll.jsonl', 'a')
while True:
    t0 = time.time()
    for m, (sym, coin, dexn) in TOK.items():
        if dec[m] is None:
            continue
        bid, ask = book(coin, dexn)
        if not bid or not ask:
            continue
        bid, ask = bid / MUL.get(coin, 1.0), ask / MUL.get(coin, 1.0)
        for SIZE in SIZES:
            qb, rb = jup(USDC, m, SIZE * 1e6)  # buy X with S USDC
            time.sleep(1.1)
            mid = (bid + ask) / 2
            qty_sell = SIZE / mid * 10 ** dec[m]
            us, rs = jup(m, USDC, qty_sell)    # sell the same USD amount of X
            time.sleep(1.1)
            rec = {'ts': round(time.time(), 1), 'sym': sym, 'size': SIZE, 'bid': bid, 'ask': ask}
            if qb:
                qty = qb / 10 ** dec[m]
                rec['buy_edge'] = bid * qty / SIZE - 1 - HL_FEE
                rec['buy_route'] = rb
            if us:
                rec['sell_edge'] = (us / 1e6) / (ask * qty_sell / 10 ** dec[m]) - 1 - HL_FEE
                rec['sell_route'] = rs
            out.write(json.dumps(rec) + '\n')
            out.flush()
    dt = time.time() - t0
    time.sleep(max(0, 30 - dt))
