//! Meteora Dynamic Bonding Curve (DBC) events: launches and swaps of the launchpads built on it
//! (pools carry the partner's config, including an optional decaying anti-sniper fee).
//!
//! Layouts from the official IDL (dynamic-bonding-curve-sdk). Events arrive as Anchor self-CPI
//! inner instructions (`EVENT_IX_TAG` + 8-byte event discriminator + Borsh payload).

use crate::pk::{pk, Pk};
use serde::{Deserialize, Serialize};

pub const DBC_PROGRAM: Pk = pk("dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN");

const EVT_SWAP: [u8; 8] = [0x1b, 0x3c, 0x15, 0xd5, 0x8a, 0xaa, 0xbb, 0x93];
const EVT_SWAP2: [u8; 8] = [0xbd, 0x42, 0x33, 0xa8, 0x26, 0x50, 0x75, 0x99];
const EVT_SWAP3: [u8; 8] = [0xbb, 0x77, 0xf8, 0x0d, 0xaf, 0xff, 0x40, 0x3f];
const EVT_SWAP2_TH: [u8; 8] = [0x86, 0x3b, 0xa8, 0x78, 0x5e, 0x33, 0x72, 0xe7];
const EVT_SWAP3_TH: [u8; 8] = [0x9c, 0x58, 0xbc, 0x31, 0xe3, 0x92, 0xa5, 0x5d];
const EVT_INIT: [u8; 8] = [0xe4, 0x32, 0xf6, 0x55, 0xcb, 0x42, 0x86, 0x25];
const EVT_INIT_TH: [u8; 8] = [0xd5, 0x89, 0xa4, 0x35, 0xc1, 0x4a, 0x0f, 0x6e];
const EVT_COMPLETE: [u8; 8] = [0xe5, 0xe7, 0x56, 0x54, 0x9c, 0x86, 0x4b, 0x18];

