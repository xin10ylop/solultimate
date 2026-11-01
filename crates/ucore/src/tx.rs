//! Source-agnostic transaction view + decoder into `Ev`s.
//!
//! Both the Yellowstone gRPC recorder and the RPC `getBlock` backfill convert their native
//! format into `TxView`, so live and historical data are decoded by the same code.

use crate::event::*;
use crate::pk::{pk, Pk, Sig};
use crate::pump::{self, RawEvent};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone, Debug, Default)]
pub struct Ix {
    pub program: u16,
    pub accounts: Vec<u16>,
    /// Raw data. Sources may leave this empty for programs the decoder never inspects.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct TokBal {
    pub account_index: u16,
    pub mint: Pk,
    pub owner: Pk,
    pub amount: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TxView {
    pub slot: u64,
    pub tx_idx: u32,
    pub block_time: i64,
    pub recv_us: u64,
    pub sig: Sig,
    /// Static keys followed by loaded writable then loaded readonly keys.
    pub keys: Vec<Pk>,
    pub ixs: Vec<Ix>,
    /// (top-level instruction index, inner instructions in execution order)
    pub inner: Vec<(u16, Vec<Ix>)>,
    pub logs: Vec<String>,
    pub failed: bool,
    pub fee: u64,
    pub pre_bal: Vec<u64>,
    pub post_bal: Vec<u64>,
    pub pre_tok: Vec<TokBal>,
    pub post_tok: Vec<TokBal>,
}

impl TxView {
    pub fn key(&self, i: u16) -> Pk {
        self.keys.get(i as usize).copied().unwrap_or_default()
    }
    pub fn touches(&self, p: &Pk) -> bool {
        self.keys.iter().any(|k| k == p)
    }
}

/// Known tip destinations. Verify against your providers' docs; unknown tips are simply
/// not counted (they only feed fingerprints, never trading decisions).
pub const TIP_ACCOUNTS: &[Pk] = &[
    // Jito
    pk("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"),
    pk("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"),
    pk("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"),
    pk("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"),
    pk("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh"),
    pk("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt"),
    pk("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL"),
    pk("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
    // Nozomi (Temporal)
    pk("TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq"),
    pk("noz3jAjPiHuBPqiSPkkugaJDkJscPuRhYnSpbi8UvC4"),
    pk("noz3str9KXfpKknefHji8L1mPgimezaiUyCHYMDv1GE"),
    pk("noz6uoYCDijhu1V7cutCpwxNiSovEwLdRHPwmgCGDNo"),
    pk("noz9EPNcT7WH6Sou3sr3GGjHQYVkN3DNTXiEgUDqG7f"),
    pk("nozc5yT15LazbLTFVZzoNZCwjh3yUtW86LoUyqsBu4L"),
    pk("nozFrhfnNGoyqwVuwPAW4aaGqempx4PU6g6D9CJMv7Z"),
    pk("nozievPk7HyK1Rqy1MPJwVQ7qQg2QoJGyP71oeDwbsu"),
    pk("noznbgwYnBLDHu8wcQVCEw6kDrXkPdKkydGJGNXGvL7"),
    pk("nozNVWs5N8mgzuD3qigrCG2UoKxZttxzZ85pvAQVrbP"),
    pk("nozpEGbwx4BcGp6pvEdAh1JoC2CQGZdU6HbNP1v2p6P"),
    pk("nozrhjhkCr3zXT3BiT4WCodYCUFeQvcdUkM7MqhKqge"),
    pk("nozrwQtWhEdrA6W8dkbt9gnUaMs52PdAv5byipnadq3"),
    pk("nozUacTVWub3cL4mJmGCYjKZTnE9RbdY5AP46iQgbPJ"),
    pk("nozWCyTPppJjRuw2fpzDhhWbW355fzosWSzrrMYB1Qk"),
    pk("nozWNju6dY353eMkMqURqwQEoM3SFgEKC6psLCSfUne"),
    pk("nozxNBgWohjR75vdspfxR5H9ceC7XXH99xpxhVGt3Bb"),
];

pub struct DecodeCfg {
    /// Minimum lamports for a fresh-wallet funding to be recorded.
    pub fund_min_lamports: u64,
    /// Maximum lamports for a fresh-wallet funding (filters exchange internal moves).
    pub fund_max_lamports: u64,
    /// Wallets whose every SOL transfer is recorded (treasuries, exchange hot wallets...).
    pub watch: FxHashSet<Pk>,
    /// How long (slots) a created mint stays "active" for token-transfer tracking.
    pub active_mint_slots: u64,
}

impl Default for DecodeCfg {
    fn default() -> Self {
        DecodeCfg {
            fund_min_lamports: 5_000_000,           // 0.005 SOL
            fund_max_lamports: 2_000_000_000_000,   // 2000 SOL
            watch: FxHashSet::default(),
            active_mint_slots: crate::pump::secs(3600),
        }
    }
}

#[derive(Default)]
pub struct DecodeState {
    pub active_mints: FxHashMap<Pk, u64>,
    last_prune: u64,
}

impl DecodeState {
    fn prune(&mut self, slot: u64, keep: u64) {
        if slot.saturating_sub(self.last_prune) < 750 {
            return;
        }
        self.last_prune = slot;
        self.active_mints.retain(|_, s| slot.saturating_sub(*s) <= keep);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxClass {
    Other,
    PumpOk,
    PumpFailed,
}

fn shape_hash(tx: &TxView) -> u32 {
    // FNV-1a over the first 4 bytes of each top-level program id, skipping ComputeBudget.
    let mut h: u32 = 0x811c9dc5;
    for ix in &tx.ixs {
        let p = tx.key(ix.program);
        if p == pump::COMPUTE_BUDGET {
            continue;
        }
        for b in &p.0[..4] {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    h
}

fn system_transfer(tx: &TxView, ix: &Ix) -> Option<(Pk, Pk, u64)> {
    if tx.key(ix.program) != pump::SYSTEM_PROGRAM || ix.data.len() < 12 || ix.accounts.len() < 2 {
        return None;
    }
    let tag = u32::from_le_bytes(ix.data[..4].try_into().ok()?);
    if tag != 2 {
        return None;
    }
    let lamports = u64::from_le_bytes(ix.data[4..12].try_into().ok()?);
    Some((tx.key(ix.accounts[0]), tx.key(ix.accounts[1]), lamports))
}

/// Decode one transaction into events (appended to `out`).
pub fn decode_tx(tx: &TxView, cfg: &DecodeCfg, st: &mut DecodeState, out: &mut Vec<Ev>) -> TxClass {
    let pump_touched = tx.touches(&pump::PUMP_PROGRAM);
    if tx.failed {
        return if pump_touched { TxClass::PumpFailed } else { TxClass::Other };
    }
    st.prune(tx.slot, cfg.active_mint_slots);

    // ---- tx-level meta
    let mut cu_price = 0u64;
    let mut cu_limit = 0u32;
    let mut tip = 0u64;
    let signer = tx.key(0);
    for ix in &tx.ixs {
        let p = tx.key(ix.program);
        if p == pump::COMPUTE_BUDGET && !ix.data.is_empty() {
            match ix.data[0] {
                2 if ix.data.len() >= 5 => cu_limit = u32::from_le_bytes(ix.data[1..5].try_into().unwrap()),
                3 if ix.data.len() >= 9 => cu_price = u64::from_le_bytes(ix.data[1..9].try_into().unwrap()),
                _ => {}
            }
        } else if let Some((_, to, lam)) = system_transfer(tx, ix) {
            if TIP_ACCOUNTS.contains(&to) {
                tip += lam;
            }
        }
    }
    let meta = Meta {
        slot: tx.slot,
        tx_idx: tx.tx_idx,
        ev_idx: 0,
        block_time: tx.block_time,
        recv_us: tx.recv_us,
        sig: tx.sig,
        signer,
        cu_price,
        cu_limit,
        tip,
        tx_fee: tx.fee,
        shape: shape_hash(tx),
    };
    let mut ev_idx: u16 = 0;
    let mut push = |out: &mut Vec<Ev>, mut e: Ev| {
        match &mut e {
            Ev::Create(x) => x.m.ev_idx = ev_idx,
            Ev::Trade(x) => x.m.ev_idx = ev_idx,
            Ev::Complete(x) => x.m.ev_idx = ev_idx,
            Ev::Fund(x) => x.m.ev_idx = ev_idx,
            Ev::TokXfer(x) => x.m.ev_idx = ev_idx,
            Ev::Block(_) => {}
        }
        ev_idx = ev_idx.wrapping_add(1);
        out.push(e);
    };

    // ---- pump events (self-CPI is authoritative; logs are a fallback)
    let mut traded_mints: FxHashSet<Pk> = FxHashSet::default();
    if pump_touched {
        let mut payloads: Vec<Vec<u8>> = Vec::new();
        for (_, inner) in &tx.inner {
            for ix in inner {
                if tx.key(ix.program) == pump::PUMP_PROGRAM
                    && ix.data.len() > 16
                    && ix.data[..8] == pump::EVENT_IX_TAG
                {
                    payloads.push(ix.data[8..].to_vec());
                }
            }
        }
        if payloads.is_empty() {
            payloads = log_payloads(&tx.logs);
        }
        for p in payloads {
            match pump::decode_event(&p) {
                Some(RawEvent::Create(c)) => {
                    st.active_mints.insert(c.mint, tx.slot);
                    push(
                        out,
                        Ev::Create(Create {
                            m: meta.clone(),
                            mint: c.mint,
                            bonding_curve: c.bonding_curve,
                            user: c.user,
                            creator: c.creator,
                            name: c.name,
                            symbol: c.symbol,
                            uri: c.uri,
                            token_program: c.token_program,
                            quote_mint: c.quote_mint,
                            mayhem: c.mayhem,
                            cashback: c.cashback,
                            holder_reward: c.holder_reward,
                            depth: c.depth,
                            vsol: c.virtual_sol_reserves,
                            vtok: c.virtual_token_reserves,
                            rtok: c.real_token_reserves,
                            supply: c.token_total_supply,
                            creator_fee_bps: c.creator_fee_bps,
                        }),
                    );
                }
                Some(RawEvent::Trade(t)) => {
                    traded_mints.insert(t.mint);
                    let fee = t.fee + t.creator_fee + t.cashback;
                    let fee_bps = (t.fee_bps + t.creator_fee_bps + t.cashback_bps) as u32;
                    push(
                        out,
                        Ev::Trade(Trade {
                            m: meta.clone(),
                            mint: t.mint,
                            user: t.user,
                            is_buy: t.is_buy,
                            sol: t.sol_amount,
                            tok: t.token_amount,
                            fee,
                            fee_bps,
                            vsol: t.virtual_sol_reserves,
                            vtok: t.virtual_token_reserves,
                            rsol: t.real_sol_reserves,
                            rtok: t.real_token_reserves,
                            ix: pump::ix_code(&t.ix_name),
                            mayhem: t.mayhem,
                            sol_quote: pump::is_sol_quote(&t.quote_mint),
                        }),
                    );
                }
                Some(RawEvent::Complete(c)) => {
                    push(out, Ev::Complete(Complete { m: meta.clone(), mint: c.mint, user: c.user }));
                }
                None => {}
            }
        }
    }

    // ---- native SOL transfers: fresh-wallet fundings and watched wallets
    let idx_of = |p: &Pk| tx.keys.iter().position(|k| k == p);
    let mut seen_pairs: FxHashSet<(Pk, Pk)> = FxHashSet::default();
    let mut scan = |ix: &Ix, out: &mut Vec<Ev>| {
        if let Some((from, to, lam)) = system_transfer(tx, ix) {
            if from == to || TIP_ACCOUNTS.contains(&to) {
                return;
            }
            let ti = idx_of(&to);
            let fi = idx_of(&from);
            let to_pre = ti.and_then(|i| tx.pre_bal.get(i).copied()).unwrap_or(u64::MAX);
            let from_post = fi.and_then(|i| tx.post_bal.get(i).copied()).unwrap_or(u64::MAX);
            let fresh = to_pre == 0 && lam >= cfg.fund_min_lamports && lam <= cfg.fund_max_lamports;
            let watched = cfg.watch.contains(&from) || cfg.watch.contains(&to);
            if (fresh || watched) && seen_pairs.insert((from, to)) {
                push(
                    out,
                    Ev::Fund(Fund { m: meta.clone(), from, to, lamports: lam, to_pre, from_post }),
                );
            }
        }
    };
    for ix in &tx.ixs {
        scan(ix, out);
    }
    for (_, inner) in &tx.inner {
        for ix in inner {
            scan(ix, out);
        }
    }

    // ---- token movements of young pump mints that are not curve trades
    if !st.active_mints.is_empty() && !tx.post_tok.is_empty() {
        let mut deltas: FxHashMap<(Pk, Pk), i128> = FxHashMap::default();
        for b in &tx.pre_tok {
            if st.active_mints.contains_key(&b.mint) && !traded_mints.contains(&b.mint) {
                *deltas.entry((b.mint, b.owner)).or_default() -= b.amount as i128;
            }
        }
        for b in &tx.post_tok {
            if st.active_mints.contains_key(&b.mint) && !traded_mints.contains(&b.mint) {
                *deltas.entry((b.mint, b.owner)).or_default() += b.amount as i128;
            }
        }
        let mut by_mint: FxHashMap<Pk, (Vec<(Pk, u64)>, Vec<(Pk, u64)>)> = FxHashMap::default();
        for ((mint, owner), d) in deltas {
            // wallet-to-wallet moves only: pools, vaults and curves are PDAs (off-curve)
            if d == 0 || owner == Pk::ZERO || !owner.is_on_curve() {
                continue;
            }
            let e = by_mint.entry(mint).or_default();
            if d < 0 {
                e.0.push((owner, (-d) as u64));
            } else {
                e.1.push((owner, d as u64));
            }
        }
        let mut mints: Vec<_> = by_mint.into_iter().collect();
        mints.sort_by_key(|(m, _)| *m);
        for (mint, (mut senders, mut receivers)) in mints {
            if senders.is_empty() || receivers.is_empty() {
                continue;
            }
            senders.sort();
            receivers.sort();
            if receivers.len() == 1 {
                let to = receivers[0].0;
                for (from, amt) in senders {
                    push(out, Ev::TokXfer(TokXfer { m: meta.clone(), mint, from, to, amount: amt }));
                }
            } else {
                let from = senders.iter().max_by_key(|x| x.1).unwrap().0;
                for (to, amt) in receivers {
                    push(out, Ev::TokXfer(TokXfer { m: meta.clone(), mint, from, to, amount: amt }));
                }
            }
        }
    }

    if pump_touched {
        TxClass::PumpOk
    } else {
        TxClass::Other
    }
}

/// Extract `Program data:` payloads emitted while the pump program is on top of the stack.
pub fn log_payloads(logs: &[String]) -> Vec<Vec<u8>> {
    use base64::Engine;
    let pump_id = pump::PUMP_PROGRAM.to_b58();
    let mut stack: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for l in logs {
        if let Some(rest) = l.strip_prefix("Program ") {
            if let Some(data) = rest.strip_prefix("data: ") {
                if stack.last() == Some(&pump_id.as_str()) {
                    if let Ok(b) = base64::engine::general_purpose::STANDARD.decode(data.trim()) {
                        out.push(b);
                    }
                }
                continue;
            }
            let mut parts = rest.split_whitespace();
            let prog = parts.next().unwrap_or("");
            match parts.next() {
                Some("invoke") => stack.push(prog),
                Some("success") | Some("failed") => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    out
}
