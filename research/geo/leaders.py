"""Slot leaders for our data periods + geolocation of each leader's gossip IP."""
import json, time, urllib.request, math

def rpc(url, m, p):
    for a in range(6):
        try:
            body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": m, "params": p}).encode()
            return json.load(urllib.request.urlopen(urllib.request.Request(url, data=body, headers={'Content-Type': 'application/json', 'User-Agent': 'curl/8.5.0'}), timeout=60))['result']
        except Exception:
            time.sleep(2 * (a + 1))
    return None

leaders = {}
for a, b in ((463424017, 464188821),):
    s = a
    while s < b:
        r = rpc('https://solana-rpc.publicnode.com', 'getSlotLeaders', [s, min(5000, b - s)])
        if r is None:
            print('failed at', s); break
        for i, l in enumerate(r):
            leaders[s + i] = l
        s += len(r)
json.dump(leaders, open('slot_leaders.json', 'w'))
uniq = sorted(set(leaders.values()))
print('slots', len(leaders), 'unique leaders', len(uniq))
nodes = json.load(open('nodes.json'))['result']
ip = {n['pubkey']: n['gossip'].split(':')[0] for n in nodes if n.get('gossip')}
todo = [ip[l] for l in uniq if l in ip]
geo = {}
for i in range(0, len(todo), 100):
    batch = [{'query': x, 'fields': 'query,country,city,lat,lon,as'} for x in todo[i:i + 100]]
    req = urllib.request.Request('http://ip-api.com/batch', data=json.dumps(batch).encode(), headers={'Content-Type': 'application/json'})
    for r in json.load(urllib.request.urlopen(req, timeout=60)):
        geo[r['query']] = r
    time.sleep(4.5)
FRA = (50.11, 8.68)
def km(a, b):
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 6371 * 2 * math.asin(math.sqrt(h))
out = {}
for l in uniq:
    g = geo.get(ip.get(l, ''), {})
    if 'lat' in g:
        out[l] = {'city': g.get('city'), 'country': g.get('country'), 'as': g.get('as'), 'km_fra': round(km(FRA, (g['lat'], g['lon'])))}
json.dump(out, open('leader_geo.json', 'w'))
from collections import Counter
w = Counter()
for s, l in leaders.items():
    d = out.get(l, {}).get('km_fra')
    w['unknown' if d is None else ('<500 km' if d < 500 else '500-2000 km' if d < 2000 else '>2000 km')] += 1
print('share of slots by leader distance from Frankfurt:', {k: f'{v / len(leaders):.1%}' for k, v in w.items()})
cities = Counter()
for s, l in leaders.items():
    g = out.get(l)
    if g: cities[f"{g['city']}, {g['country']}"] += 1
print('top leader cities by slots:', [(c, f'{n / len(leaders):.1%}') for c, n in cities.most_common(12)])