/// Instruction discriminators (to learn pool -> base / quote mint from the accounts).
pub const IX_SWAP: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
pub const IX_SWAP2: [u8; 8] = [65, 75, 63, 76, 235, 91, 91, 136];
pub const IX_INIT_SPL: [u8; 8] = [140, 85, 215, 176, 102, 54, 104, 79];
pub const IX_INIT_2022: [u8; 8] = [169, 118, 51, 78, 145, 110, 220, 155];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DbcKind {
    #[default]
    Swap,
    Init,
    Complete,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RawDbc {
    pub kind: DbcKind,
    pub pool: Pk,
    pub config: Pk,
    /// Swaps: 1 = quote -> base (buy), 0 = base -> quote (sell). Init: pool_type.
    pub dir: u8,
    /// Gross amount that left the trader (fee included), in the input token's units.
    pub amount_in: u64,
    /// Amount received by the trader, in the output token's units.
    pub amount_out: u64,
    /// trading + protocol + referral fee, in the fee token's units.
    pub fee: u64,
    /// The fee was taken in the base token (else in quote).
    pub fee_on_base: bool,
    /// Quote reserve after the swap (0 when the event does not carry it). Complete: quote reserve.
    pub quote_reserve: u64,
    pub sqrt_price: u128,
    /// Program clock timestamp of the swap (unix seconds). Init: activation point.
    pub ts: u64,
    /// Init: creator; swaps: trader (fee payer of the transaction, filled by the decoder).
    pub user: Pk,
    pub base_mint: Pk,
    pub quote_mint: Pk,
    /// Event version (1 = EvtSwap, 2 = EvtSwap2, 3 = EvtSwap3). One swap emits several versions;
    /// the decoder keeps only the newest.
    #[serde(default)]
    pub ver: u8,
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u64(&mut self) -> Option<u64> {
        let v = u64::from_le_bytes(self.b.get(self.p..self.p + 8)?.try_into().ok()?);
        self.p += 8;
        Some(v)
    }
    fn u128(&mut self) -> Option<u128> {
        let v = u128::from_le_bytes(self.b.get(self.p..self.p + 16)?.try_into().ok()?);
        self.p += 16;
        Some(v)
    }
    fn pk(&mut self) -> Option<Pk> {
        let v = Pk::from_slice(self.b.get(self.p..self.p + 32)?)?;
        self.p += 32;
        Some(v)
    }
}

/// Decode one event payload (after the 8-byte EVENT_IX_TAG).
pub fn decode_event(d: &[u8]) -> Option<RawDbc> {
    if d.len() < 8 {
        return None;
    }
    let disc: [u8; 8] = d[..8].try_into().ok()?;
    let mut r = R { b: &d[8..], p: 0 };
    let mut e = RawDbc::default();
    match disc {
        EVT_INIT | EVT_INIT_TH => {
            e.kind = DbcKind::Init;
            e.pool = r.pk()?;
            e.config = r.pk()?;
            e.user = r.pk()?;
            e.base_mint = r.pk()?;
            e.dir = r.u8()?;
            e.ts = r.u64()?;
        }
        EVT_COMPLETE => {
            e.kind = DbcKind::Complete;
            e.pool = r.pk()?;
            e.config = r.pk()?;
            let _base_reserve = r.u64()?;
            e.quote_reserve = r.u64()?;
        }
        EVT_SWAP => {
            e.ver = 1;
            e.pool = r.pk()?;
            e.config = r.pk()?;
            e.dir = r.u8()?;
            let _ref = r.u8()?;
            let _amount_in = r.u64()?;
            let _min_out = r.u64()?;
            e.amount_in = r.u64()?; // actual_input_amount
            e.amount_out = r.u64()?;
            e.sqrt_price = r.u128()?;
            e.fee = r.u64()?.saturating_add(r.u64()?).saturating_add(r.u64()?);
            let _amount_in2 = r.u64()?;
            e.ts = r.u64()?;
            // v1 carries no fee-token flag: buys usually pay the fee in the base output
            e.fee_on_base = e.dir == 1 && e.fee as f64 > 0.5 * e.amount_in as f64;
        }
        EVT_SWAP2 | EVT_SWAP2_TH => {
            e.ver = 2;
            e.pool = r.pk()?;
            e.config = r.pk()?;
            e.dir = r.u8()?;
            let _ref = r.u8()?;
            let _a0 = r.u64()?;
            let _a1 = r.u64()?;
            let _mode = r.u8()?;
            let incl = r.u64()?;
            let excl = r.u64()?;
            let _left = r.u64()?;
            e.amount_in = incl;
            e.amount_out = r.u64()?;
            e.sqrt_price = r.u128()?;
            e.fee = r.u64()?.saturating_add(r.u64()?).saturating_add(r.u64()?);
            e.quote_reserve = r.u64()?;
            let _threshold = r.u64()?;
            e.ts = r.u64()?;
            // the fee was taken from the input when included > excluded
            e.fee_on_base = (incl > excl) == (e.dir == 0);
        }
        EVT_SWAP3 | EVT_SWAP3_TH => {
            e.ver = 3;
            e.pool = r.pk()?;
            e.config = r.pk()?;
            e.dir = r.u8()?;
            let _mode = r.u8()?;
            let _ref = r.u8()?;
            e.fee_on_base = r.u8()? != 0;
            e.amount_in = r.u64()?; // included_transfer_fee_amount_in
            let _in_excl = r.u64()?;
            let _out_incl = r.u64()?;
            e.amount_out = r.u64()?; // excluded_transfer_fee_amount_out (what the trader receives)
            e.fee = r.u64()?.saturating_add(r.u64()?).saturating_add(r.u64()?);
            e.sqrt_price = r.u128()?;
            e.quote_reserve = r.u64()?;
            e.ts = r.u64()?;
        }
        _ => return None,
    }
    Some(e)
}

/// Effective fee rate of a swap as a fraction of the quote amount traded.
pub fn fee_rate(e: &RawDbc) -> f64 {
    if e.kind != DbcKind::Swap {
        return 0.0;
    }
    // quote side gross: buys pay quote in, sells receive quote out (+ the fee if charged in quote)
    let (quote_gross, fee_in_quote) = if e.dir == 1 { (e.amount_in, !e.fee_on_base) } else { (e.amount_out + if e.fee_on_base { 0 } else { e.fee }, !e.fee_on_base) };
    if fee_in_quote {
        e.fee as f64 / quote_gross.max(1) as f64
    } else {
        // fee in base: relative to the base amount traded
        let base_gross = if e.dir == 1 { e.amount_out + e.fee } else { e.amount_in };
        e.fee as f64 / base_gross.max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swap_v1_layout() {
        // pool, config, dir 0, ref 0, params (2 u64), result (in, out, sqrt u128, 3 fees), amount_in, ts
        let mut b = EVT_SWAP.to_vec();
        b.extend_from_slice(&[1u8; 32]);
        b.extend_from_slice(&[2u8; 32]);
        b.push(0);
        b.push(0);
        for v in [205471966394u64, 100060996, 205471966394, 101723495] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&410534597164857407u128.to_le_bytes());
        for v in [203958u64, 50989, 0, 205471966394, 1793631318] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let e = decode_event(&b).unwrap();
        assert_eq!(e.kind, DbcKind::Swap);
        assert_eq!(e.amount_out, 101723495);
        assert_eq!(e.fee, 254947);
        assert_eq!(e.ts, 1793631318);
        let r = fee_rate(&e);
        assert!((r - 254947.0 / (101723495.0 + 254947.0)).abs() < 1e-9);
    }
}
