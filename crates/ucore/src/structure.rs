//! Launch-Structure Engine (LSE): ride a professional operator's opening pump for ≤ 15 s.
//! Signals read launch structure and wallet-derived context (bundle share of supply,
//! fresh-funded bundle wallets, the operator's past outcomes). Nothing copies a wallet's trades.
//! Conditions are shared by the brain (live/backtest) and documented in
//! research/frozen_candidates.md (candidate D).

use crate::book::OperatorBook;
use crate::mint::MintTracker;
use crate::pump::secs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct StructureCfg {
    pub enabled: bool,
    pub op_quality: bool,
    pub op_min_resolved: u32,
    pub op_min_frac_2x: f64,
    pub g0_runmode: bool,
    pub g0_supply: f64,
    pub g0_fresh: f64,
    pub g0_window_s: u64,
    pub g0_min_n: usize,
    pub b0_runmode: bool,
    pub b0_supply: f64,
    pub b0_fresh: f64,
    pub b0_window_s: u64,
    pub b0_min_n: usize,
    pub big_bundle: bool,
    pub big_bundle_sol: f64,
    pub big_bundle_max_nonfresh: usize,
    pub fresh_bundle: bool,
    pub fresh_bundle_frac: f64,
    pub fresh_bundle_min_n: usize,
    pub fresh_window_s: u64,
    /// Max adverse price increase vs the decision state for bundle-end / first-candle
    /// entries (1.0 = accept up to 2× the trigger price).
    pub entry_slip: f64,
    /// Same for create-tx entries (the operator's bundle lands ahead of us: ~+165% typical).
    pub create_entry_slip: f64,
    /// Exit for bundle-end / first-candle members: hold seconds, stop loss, trailing (0 = off).
    pub exit_hold_s: u64,
    pub exit_sl: f64,
    pub exit_trail_act: f64,
    pub exit_trail_give: f64,
    /// Exit for create-tx (operator quality) entries.
    pub create_exit_hold_s: u64,
    pub create_exit_sl: f64,
    /// Anti-farm: skip operators with more than this many earlier launches (0 = off).
    pub max_op_prior: u32,
    /// Bundle-based members need at least this many create-slot buyers (0 = off).
    pub min_bundle_buyers: usize,
    /// First-candle run mode only for launches attributed to an operator with earlier launches
    /// (FOMO: snipe known ruggers; unknown operators lost in every period).
    pub b0_known_op: bool,
    /// First-candle run mode only below this market cap in SOL at decision time (0 = off;
    /// FOMO's rule 2: first candle ≤ ~$15K ≈ 160 SOL).
    pub b0_max_mcap_sol: f64,
}

impl Default for StructureCfg {
    fn default() -> Self {
        StructureCfg {
            enabled: false,
            op_quality: true,
            op_min_resolved: 3,
            op_min_frac_2x: 0.5,
            g0_runmode: true,
            g0_supply: 0.5,
            g0_fresh: 0.5,
            g0_window_s: 21_600,
            g0_min_n: 2,
            b0_runmode: true,
            b0_supply: 0.3,
            b0_fresh: 0.5,
            b0_window_s: 7_200,
            b0_min_n: 2,
            big_bundle: true,
            big_bundle_sol: 5.0,
            big_bundle_max_nonfresh: 1,
            fresh_bundle: true,
            fresh_bundle_frac: 0.6,
            fresh_bundle_min_n: 3,
            fresh_window_s: 7_200,
            entry_slip: 1.0,
            create_entry_slip: 2.0,
            exit_hold_s: 15,
            exit_sl: -0.15,
            exit_trail_act: 0.20,
            exit_trail_give: 0.10,
            create_exit_hold_s: 15,
            create_exit_sl: -0.15,
            max_op_prior: 20,
            min_bundle_buyers: 12,
            b0_known_op: false,
            b0_max_mcap_sol: 0.0,
        }
    }
}

/// Number of create-slot buyers whose first funding happened within `window_s` before launch.
pub fn fresh_buyers(mt: &MintTracker, book: &OperatorBook, window_s: u64) -> usize {
    let cs = mt.create_slot;
    mt.block0
        .keys()
        .filter(|w| book.funding_of(w).map(|(_, _, fs)| fs <= cs && fs + secs(window_s) >= cs).unwrap_or(false))
        .count()
}

/// Anti-farm check shared by all members.
pub fn not_farm(mt: &MintTracker, c: &StructureCfg) -> bool {
    c.max_op_prior == 0 || mt.ctx.op_created_before <= c.max_op_prior
}

fn big_enough(mt: &MintTracker, c: &StructureCfg) -> bool {
    mt.block0.len() >= c.min_bundle_buyers
}

pub fn runmode(mt: &MintTracker, book: &OperatorBook, supply: f64, fresh: f64, window_s: u64, min_n: usize) -> bool {
    let n = mt.block0.len();
    if n < min_n || mt.insider_share() < supply {
        return false;
    }
    fresh_buyers(mt, book, window_s) as f64 / n as f64 >= fresh
}

pub fn op_quality(book: &mut OperatorBook, mt: &MintTracker, c: &StructureCfg) -> bool {
    if !not_farm(mt, c) {
        return false;
    }
    let (n, _, f2) = book.peak_stats(mt.ctx.op);
    n >= c.op_min_resolved && f2 >= c.op_min_frac_2x
}

/// First-candle-close members. Returns the member name that fired.
pub fn at_b0(mt: &MintTracker, book: &OperatorBook, c: &StructureCfg) -> Option<&'static str> {
    if !not_farm(mt, c) || !big_enough(mt, c) {
        return None;
    }
    let known_ok = !c.b0_known_op || mt.ctx.how != 0;
    let mcap_ok = c.b0_max_mcap_sol <= 0.0 || mt.mcap_sol() <= c.b0_max_mcap_sol;
    if c.b0_runmode && known_ok && mcap_ok && runmode(mt, book, c.b0_supply, c.b0_fresh, c.b0_window_s, c.b0_min_n) {
        return Some("rm_b0");
    }
    let n = mt.block0.len();
    let fresh = fresh_buyers(mt, book, c.fresh_window_s);
    let b0_sol = mt.block0.values().sum::<u64>() as f64 / 1e9;
    if c.big_bundle && b0_sol >= c.big_bundle_sol && n.saturating_sub(fresh) <= c.big_bundle_max_nonfresh {
        return Some("bigbundle");
    }
    if c.fresh_bundle && n >= c.fresh_bundle_min_n && fresh as f64 / n as f64 >= c.fresh_bundle_frac {
        return Some("freshbundle");
    }
    None
}
