//! Decode a saved getBlock response with PumpSwap decoding on (set AMM_BLOCK=path to run).
use ucore::event::Ev;
use ucore::rpcjson::{decode_block, parse_block_response};
use ucore::tx::{DecodeCfg, DecodeState};

#[test]
fn amm_block() {
    let Ok(p) = std::env::var("AMM_BLOCK") else { return };
    let bytes = std::fs::read(p).unwrap();
    let b = parse_block_response(&bytes).unwrap().unwrap();
    let cfg = DecodeCfg { decode_amm: true, ..DecodeCfg::default() };
    let mut st = DecodeState::default();
    let evs = decode_block(464219300, &b, &cfg, &mut st);
    let n = evs.iter().filter(|e| matches!(e, Ev::Pool(_))).count();
    let ntx = b.transactions.len();
    let amm = ucore::amm::PUMP_AMM;
    let mut touch = 0;
    let mut inner_amm = 0;
    for (i, t) in b.transactions.iter().enumerate() {
        if let Some(tv) = ucore::rpcjson::to_txview(464219300, i as u32, 0, t) {
            if tv.touches(&amm) {
                touch += 1;
                for (_, inner) in &tv.inner {
                    for ix in inner {
                        if tv.key(ix.program) == amm && ix.data.len() > 16 && ix.data[..8] == ucore::pump::EVENT_IX_TAG {
                            inner_amm += 1;
                        }
                    }
                }
            }
        }
    }
    println!("txs {ntx} amm-touching {touch} amm event cpis {inner_amm} pool events {n}");
}
