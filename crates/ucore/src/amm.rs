//! PumpSwap (pAMMBay6…) event decoding: pool creation (graduation), trades and boost buy-and-burn.
//! Layouts follow the official `pump_amm` IDL (self-CPI Anchor events).

use crate::pk::{pk, Pk};
use crate::pump::Rd;
use serde::{Deserialize, Serialize};

pub const PUMP_AMM: Pk = pk("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");

pub const DISC_BUY: [u8; 8] = [103, 244, 82, 31, 44, 245, 119, 119];
pub const DISC_SELL: [u8; 8] = [62, 47, 55, 10, 165, 3, 220, 42];
pub const DISC_CREATE_POOL: [u8; 8] = [177, 49, 12, 210, 160, 118, 167, 116];
pub const DISC_BOOST_BURN: [u8; 8] = [63, 69, 28, 22, 48, 92, 194, 185];
pub const DISC_INIT_BOOST: [u8; 8] = [174, 124, 74, 249, 4, 81, 246, 17];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolKind {
    Create,
    Buy,
    Sell,
    BoostBurn,
    InitBoost,
}

/// One decoded PumpSwap event. Amounts are in base / quote base units.
/// `base_res` / `quote_res` are the pool reserves the event reports (before the trade for buy/sell,
/// after for create / boost); `vq` is the signed virtual quote reserve adjustment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawPool {
    pub kind: PoolKind,
    pub pool: Pk,
    pub mint: Pk,
    pub quote_mint: Pk,
    pub user: Pk,
    pub base: u64,
    pub quote: u64,
    pub base_res: u64,
    pub quote_res: u64,
    pub vq: i64,
    pub fee_bps: u32,
    pub fee: u64,
    pub creator: Pk,
    pub extra: u64,
}

fn i128le(r: &mut Rd) -> Option<i128> {
    let lo = r.u64()? as u128;
    let hi = r.u64()? as u128;
    Some((lo | (hi << 64)) as i128)
}

