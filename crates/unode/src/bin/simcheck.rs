//! Validate our pump.fun v3 transactions against mainnet using simulateTransaction
//! (sigVerify=false lets us use a real funded wallet as the payer without its key).
//! Also checks that our curve quote matches what the program actually does.

use anyhow::{anyhow, Result};
use clap::Parser;
use serde_json::json;
use ucore::curve::Curve;
use ucore::event::Ev;
use ucore::pk::Pk;
use ucore::pump::{self, RawEvent};
use ucore::rpcjson::{decode_block, parse_block_response};
use ucore::tx::{DecodeCfg, DecodeState, TIP_ACCOUNTS};
use ucore::txbuild::*;
use unode::rpc::{decode_bonding_curve, Rpc};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc: String,
    /// Lamports to spend in the simulated buy.
    #[arg(long, default_value_t = 20_000_000)]
    spend: u64,
}

fn trade_from_logs(v: &serde_json::Value) -> Option<pump::RawTrade> {
    let logs: Vec<String> = v["value"]["logs"].as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    for p in ucore::tx::log_payloads(&logs) {
        if let Some(RawEvent::Trade(t)) = pump::decode_event(&p) {
            return Some(t);
        }
    }
    None
}

fn show(v: &serde_json::Value) {
    println!("  err: {}", v["value"]["err"]);
    println!("  units consumed: {}", v["value"]["unitsConsumed"]);
    if let Some(l) = v["value"]["logs"].as_array() {
        for x in l.iter().filter_map(|x| x.as_str()) {
            if x.contains("Instruction:") || x.contains("failed") || x.contains("Error") || x.contains("error") {
                println!("    {x}");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = Args::parse();
    let rpc = Rpc::new(&a.rpc)?;
    let fin = rpc.call("getSlot", json!([{"commitment":"confirmed"}])).await?.as_u64().unwrap();
    // 1) find a fresh non-mayhem SOL-paired buy in recent blocks
    let cfg = DecodeCfg::default();
    let mut st = DecodeState::default();
    let mut cand = None;
    'outer: for slot in (fin - 40..fin).rev() {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"getBlock","params":[slot,{"encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"full","rewards":false,"commitment":"confirmed"}]});
        let bytes = rpc.http.post(&rpc.url).json(&body).send().await?.bytes().await?;
        let Ok(Some(b)) = parse_block_response(&bytes) else { continue };
        for e in decode_block(slot, &b, &cfg, &mut st) {
            if let Ev::Trade(t) = e {
                if t.is_buy && !t.mayhem && t.sol_quote && t.sol > 50_000_000 && t.rtok > 100_000_000_000_000 {
                    cand = Some(t);
                    break 'outer;
                }
            }
        }
    }
    let t = cand.ok_or_else(|| anyhow!("no candidate trade found"))?;
    let mint = t.mint;
    let buyer = t.user;
    let (_, token_program, _) = rpc.account_data(&mint).await?.ok_or_else(|| anyhow!("mint not found"))?;
    println!("mint {mint} token_program {token_program} buyer {buyer} (slot {})", t.m.slot);
    let accts = pump_accounts(&buyer, &mint, &token_program);
    let (bc_data, _, _) = rpc.account_data(&accts.bonding_curve).await?.ok_or_else(|| anyhow!("no bonding curve"))?;
    let curve: Curve = decode_bonding_curve(&bc_data).ok_or_else(|| anyhow!("bad curve"))?;
    println!("curve: vsol {} vtok {} rsol {} rtok {} mcap {:.2} SOL", curve.vsol, curve.vtok, curve.rsol, curve.rtok, curve.mcap_sol());

    let p = TxParams { cu_limit: 150_000, cu_price: 1_000_000, tip_lamports: 1_000_000, tip_account: Some(TIP_ACCOUNTS[8]) };
    // 2) buy
    let fee_bps = t.fee_bps;
    let (q_tok, q_net, q_fee) = curve.buy_quote_in(a.spend, fee_bps);
    let ixs = buy_ixs(&buyer, &mint, &token_program, a.spend, q_tok * 95 / 100, &p, 3);
    let (tx, _) = build_tx(&buyer, &ixs, &[0u8; 32], None);
    println!("\nBUY buy_exact_quote_in_v3 spend {} lamports, tx {} bytes", a.spend, tx.len());
    let r = rpc.simulate(&tx).await?;
    show(&r);
    if let Some(ev) = trade_from_logs(&r) {
        println!("  program: tokens {} sol_net {} fee {} cfee {} ix {}", ev.token_amount, ev.sol_amount, ev.fee, ev.creator_fee, ev.ix_name);
        println!("  ours:    tokens {q_tok} sol_net {q_net} fee_total {q_fee}");
        let d = (ev.token_amount as f64 / q_tok.max(1) as f64 - 1.0) * 1e4;
        println!("  token quote error: {d:.2} bps (state may move between our read and the simulation)");
    } else {
        println!("  no TradeEvent in simulation logs");
    }

    // 3) sell from a real holder
    let ata = Pk::ata(&buyer, &mint, &token_program);
    let held = rpc
        .account_data(&ata)
        .await?
        .and_then(|(d, _, _)| d.get(64..72).map(|b| u64::from_le_bytes(b.try_into().unwrap())));
    if let Some((holder, amt)) = held.filter(|a| *a > 0).map(|a| (buyer, a)) {
        let sell_amt = (amt / 100).max(1);
        let (q_out, _, _) = curve.sell(sell_amt, fee_bps);
        let ixs = sell_ixs(&holder, &mint, &token_program, sell_amt, q_out * 90 / 100, false, &p, 5);
        let (tx, _) = build_tx(&holder, &ixs, &[0u8; 32], None);
        println!("\nSELL sell_v3 {sell_amt} tokens from holder {holder}, tx {} bytes", tx.len());
        let r = rpc.simulate(&tx).await?;
        show(&r);
        if let Some(ev) = trade_from_logs(&r) {
            println!("  program: sol_out(net to curve) {} fee {} cfee {}", ev.sol_amount, ev.fee, ev.creator_fee);
            println!("  ours:    sol_to_user {q_out}");
        }
    }
    Ok(())
}
