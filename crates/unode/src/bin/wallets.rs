//! Follow the money over many days: per-wallet, per-UTC-day profit of every bonding-curve position
//! (SOL-quoted, non-mayhem coins whose creation is in the data), after curve fees, transaction fees
//! and tips. A position opens at a wallet's first buy of a coin and closes when its balance is back
//! to ~0; positions still open at the end are marked at the curve sell value of the last state.
//! Rows are attributed to the UTC day of the position's first buy.
//!
//! usage: wallets --out wd.csv <data dir> [<data dir> ...]   (dirs in chronological order)

use anyhow::Result;
use rustc_hash::{FxHashMap, FxHashSet};
use std::io::Write;
use ucore::event::Ev;
use ucore::pk::{Pk, Sig};
use ucore::store::StoreReader;

#[derive(Clone, Copy)]
struct MintInfo {
    create_slot: u64,
    dev: Pk,
    vsol: u64,
    vtok: u64,
    holder_reward: bool,
}

#[derive(Default, Clone)]
struct Pos {
    first_slot: u64,
    last_slot: u64,
    day: i64,
    spent: i64,
    recv: i64,
    costs: i64,
    bought: u64,
    tok: u64,
    n_buys: u32,
    n_sells: u32,
    entry_age: u64,
    entry_mcap: f64,
    entry_cu_price: u64,
    entry_tip: u64,
    b0: bool,
    dev: bool,
    hr: bool,
}

#[derive(Default, Clone)]
struct Day {
    n: u64,
    spent: f64,
    pnl: f64,
    wins: u64,
    hold_s: f64,
    age_s: f64,
    b0: u64,
    dev: u64,
    mcap: f64,
    cu_price: f64,
    tip: f64,
    costs: f64,
    open_at_end: u64,
    fast_entry: u64,
    fast_exit: u64,
    hr: u64,
}

const SLOT_S: f64 = 0.215;

fn curve_sell(vsol: u64, vtok: u64, tok: u64) -> f64 {
    if tok == 0 || vtok == 0 {
        return 0.0;
    }
    let (vs, vt, t) = (vsol as f64, vtok as f64, tok as f64);
    let out = vs - vs * vt / (vt + t);
    out.max(0.0) * (1.0 - 0.0125)
}

static mut TOT: (f64, f64, f64) = (0.0, 0.0, 0.0);

