import json, sys, hashlib, struct, collections
f = sys.argv[1]
R = [json.loads(l) for l in open(f)]
def sh(ns, n):
    return hashlib.sha256(f'{ns}:{n}'.encode()).digest()[:8].hex()
names = ['buy', 'sell', 'create', 'rebalance', 'set_virtual_params', 'crank', 'update', 'tick', 'trade', 'init', 'initialize', 'close', 'end', 'burn',
         'finalize', 'withdraw', 'claim', 'agent_buy', 'agent_sell', 'mayhem_buy', 'mayhem_sell', 'start', 'stop', 'execute', 'swap', 'step', 'act',
         'update_virtual_params', 'set_mayhem_virtual_params', 'end_mayhem', 'close_mayhem', 'finish', 'settle', 'deposit', 'buy_exact_sol_in',
         'buy_v2', 'sell_v2', 'buy_exact_quote_in_v2', 'expire', 'complete', 'sweep', 'reclaim', 'unwind', 'liquidate', 'create_mayhem', 'init_mayhem',
         'initialize_mayhem_state', 'create_mayhem_state', 'set_params', 'update_params', 'distribute', 'airdrop', 'reward', 'sync', 'refresh', 'rotate']
ixmap = {sh('global', n): n for n in names}
evnames = ['TradeEvent', 'MayhemTradeEvent', 'AgentTradeEvent', 'SellEvent', 'BuyEvent', 'MayhemEvent', 'AgentEvent', 'SwapEvent', 'ActionEvent',
           'MayhemSellEvent', 'MayhemBuyEvent', 'StateEvent', 'MayhemStateEvent', 'UpdateMayhemVirtualParamsEvent', 'EndMayhemEvent', 'CloseEvent',
           'MayhemEndedEvent', 'MayhemStartedEvent', 'CreateEvent', 'BurnEvent', 'TradeExecutedEvent', 'AgentActionEvent', 'MayhemTradeExecutedEvent']
evmap = {sh('event', n): n for n in evnames}
TAG = 'e445a52e51cb9a1d'
cnt = collections.Counter()
inner_cnt = collections.Counter()
ev_cnt = collections.Counter()
signers = collections.Counter()
for r in R:
    signers[r['signer'][:10]] += 1
    tops = [t for t in r['top'] if t['p'] == 'MAyh']
    for t in tops:
        cnt[ixmap.get(t['d'][:16], t['d'][:16])] += 1
    for x in r['inner']:
        if x['d'].startswith(TAG):
            ev_cnt[(x['p'], evmap.get(x['d'][16:32], x['d'][16:32]), len(x['d']) // 2)] += 1
        else:
            inner_cnt[(x['p'], ixmap.get(x['d'][:16], x['d'][:16]))] += 1
print('txs', len(R), 'failed', sum(r['err'] for r in R), 'signers', signers.most_common(5))
print('top-level mayhem ixs:', cnt.most_common())
print('inner ixs:', inner_cnt.most_common(12))
print('events:', ev_cnt.most_common(12))
