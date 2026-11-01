//! End-to-end brain test on a synthetic serial operator.

use ucore::brain::{Brain, BrainCfg, ExecMode, Intent};
use ucore::curve::Curve;
use ucore::event::*;
use ucore::pk::{Pk, Sig};

fn key(tag: u8, i: u32) -> Pk {
    let mut b = [0u8; 32];
    b[0] = tag;
    b[1..5].copy_from_slice(&i.to_le_bytes());
    b[31] = 1;
    Pk(b)
}

fn meta(slot: u64, tx: u32) -> Meta {
    Meta { slot, tx_idx: tx, block_time: 1_700_000_000 + (slot / 2) as i64, ..Default::default() }
}

struct Gen {
    evs: Vec<Ev>,
}

impl Gen {
    fn trade(&mut self, c: &mut Curve, mint: Pk, user: Pk, slot: u64, tx: u32, buy: bool, amount: u64) -> u64 {
        let fee_bps = 125;
        let (sol, tok) = if buy {
            let (tok, net, _) = c.buy_quote_in(amount, fee_bps);
            c.apply_buy(net, tok);
            (net, tok)
        } else {
            let (_, gross, _) = c.sell(amount, fee_bps);
            c.apply_sell(amount, gross);
            (gross, amount)
        };
        self.evs.push(Ev::Trade(Trade {
            m: Meta { sig: Sig([tx as u8; 64]), ..meta(slot, tx) },
            mint,
            user,
            is_buy: buy,
            sol,
            tok,
            fee: sol * 125 / 10_000,
            fee_bps,
            vsol: c.vsol,
            vtok: c.vtok,
            rsol: c.rsol,
            rtok: c.rtok,
            ix: 4,
            mayhem: false,
            sol_quote: true,
        }));
        tok
    }

    /// One pump-and-dump launch by `dev`, starting at slot0.
    fn launch(&mut self, n: u32, dev: Pk, slot0: u64) {
        let mint = key(9, n);
        let mut c = Curve::initial();
        self.evs.push(Ev::Create(Create {
            m: meta(slot0, 0),
            mint,
            bonding_curve: key(8, n),
            user: dev,
            creator: dev,
            name: format!("T{n}"),
            symbol: format!("T{n}"),
            uri: String::new(),
            token_program: ucore::pump::TOKEN_2022,
            quote_mint: Pk::ZERO,
            mayhem: false,
            cashback: false,
            holder_reward: false,
            depth: 0,
            vsol: c.vsol,
            vtok: c.vtok,
            rtok: c.rtok,
            supply: ucore::pump::TOKEN_SUPPLY,
            creator_fee_bps: 30,
        }));
        self.trade(&mut c, mint, dev, slot0, 0, true, 1_000_000_000);
        let mut held = vec![];
        for i in 0..4u32 {
            let w = key(2, i); // recurring bundle wallets
            let t = self.trade(&mut c, mint, w, slot0, 1 + i, true, 2_000_000_000);
            held.push((w, t));
        }
        self.evs.push(Ev::Block(BlockStat { slot: slot0, block_time: 1_700_000_000 + (slot0 / 2) as i64, ..Default::default() }));
        for s in 1..1600u64 {
            let slot = slot0 + s;
            if s <= 8 {
                for j in 0..2u32 {
                    self.trade(&mut c, mint, key(3, n * 100 + s as u32 * 2 + j), slot, j, true, 1_500_000_000);
                }
            }
            if s == 30 {
                for (j, (w, t)) in held.iter().enumerate() {
                    self.trade(&mut c, mint, *w, slot, j as u32, false, *t);
                }
            }
            self.evs.push(Ev::Block(BlockStat { slot, block_time: 1_700_000_000 + (slot / 2) as i64, ..Default::default() }));
        }
    }
}

fn run(mode: ExecMode) -> Brain {
    let mut g = Gen { evs: vec![] };
    let dev = key(1, 0);
    for n in 0..14u32 {
        g.launch(n, dev, 1_000 + n as u64 * 2_000);
    }
    let mut cfg = BrainCfg::default();
    cfg.mode = mode;
    cfg.risk.default_max_entry_mcap = 200.0;
    let mut b = Brain::new(cfg);
    for e in &g.evs {
        b.on_event(e);
        if mode == ExecMode::Live {
            // pretend the chain filled every buy immediately at the current curve
            let intents: Vec<Intent> = b.intents.drain(..).collect();
            for it in intents {
                match it {
                    Intent::Buy { pos, mint, lamports, .. } => {
                        let c = b.mints.get(&mint).unwrap().curve;
                        let (tok, net, fee) = c.buy_quote_in(lamports, 125);
                        b.on_live_buy_fill(pos, net + fee, net, tok, e.slot(), 2_400_000);
                    }
                    Intent::Sell { pos, mint, tokens, .. } => {
                        let c = b.mints.get(&mint).unwrap().curve;
                        let (out, _, _) = c.sell(tokens, 125);
                        b.on_live_sell_fill(pos, tokens, out, e.slot(), 2_200_000);
                    }
                }
            }
        }
    }
    b
}

#[test]
fn learns_operator_and_trades_it_in_sim() {
    let b = run(ExecMode::Sim);
    assert!(b.counters.attributed_known >= 13, "dev reuse must attribute: {:?}", b.counters);
    assert!(b.counters.gate_pass > 0, "gate never passed: {:?}", b.counters.gate_reasons);
    assert!(!b.closed_real.is_empty(), "no real trades");
    // the gate must wait for evidence: no real trade on the first min_n launches
    let first_real = b.closed_real.iter().map(|t| t.decision_slot).min().unwrap();
    assert!(first_real >= 1_000 + 6 * 2_000, "traded before enough history: {first_real}");
    let pnl: i64 = b.closed_real.iter().map(|t| t.pnl).sum();
    assert!(pnl > 0, "synthetic pump operator should be profitable, pnl {pnl}");
}

#[test]
fn live_mode_emits_intents_and_closes() {
    let b = run(ExecMode::Live);
    assert!(b.counters.real_entries > 0);
    assert!(!b.closed_real.is_empty(), "live positions never closed");
    assert!(b.closed_real.iter().all(|t| !t.shadow));
}
