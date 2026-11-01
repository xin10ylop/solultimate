//! Identity-free order-flow features. Nothing here depends on *who* trades (no wallet
//! lists, no operators): only on the shape of the flow, which every launch exhibits.

use crate::mint::MintTracker;
use crate::pump::{secs, SLOT_MS, TOKEN_SUPPLY};

pub const FEATURE_NAMES: &[&str] = &[
    "age_s",
    "mcap_sol",
    "progress",
    "ret_since_create",
    "dd_from_max",
    "ret_2s",
    "ret_5s",
    "ret_15s",
    "ret_60s",
    "net_sol_2s",
    "net_sol_5s",
    "net_sol_15s",
    "net_sol_60s",
    "buys_5s",
    "sells_5s",
    "buys_15s",
    "sells_15s",
    "uniq_buyers_15s",
    "uniq_buyers_60s",
    "max_buy_15s",
    "avg_buy_15s",
    "flip_sell_frac_15s",
    "idle_s",
    "top1_share",
    "top5_share",
    "insider_share",
    "dev_left_frac",
    "block0_sol",
    "dev_buy_sol",
    "n_trades",
    "buy_frac_all",
];

fn price_at_or_before(mt: &MintTracker, slot: u64) -> f64 {
    let mut p = mt.init_price;
    for t in mt.recent.iter() {
        if t.slot > slot {
            break;
        }
        p = t.price;
    }
    // if the window starts before our buffer, the earliest buffered price is the best we have
    if mt.recent.front().map(|f| f.slot > slot).unwrap_or(false) && mt.age_slots(slot) > secs(60) {
        p = mt.recent.front().unwrap().price;
    }
    p
}

/// Feature vector at `slot` (call right after applying the latest event).
pub fn features(mt: &MintTracker, slot: u64) -> Vec<f32> {
    let price = mt.curve.price();
    let lr = |p0: f64| if p0 > 0.0 && price > 0.0 { (price / p0).ln() } else { 0.0 };
    let win = |w: u64| {
        let from = slot.saturating_sub(secs(w));
        let (mut net, mut nb, mut ns, mut flip_sol, mut sell_sol, mut maxb, mut sumb) = (0i64, 0u32, 0u32, 0u64, 0u64, 0u64, 0u64);
        let mut uniq: Vec<[u8; 4]> = Vec::new();
        for t in mt.recent.iter().rev() {
            if t.slot < from {
                break;
            }
            if t.is_buy {
                net += t.sol as i64;
                nb += 1;
                maxb = maxb.max(t.sol);
                sumb += t.sol;
                let k: [u8; 4] = t.user.0[..4].try_into().unwrap();
                if !uniq.contains(&k) {
                    uniq.push(k);
                }
            } else {
                net -= t.sol as i64;
                ns += 1;
                sell_sol += t.sol;
                if t.flip {
                    flip_sol += t.sol;
                }
            }
        }
        (net as f64 / 1e9, nb, ns, uniq.len() as f64, maxb as f64 / 1e9, if nb > 0 { sumb as f64 / nb as f64 / 1e9 } else { 0.0 }, if sell_sol > 0 { flip_sol as f64 / sell_sol as f64 } else { 0.0 })
    };
    let w2 = win(2);
    let w5 = win(5);
    let w15 = win(15);
    let w60 = win(60);
    // holder concentration (wallets only; the curve's own account is not in `holdings`)
    let mut hs: Vec<i128> = mt.holdings.values().copied().filter(|h| *h > 0).collect();
    hs.sort_unstable_by(|a, b| b.cmp(a));
    let sup = TOKEN_SUPPLY as f64;
    let top1 = hs.first().copied().unwrap_or(0) as f64 / sup;
    let top5 = hs.iter().take(5).sum::<i128>() as f64 / sup;
    let dev_now = mt.holdings.get(&mt.dev).copied().unwrap_or(0).max(0);
    let dev_left = if mt.dev_max_hold > 0 { dev_now as f64 / mt.dev_max_hold as f64 } else { 0.0 };
    vec![
        (mt.age_slots(slot) * SLOT_MS) as f32 / 1000.0,
        mt.curve.mcap_sol() as f32,
        mt.curve.progress() as f32,
        lr(mt.init_price) as f32,
        (price / mt.max_price.max(1e-30) - 1.0) as f32,
        lr(price_at_or_before(mt, slot.saturating_sub(secs(2)))) as f32,
        lr(price_at_or_before(mt, slot.saturating_sub(secs(5)))) as f32,
        lr(price_at_or_before(mt, slot.saturating_sub(secs(15)))) as f32,
        lr(price_at_or_before(mt, slot.saturating_sub(secs(60)))) as f32,
        w2.0 as f32,
        w5.0 as f32,
        w15.0 as f32,
        w60.0 as f32,
        w5.1 as f32,
        w5.2 as f32,
        w15.1 as f32,
        w15.2 as f32,
        w15.3 as f32,
        w60.3 as f32,
        w15.4 as f32,
        w15.5 as f32,
        w15.6 as f32,
        (slot.saturating_sub(mt.last_trade_slot) * SLOT_MS) as f32 / 1000.0,
        top1 as f32,
        top5 as f32,
        mt.insider_share() as f32,
        dev_left as f32,
        (mt.block0.values().sum::<u64>() as f64 / 1e9) as f32,
        (mt.dev_buy_sol as f64 / 1e9) as f32,
        mt.n_trades as f32,
        if mt.n_trades > 0 { mt.n_buys as f32 / mt.n_trades as f32 } else { 0.0 },
    ]
}