fn finish(p: &Pos, mark: f64, wallet: Pk, days: &mut FxHashMap<(Pk, i64), Day>, open: bool) {
    let pnl = (p.recv as f64 + mark - p.spent as f64 - p.costs as f64) / 1e9;
    unsafe {
        TOT.0 += p.spent as f64 / 1e9;
        TOT.1 += pnl;
        TOT.2 += mark / 1e9;
    }
    let d = days.entry((wallet, p.day)).or_default();
    d.n += 1;
    d.spent += p.spent as f64 / 1e9;
    d.pnl += pnl;
    d.wins += (pnl > 0.0) as u64;
    d.hold_s += p.last_slot.saturating_sub(p.first_slot) as f64 * SLOT_S;
    d.age_s += p.entry_age as f64 * SLOT_S;
    d.b0 += p.b0 as u64;
    d.dev += p.dev as u64;
    d.mcap += p.entry_mcap;
    d.cu_price += p.entry_cu_price as f64;
    d.tip += p.entry_tip as f64 / 1e9;
    d.costs += p.costs as f64 / 1e9;
    d.open_at_end += open as u64;
    d.fast_entry += (p.entry_age <= 2) as u64;
    d.fast_exit += (p.last_slot.saturating_sub(p.first_slot) <= 47) as u64;
    d.hr += p.hr as u64;
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = &args[2];
    let dirs = &args[3..];
    let mut mints: FxHashMap<Pk, MintInfo> = FxHashMap::default();
    let mut pos: FxHashMap<(Pk, Pk), Pos> = FxHashMap::default();
    let mut days: FxHashMap<(Pk, i64), Day> = FxHashMap::default();
    let mut charged: FxHashSet<Sig> = FxHashSet::default();
    let mut now_day = 0i64;
    let (mut n_tr, mut n_blk) = (0u64, 0u64);
    for d in dirs {
        for blk in StoreReader::open(d)? {
            n_blk += 1;
            charged.clear();
            for e in &blk {
                match e {
                    Ev::Create(c) => {
                        if ucore::pump::is_sol_quote(&c.quote_mint) && !c.mayhem && c.depth == 0 {
                            mints.insert(c.mint, MintInfo { create_slot: c.m.slot, dev: c.user, vsol: c.vsol, vtok: c.vtok, holder_reward: c.holder_reward });
                        }
                    }
                    Ev::Trade(t) => {
                        if !t.sol_quote || t.mayhem {
                            continue;
                        }
                        let Some(mi) = mints.get_mut(&t.mint) else { continue };
                        n_tr += 1;
                        mi.vsol = t.vsol;
                        mi.vtok = t.vtok;
                        let mi = *mi;
                        if t.m.block_time > 0 {
                            now_day = t.m.block_time.div_euclid(86_400);
                        }
                        let key = (t.user, t.mint);
                        if !t.is_buy && !pos.contains_key(&key) {
                            continue; // selling tokens received outside the curve: not a position we can price
                        }
                        let p = pos.entry(key).or_insert_with(|| Pos {
                            first_slot: t.m.slot,
                            day: now_day,
                            entry_age: t.m.slot.saturating_sub(mi.create_slot),
                            entry_mcap: t.vsol as f64 / t.vtok.max(1) as f64 * 1e6,
                            entry_cu_price: t.m.cu_price,
                            entry_tip: t.m.tip,
                            b0: t.m.slot == mi.create_slot,
                            dev: t.user == mi.dev,
                            hr: mi.holder_reward,
                            ..Default::default()
                        });
                        p.last_slot = t.m.slot;
                        if t.is_buy {
                            p.spent += (t.sol + t.fee) as i64;
                            p.bought += t.tok;
                            p.tok += t.tok;
                            p.n_buys += 1;
                        } else {
                            // only the tokens this position bought on the curve count (transfers in are ignored)
                            let frac = if t.tok == 0 { 0.0 } else { (p.tok as f64 / t.tok as f64).min(1.0) };
                            p.recv += ((t.sol as i64 - t.fee as i64) as f64 * frac) as i64;
                            p.tok = p.tok.saturating_sub(t.tok);
                            p.n_sells += 1;
                        }
                        if t.user == t.m.signer && charged.insert(t.m.sig) {
                            p.costs += (t.m.tx_fee + t.m.tip) as i64;
                        }
                        if !t.is_buy && p.tok <= p.bought / 1000 {
                            let p = pos.remove(&key).unwrap();
                            finish(&p, 0.0, t.user, &mut days, false);
                        }
                    }
                    _ => {}
                }
            }
        }
        eprintln!("{d}: blocks {n_blk} trades {n_tr} open positions {} wallet-days {}", pos.len(), days.len());
    }
    // conservative mark: all open holders of a coin sell together, proceeds shared pro rata
    let mut open_tok: FxHashMap<Pk, u64> = FxHashMap::default();
    for ((_, m), p) in pos.iter() {
        *open_tok.entry(*m).or_default() += p.tok;
    }
    for ((w, m), p) in pos.drain() {
        let tot = open_tok.get(&m).copied().unwrap_or(0);
        let mark = match mints.get(&m) {
            Some(mi) if tot > 0 => curve_sell(mi.vsol, mi.vtok, tot) * (p.tok as f64 / tot as f64),
            _ => 0.0,
        };
        finish(&p, mark, w, &mut days, true);
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
    writeln!(f, "wallet,day,n,spent,pnl,wins,hold_s,age_s,b0,dev,mcap,cu_price,tip,costs,open_end,fast_entry,fast_exit,hr")?;
    for ((w, day), d) in &days {
        if d.n < 3 {
            continue;
        }
        let n = d.n as f64;
        writeln!(
            f,
            "{},{},{},{:.4},{:.4},{},{:.1},{:.1},{},{},{:.1},{:.0},{:.6},{:.5},{},{},{},{}",
            w.to_b58(),
            day,
            d.n,
            d.spent,
            d.pnl,
            d.wins,
            d.hold_s / n,
            d.age_s / n,
            d.b0,
            d.dev,
            d.mcap / n,
            d.cu_price / n,
            d.tip / n,
            d.costs,
            d.open_at_end,
            d.fast_entry,
            d.fast_exit,
            d.hr
        )?;
    }
    let tot = unsafe { TOT };
    eprintln!("done: trades {n_tr}, wallet-days {}; all positions: spent {:.0} SOL, pnl {:.0} SOL (of which marked open {:.0})", days.len(), tot.0, tot.1, tot.2);
    Ok(())
}
