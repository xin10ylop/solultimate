//! Per-launch state: live curve, participants, and dump detectors.

use crate::curve::Curve;
use crate::event::{Create, TokXfer, Trade};
use crate::pk::Pk;
use crate::pump::TOKEN_SUPPLY;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Compact trade record kept for rolling order-flow features.
#[derive(Clone, Copy, Debug)]
pub struct TradeLite {
    pub slot: u64,
    pub is_buy: bool,
    pub sol: u64,
    pub user: Pk,
    pub price: f64,
    /// Seller's first buy of this coin was within the last 10 s (sniper / flipper exit).
    pub flip: bool,
}

/// Dump / exit signals raised by a single event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signals {
    /// Dev wallet sold.
    pub dev_sell: bool,
    /// A create-slot buyer sold a meaningful size.
    pub insider_sell: bool,
    /// A wallet that never bought on the curve sold (tokens arrived by transfer → hidden insider).
    pub nonbuyer_sell: bool,
    /// A single sell took a large share of the curve's SOL.
    pub big_sell: bool,
    /// Tokens are being consolidated into one wallet (classic pre-dump move).
    pub consolidation: bool,
    /// A wallet linked to this launch's operator sold.
    pub operator_sell: bool,
}

impl Signals {
    pub fn any_dump(&self) -> bool {
        self.insider_sell || self.nonbuyer_sell || self.big_sell || self.consolidation || self.operator_sell
    }
    pub fn merge(&mut self, o: Signals) {
        self.dev_sell |= o.dev_sell;
        self.insider_sell |= o.insider_sell;
        self.nonbuyer_sell |= o.nonbuyer_sell;
        self.big_sell |= o.big_sell;
        self.consolidation |= o.consolidation;
        self.operator_sell |= o.operator_sell;
    }
    pub fn label(&self) -> &'static str {
        if self.consolidation {
            "consolidation"
        } else if self.operator_sell {
            "operator_sell"
        } else if self.insider_sell {
            "insider_sell"
        } else if self.nonbuyer_sell {
            "nonbuyer_sell"
        } else if self.big_sell {
            "big_sell"
        } else if self.dev_sell {
            "dev_sell"
        } else {
            "none"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DumpCfg {
    pub insider_min_lamports: u64,
    pub insider_min_frac_rsol: f64,
    pub nonbuyer_min_lamports: u64,
    pub big_sell_min_lamports: u64,
    pub big_sell_frac_rsol: f64,
    pub consolidation_frac_supply: f64,
    pub operator_min_lamports: u64,
}

impl Default for DumpCfg {
    fn default() -> Self {
        DumpCfg {
            insider_min_lamports: 250_000_000,
            insider_min_frac_rsol: 0.04,
            nonbuyer_min_lamports: 100_000_000,
            big_sell_min_lamports: 500_000_000,
            big_sell_frac_rsol: 0.12,
            consolidation_frac_supply: 0.12,
            operator_min_lamports: 100_000_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MintTracker {
    pub mint: Pk,
    pub dev: Pk,
    pub create_slot: u64,
    pub create_tx_idx: u32,
    pub create_time: i64,
    pub mayhem: bool,
    pub tradable: bool,
    pub token_program: Pk,
    pub name: String,
    pub symbol: String,
    pub curve: Curve,
    pub fee_bps: u32,
    pub completed: bool,
    pub last_trade_slot: u64,
    pub n_trades: u32,
    /// Net tokens per wallet from curve trades and transfers.
    pub holdings: FxHashMap<Pk, i128>,
    pub buyers: FxHashSet<Pk>,
    /// Buyers in the creation slot (dev excluded) and their SOL.
    pub block0: FxHashMap<Pk, u64>,
    pub dev_buy_sol: u64,
    /// Distinct non-dev buyers in the create slot and the next one.
    pub crowd: u16,
    crowd_set: FxHashSet<Pk>,
    /// Tokens received via transfers, per wallet.
    pub received: FxHashMap<Pk, u64>,
    /// Curve state at the end of the create slot (first "candle").
    pub block0_end: Option<Curve>,
    /// First dump-like signal: (slot, label)
    pub first_dump: Option<(u64, &'static str)>,
    /// Operator wallets (filled by the brain on attribution).
    pub operator_wallets: FxHashSet<Pk>,
    // ----- reference-entry path stats (P-windows), seconds after realistic entry
    pub ref_entry: Option<(u64, f64)>,
    pub ref_peaks: [f64; 7],
    pub max_buy_lamports: u64,
    // ----- identity-free order-flow state
    pub recent: VecDeque<TradeLite>,
    pub first_buy: FxHashMap<Pk, u64>,
    pub init_price: f64,
    pub max_price: f64,
    pub dev_max_hold: i128,
    pub n_buys: u32,
    pub n_sells: u32,
    pub max_price_after_b0: f64,
    /// Wallet-derived launch context (set by the brain / flow simulator at create time).
    pub ctx: LaunchCtx,
}

/// Wallet-derived, non-copy-trading context of a launch.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaunchCtx {
    pub op: u32,
    /// 0 new, 1 dev reuse, 2 funder, 3 second-hop funder, 4 exchange exact amount, 5 insider
    pub how: u8,
    /// 0 unseen, 1 hub round amount, 2 hub non-round amount, 3 non-hub funder
    pub fund_kind: u8,
    pub fund_sol: f32,
    pub fund_age_s: f32,
    pub op_created_before: u32,
}

pub fn how_code(how: &str) -> u8 {
    match how {
        "dev_reuse" => 1,
        "funder" => 2,
        "funder2" => 3,
        "exchange_amount" => 4,
        "insider" => 5,
        _ => 0,
    }
}

/// P-window horizons: 2s, 5s, 10s, 30s, 60s, 3m, 10m (in slots).
pub const PWIN_SLOTS: [u64; 7] = {
    use crate::pump::secs;
    [secs(2), secs(5), secs(10), secs(30), secs(60), secs(180), secs(600)]
};

impl MintTracker {
    pub fn new(c: &Create) -> MintTracker {
        let tradable = crate::pump::is_sol_quote(&c.quote_mint) && c.depth == 0 && !c.cashback;
        MintTracker {
            mint: c.mint,
            dev: c.user,
            create_slot: c.m.slot,
            create_tx_idx: c.m.tx_idx,
            create_time: c.m.block_time,
            mayhem: c.mayhem,
            tradable,
            token_program: c.token_program,
            name: c.name.clone(),
            symbol: c.symbol.clone(),
            curve: Curve { vsol: c.vsol, vtok: c.vtok, rsol: 0, rtok: c.rtok },
            fee_bps: if c.mayhem { 0 } else { 125 },
            completed: false,
            last_trade_slot: c.m.slot,
            n_trades: 0,
            holdings: FxHashMap::default(),
            buyers: FxHashSet::default(),
            block0: FxHashMap::default(),
            dev_buy_sol: 0,
            crowd: 0,
            crowd_set: FxHashSet::default(),
            received: FxHashMap::default(),
            block0_end: None,
            first_dump: None,
            operator_wallets: FxHashSet::default(),
            ref_entry: None,
            ref_peaks: [f64::NAN; 7],
            max_buy_lamports: 0,
            recent: VecDeque::new(),
            first_buy: FxHashMap::default(),
            init_price: c.vsol as f64 / c.vtok.max(1) as f64,
            max_price: c.vsol as f64 / c.vtok.max(1) as f64,
            dev_max_hold: 0,
            n_buys: 0,
            n_sells: 0,
            max_price_after_b0: 0.0,
            ctx: LaunchCtx::default(),
        }
    }

    /// Peak price after the first block relative to the first-candle close (≥ 1).
    pub fn peak_after_b0(&self) -> f64 {
        match self.block0_end {
            Some(c) if c.price() > 0.0 => (self.max_price_after_b0 / c.price()).max(1.0),
            _ => 1.0,
        }
    }

    pub fn age_slots(&self, slot: u64) -> u64 {
        slot.saturating_sub(self.create_slot)
    }

    pub fn mcap_sol(&self) -> f64 {
        self.curve.mcap_sol()
    }

    /// Apply a curve trade. Returns the signals it raised.
    pub fn on_trade(&mut self, t: &Trade, cfg: &DumpCfg) -> Signals {
        let pre = self.curve;
        self.curve = Curve { vsol: t.vsol, vtok: t.vtok, rsol: t.rsol, rtok: t.rtok };
        if t.fee_bps > 0 || t.mayhem {
            self.fee_bps = t.fee_bps;
        }
        self.n_trades += 1;
        self.last_trade_slot = t.m.slot;
        let mut s = Signals::default();
        let in_block0 = t.m.slot == self.create_slot;
        if t.is_buy {
            self.max_buy_lamports = self.max_buy_lamports.max(t.sol);
            *self.holdings.entry(t.user).or_default() += t.tok as i128;
            self.buyers.insert(t.user);
            if t.user == self.dev {
                if in_block0 {
                    self.dev_buy_sol += t.sol;
                }
            } else {
                if in_block0 {
                    *self.block0.entry(t.user).or_default() += t.sol;
                }
                if t.m.slot <= self.create_slot + 1 && self.crowd_set.insert(t.user) {
                    self.crowd = self.crowd.saturating_add(1);
                }
            }
        } else {
            *self.holdings.entry(t.user).or_default() -= t.tok as i128;
            let rsol_pre = pre.rsol.max(1) as f64;
            if t.user == self.dev {
                s.dev_sell = true;
            }
            if self.block0.contains_key(&t.user)
                && t.sol >= cfg.insider_min_lamports
                && t.sol as f64 >= cfg.insider_min_frac_rsol * rsol_pre
            {
                s.insider_sell = true;
            }
            if !self.buyers.contains(&t.user) && t.user != self.dev && t.sol >= cfg.nonbuyer_min_lamports {
                s.nonbuyer_sell = true;
            }
            if t.sol >= cfg.big_sell_min_lamports && t.sol as f64 >= cfg.big_sell_frac_rsol * rsol_pre {
                s.big_sell = true;
            }
            if self.operator_wallets.contains(&t.user) && t.sol >= cfg.operator_min_lamports {
                s.operator_sell = true;
            }
        }
        if s.any_dump() && self.first_dump.is_none() {
            self.first_dump = Some((t.m.slot, s.label()));
        }
        // order-flow bookkeeping
        let price = self.curve.price();
        self.max_price = self.max_price.max(price);
        if t.m.slot > self.create_slot {
            self.max_price_after_b0 = self.max_price_after_b0.max(price);
        }
        let flip = if t.is_buy {
            self.n_buys += 1;
            self.first_buy.entry(t.user).or_insert(t.m.slot);
            false
        } else {
            self.n_sells += 1;
            self.first_buy.get(&t.user).map(|s0| t.m.slot.saturating_sub(*s0) <= crate::pump::secs(10)).unwrap_or(false)
        };
        if t.user == self.dev {
            let h = self.holdings.get(&self.dev).copied().unwrap_or(0);
            self.dev_max_hold = self.dev_max_hold.max(h);
        }
        self.recent.push_back(TradeLite { slot: t.m.slot, is_buy: t.is_buy, sol: t.sol, user: t.user, price, flip });
        let horizon = crate::pump::secs(60);
        while let Some(f) = self.recent.front() {
            if t.m.slot.saturating_sub(f.slot) > horizon {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        self.update_ref(t.m.slot);
        s
    }

    pub fn on_xfer(&mut self, x: &TokXfer, cfg: &DumpCfg) -> Signals {
        if self.completed || !x.from.is_on_curve() || !x.to.is_on_curve() {
            return Signals::default();
        }
        *self.holdings.entry(x.from).or_default() -= x.amount as i128;
        *self.holdings.entry(x.to).or_default() += x.amount as i128;
        let r = self.received.entry(x.to).or_default();
        *r += x.amount;
        let mut s = Signals::default();
        if *r as f64 >= cfg.consolidation_frac_supply * TOKEN_SUPPLY as f64 {
            s.consolidation = true;
            if self.first_dump.is_none() {
                self.first_dump = Some((x.m.slot, "consolidation"));
            }
        }
        s
    }

    /// Called on every block boundary for this mint's lifetime.
    pub fn on_block_end(&mut self, slot: u64) {
        if slot >= self.create_slot && self.block0_end.is_none() {
            self.block0_end = Some(self.curve);
        }
        self.update_ref(slot);
    }

    pub fn set_ref_entry(&mut self, slot: u64, price: f64) {
        if self.ref_entry.is_none() {
            self.ref_entry = Some((slot, price));
            self.ref_peaks = [0.0; 7];
        }
    }

    fn update_ref(&mut self, slot: u64) {
        if let Some((s0, p0)) = self.ref_entry {
            let g = self.curve.price() / p0 - 1.0;
            for (i, w) in PWIN_SLOTS.iter().enumerate() {
                if slot <= s0 + w && g > self.ref_peaks[i] {
                    self.ref_peaks[i] = g;
                }
            }
        }
    }

    /// Estimated share of supply held by create-slot buyers + dev right now.
    pub fn insider_share(&self) -> f64 {
        let mut tot: i128 = self.holdings.get(&self.dev).copied().unwrap_or(0).max(0);
        for w in self.block0.keys() {
            tot += self.holdings.get(w).copied().unwrap_or(0).max(0);
        }
        tot as f64 / TOKEN_SUPPLY as f64
    }
}