pub fn decode_event(payload: &[u8]) -> Option<RawPool> {
    if payload.len() < 8 {
        return None;
    }
    let disc: [u8; 8] = payload[..8].try_into().ok()?;
    let mut r = Rd::new(&payload[8..]);
    match disc {
        DISC_BUY | DISC_SELL => {
            let buy = disc == DISC_BUY;
            let _ts = r.i64()?;
            let base = r.u64()?; // base_amount_out / base_amount_in
            let _lim = r.u64()?;
            let _ub = r.u64()?;
            let _uq = r.u64()?;
            let base_res = r.u64()?;
            let quote_res = r.u64()?;
            let _q = r.u64()?; // quote_amount_in / quote_amount_out
            let lp_bps = r.u64()?;
            let lp_fee = r.u64()?;
            let pr_bps = r.u64()?;
            let pr_fee = r.u64()?;
            let _q2 = r.u64()?;
            let user_quote = r.u64()?; // what the user paid / received, all fees included
            let pool = r.pk()?;
            let user = r.pk()?;
            r.skip(32 * 4)?;
            let creator = r.pk()?;
            let cr_bps = r.u64()?;
            let cr_fee = r.u64()?;
            if buy {
                // track_volume, total_unclaimed, total_claimed, current_sol_volume, last_update_ts, min_base_out, ix_name
                r.skip(1 + 8 * 5)?;
                let _ix = r.string()?;
            }
            let cb_bps = r.u64().unwrap_or(0);
            let cb = r.u64().unwrap_or(0);
            // buyback fee is a share (bps of the protocol fee) paid out of the protocol fee: not extra
            let _bb_share_bps = r.u64().unwrap_or(0);
            let _bb = r.u64().unwrap_or(0);
            let vq = i128le(&mut r).unwrap_or(0);
            let can_boost = r.bool().unwrap_or(false);
            let base_supply = r.u64().unwrap_or(0);
            Some(RawPool {
                kind: if buy { PoolKind::Buy } else { PoolKind::Sell },
                pool,
                mint: Pk::default(),
                quote_mint: Pk::default(),
                user,
                base,
                quote: user_quote,
                base_res,
                quote_res,
                vq: vq.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
                fee_bps: (lp_bps + pr_bps + cr_bps + cb_bps) as u32,
                fee: lp_fee + pr_fee + cr_fee + cb,
                creator,
                // base supply (burns show up here) with the boost flag in the top bit
                extra: base_supply | ((can_boost as u64) << 63),
            })
        }
        DISC_CREATE_POOL => {
            let _ts = r.i64()?;
            let _index = r.u16()?;
            let creator = r.pk()?;
            let mint = r.pk()?;
            let quote_mint = r.pk()?;
            r.skip(2)?;
            let base_in = r.u64()?;
            let quote_in = r.u64()?;
            let pool_base = r.u64()?;
            let pool_quote = r.u64()?;
            r.skip(8 * 3 + 1)?;
            let pool = r.pk()?;
            r.skip(32 * 3)?;
            let coin_creator = r.pk()?;
            let mayhem = r.bool().unwrap_or(false);
            Some(RawPool {
                kind: PoolKind::Create,
                pool,
                mint,
                quote_mint,
                user: creator,
                base: base_in,
                quote: quote_in,
                base_res: pool_base,
                quote_res: pool_quote,
                vq: 0,
                fee_bps: 0,
                fee: 0,
                creator: coin_creator,
                extra: mayhem as u64,
            })
        }
        DISC_BOOST_BURN => {
            let _ts = r.i64()?;
            let mint = r.pk()?;
            let _bc = r.pk()?;
            let pool = r.pk()?;
            let authority = r.pk()?;
            let _req = r.u64()?;
            let used = r.u64()?;
            let burned = r.u64()?;
            let vq = i128le(&mut r)?;
            let rq_after = r.u64()?;
            let base_after = r.u64()?;
            let remaining = r.u64()?;
            Some(RawPool {
                kind: PoolKind::BoostBurn,
                pool,
                mint,
                quote_mint: Pk::default(),
                user: authority,
                base: burned,
                quote: used,
                base_res: base_after,
                quote_res: rq_after,
                vq: vq.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
                fee_bps: 0,
                fee: 0,
                creator: Pk::default(),
                extra: remaining,
            })
        }
        DISC_INIT_BOOST => {
            let _ts = r.i64()?;
            let mint = r.pk()?;
            let _bc = r.pk()?;
            let pool = r.pk()?;
            let vq = i128le(&mut r)?;
            let rq_after = r.u64()?;
            Some(RawPool {
                kind: PoolKind::InitBoost,
                pool,
                mint,
                quote_mint: Pk::default(),
                user: Pk::default(),
                base: 0,
                quote: 0,
                base_res: 0,
                quote_res: rq_after,
                vq: vq.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
                fee_bps: 0,
                fee: 0,
                creator: Pk::default(),
                extra: 0,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
    /// Real mainnet events (slot 464219300).
    #[test]
    fn decodes_mainnet_buy_and_sell() {
        let b = decode_event(&hex("67f4521f2cf57777e49ae76a000000009da529f2000000003debfd1400000000a180e4601a0100003debfd1400000000f08fb0cd650d00007914cfda15010000a0a0e913000000001400000000000000fd310a00000000000500000000000000808c0200000000009dd2f313000000009b04fe13000000008f7f4db478a11fe7dbc3744e3737e73abe9ff73028f5cc8644bf30f5921b5e7e634693ddd699caa4b383243dbb252565ae546d0b468ad03ac05cd338e91a88abb0192964025b64a1ee31fb2e15872c0a6850d693649f9b1c457df96ecd3d9d787ae5e254eb77bf69be27f89cf6f6747b1a7b5d8bf59be8d90a27e8f9bc4ea70a4ac2f8d0dd5cbc97e3289c197cb5062a54f3d956b9ce6e5115f96567aa5cb3e677d915955f8880731ceb4a75a0cc96c174fa4095c4e1d9967acfc42845ae67ae46283194e4fe4cd82571f06d01ba7410ce720c52847f3b76a05fd700454e1b6a0f000000000000007ea50700000000000100000000000000000000000000000000000000000000000000000000000000009da529f200000000030000006275790000000000000000000000000000000088130000000000004046010000000000e68f191804000000000000000000000001473e760d158d030000000000000000000000000000000000e2b10400000000000000000000000000")).expect("buy");
        assert_eq!(b.kind, PoolKind::Buy);
        assert_eq!(b.base, 4062815645);
        assert_eq!(b.quote, 335414427);
        assert_eq!(b.base_res, 14730893758448);
        assert_eq!(b.quote_res, 1193376945273);
        let s = decode_event(&hex("3e2f370aa503dc2ae49ae76a00000000404b4c0000000000510b507901000000404b4c00000000000000000000000000bb5ee1cc13000000672b277d63c400003c8aacf40200000019000000000000007845e401000000000500000000000000b2da600000000000c444c8f202000000126a67f202000000a66c7fadf0673ea6c8f0966f7259c86748fa0b1be867379fa62b6874c8c5697c8cff3c11a61d80744295746380b518b6ced7e25854fec648bdb4fd1837c8d3a4d209cafba029691a4099490871e1281e25ed59a8e273cbef4eb1a40c14a8c76d77ea85f1a756c10a50d26bd777eab791bf9ed96f43e23fe5c2c222dfeb9cc8f24ac2f8d0dd5cbc97e3289c197cb5062a54f3d956b9ce6e5115f96567aa5cb3e6f661261b76a0718fe1c87dbb650ea6ba12debab40788b840b6375c5886c48045000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008813000000000000596d3000000000008835ffffffffffffffffffffffffffff0000000000000000000000000000000000000000000000000000000000000000000000000000000000")).expect("sell");
        assert_eq!(s.kind, PoolKind::Sell);
        assert!(s.base > 0 && s.quote > 0);
        assert!(s.fee_bps > 0 && s.fee_bps < 500, "sell fee bps {}", s.fee_bps);
        assert!(b.fee_bps > 0 && b.fee_bps < 500, "buy fee bps {}", b.fee_bps);
    }
}
