//! Normalized, storable events. Everything downstream (recorder, lab, simulator, live brain)
//! consumes exactly this stream, ordered by (slot, tx_idx, ev_idx).

use crate::pk::{Pk, Sig};
use serde::{Deserialize, Serialize};

/// Per-transaction metadata attached to each event.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Meta {
    pub slot: u64,
    /// Index of the transaction inside its block (execution order).
    pub tx_idx: u32,
    /// Index of the event inside its transaction.
    pub ev_idx: u16,
    pub block_time: i64,
    /// Local receive time in microseconds since epoch (0 when backfilled from RPC).
    pub recv_us: u64,
    pub sig: Sig,
    /// Fee payer / first signer.
    pub signer: Pk,
    /// Compute-unit price (micro-lamports) and limit from ComputeBudget instructions.
    pub cu_price: u64,
    pub cu_limit: u32,
    /// Lamports sent to known tip accounts (Jito, Nozomi, ...).
    pub tip: u64,
    pub tx_fee: u64,
    /// Hash of the ordered top-level program list (tx "shape" fingerprint).
    pub shape: u32,
}

impl Meta {
    pub fn order_key(&self) -> (u64, u32, u16) {
        (self.slot, self.tx_idx, self.ev_idx)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Create {
    pub m: Meta,
    pub mint: Pk,
    pub bonding_curve: Pk,
    /// Signer that created the coin (the "dev" wallet).
    pub user: Pk,
    /// Creator-fee recipient.
    pub creator: Pk,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub token_program: Pk,
    pub quote_mint: Pk,
    pub mayhem: bool,
    pub cashback: bool,
    pub holder_reward: bool,
    pub depth: u8,
    pub vsol: u64,
    pub vtok: u64,
    pub rtok: u64,
    pub supply: u64,
    pub creator_fee_bps: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    pub m: Meta,
    pub mint: Pk,
    pub user: Pk,
    pub is_buy: bool,
    /// Net quote amount that moved into / out of the curve (excludes fees).
    pub sol: u64,
    pub tok: u64,
    /// Protocol + creator (+cashback) fee charged, lamports.
    pub fee: u64,
    pub fee_bps: u32,
    /// Curve state *after* the trade.
    pub vsol: u64,
    pub vtok: u64,
    pub rsol: u64,
    pub rtok: u64,
    pub ix: u8,
    pub mayhem: bool,
    pub sol_quote: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Complete {
    pub m: Meta,
    pub mint: Pk,
    pub user: Pk,
}

/// A native SOL transfer: either into a previously empty account (fresh-wallet funding)
/// or touching a watched wallet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fund {
    pub m: Meta,
    pub from: Pk,
    pub to: Pk,
    pub lamports: u64,
    /// Recipient balance before the tx (0 = fresh wallet).
    pub to_pre: u64,
    /// Sender balance after the tx (0 = sender emptied, "last transfer" pattern).
    pub from_post: u64,
}

/// Token movement of a tracked (young pump) mint outside of curve trades.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokXfer {
    pub m: Meta,
    pub mint: Pk,
    pub from: Pk,
    pub to: Pk,
    pub amount: u64,
}

/// One per block: boundaries and competition stats.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BlockStat {
    pub slot: u64,
    pub parent_slot: u64,
    pub block_time: i64,
    pub n_tx: u32,
    pub pump_ok: u32,
    pub pump_failed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Ev {
    Create(Create),
    Trade(Trade),
    Complete(Complete),
    Fund(Fund),
    TokXfer(TokXfer),
    Block(BlockStat),
    /// PumpSwap event (appended last: older stores stay readable).
    Pool(PoolEv),
}

/// PumpSwap event with tx meta. `mint` is resolved from the instruction accounts or the
/// pool-creation event (default when unknown, e.g. multi-hop into a pool never seen).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolEv {
    pub m: Meta,
    pub e: crate::amm::RawPool,
}

impl Ev {
    pub fn slot(&self) -> u64 {
        match self {
            Ev::Create(e) => e.m.slot,
            Ev::Trade(e) => e.m.slot,
            Ev::Complete(e) => e.m.slot,
            Ev::Fund(e) => e.m.slot,
            Ev::TokXfer(e) => e.m.slot,
            Ev::Block(b) => b.slot,
            Ev::Pool(p) => p.m.slot,
        }
    }
    pub fn meta(&self) -> Option<&Meta> {
        match self {
            Ev::Create(e) => Some(&e.m),
            Ev::Trade(e) => Some(&e.m),
            Ev::Complete(e) => Some(&e.m),
            Ev::Fund(e) => Some(&e.m),
            Ev::TokXfer(e) => Some(&e.m),
            Ev::Block(_) => None,
            Ev::Pool(p) => Some(&p.m),
        }
    }
    pub fn mint(&self) -> Option<Pk> {
        match self {
            Ev::Create(e) => Some(e.mint),
            Ev::Trade(e) => Some(e.mint),
            Ev::Complete(e) => Some(e.mint),
            Ev::TokXfer(e) => Some(e.mint),
            _ => None,
        }
    }
    /// Block events sort after every event of their slot.
    pub fn order_key(&self) -> (u64, u32, u16) {
        match self.meta() {
            Some(m) => m.order_key(),
            None => (self.slot(), u32::MAX, u16::MAX),
        }
    }
}
