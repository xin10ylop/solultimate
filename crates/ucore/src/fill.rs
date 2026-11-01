//! Landing (latency) model and transaction cost model.
//!
//! Default numbers come from 15 measured copy-trade landings on the production box
//! (Frankfurt, Triton gRPC "Riptide" feed, Nozomi fra2 sender):
//!   * 7/15 landed in the trigger's slot, 8/15 in the next slot, never later;
//!   * same slot: 5–13 other trades of the coin ahead of us; overall 2–13 trades between
//!     the trigger and us (median 4);
//!   * next slot: often the first trade of the coin in that block, 2–7 behind the trigger;
//!   * 3/15 buys refused by the price limit (~0.0004 SOL each).
//! The model never puts us first behind the trigger.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencyModel {
    pub p_same_slot: f64,
    /// Same-slot case: number of this coin's trades executed between the trigger and us.
    pub same_slot_between: Vec<(u32, f64)>,
    /// Next-slot case: number of this coin's trades in the landing slot ahead of us.
    pub next_slot_ahead: Vec<(u32, f64)>,
    /// Extra whole slots of delay (stress testing). 0 = measured behaviour.
    pub extra_slots: u32,
}

impl Default for LatencyModel {
    fn default() -> Self {
        LatencyModel {
            p_same_slot: 7.0 / 15.0,
            same_slot_between: vec![(2, 0.12), (3, 0.18), (4, 0.22), (5, 0.12), (6, 0.1), (8, 0.1), (10, 0.08), (13, 0.08)],
            next_slot_ahead: vec![(0, 0.55), (1, 0.25), (2, 0.12), (4, 0.08)],
            extra_slots: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Land in the trigger slot after `between` more trades of the coin (or at slot end).
    SameSlot { between: u32 },
    /// Land in trigger_slot + `offset` after `ahead` trades of the coin in that slot (or at its end).
    Later { offset: u32, ahead: u32 },
}

fn pick(dist: &[(u32, f64)], u: f64) -> u32 {
    let tot: f64 = dist.iter().map(|x| x.1).sum();
    let mut acc = 0.0;
    for (v, p) in dist {
        acc += p / tot;
        if u <= acc {
            return *v;
        }
    }
    dist.last().map(|x| x.0).unwrap_or(0)
}

impl LatencyModel {
    pub fn sample(&self, rng: &mut Rng) -> Landing {
        if self.extra_slots == 0 && rng.f64() < self.p_same_slot {
            Landing::SameSlot { between: pick(&self.same_slot_between, rng.f64()) }
        } else {
            Landing::Later { offset: 1 + self.extra_slots, ahead: pick(&self.next_slot_ahead, rng.f64()) }
        }
    }
    /// Pessimistic variants for robustness checks.
    pub fn stressed(level: u32) -> LatencyModel {
        let mut m = LatencyModel::default();
        match level {
            0 => {}
            1 => {
                m.p_same_slot = 0.25;
                m.next_slot_ahead = vec![(1, 0.4), (2, 0.3), (4, 0.2), (7, 0.1)];
            }
            _ => {
                m.p_same_slot = 0.0;
                m.extra_slots = level - 1;
                m.next_slot_ahead = vec![(2, 0.4), (4, 0.3), (7, 0.3)];
            }
        }
        m
    }
}

/// Fixed per-transaction costs (lamports).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CostModel {
    pub base_fee: u64,
    pub buy_cu_limit: u32,
    pub buy_cu_price: u64,
    pub buy_tip: u64,
    pub sell_cu_limit: u32,
    pub sell_cu_price: u64,
    pub sell_tip: u64,
}

impl Default for CostModel {
    fn default() -> Self {
        CostModel {
            base_fee: 5_000,
            buy_cu_limit: 120_000,
            buy_cu_price: 3_000_000, // 3 lamports/CU → 0.00036 SOL priority (≈ the measured 0.0004 fail cost)
            buy_tip: 1_000_000,      // 0.001 SOL Nozomi tip
            sell_cu_limit: 100_000,
            sell_cu_price: 2_000_000,
            sell_tip: 1_000_000,
        }
    }
}

impl CostModel {
    fn prio(limit: u32, price: u64) -> u64 {
        (limit as u128 * price as u128 / 1_000_000) as u64
    }
    pub fn buy_landed(&self) -> u64 {
        self.base_fee + Self::prio(self.buy_cu_limit, self.buy_cu_price) + self.buy_tip
    }
    /// A reverted tx still pays base + priority fee, but not the tip (tip ix reverts too).
    pub fn buy_failed(&self) -> u64 {
        self.base_fee + Self::prio(self.buy_cu_limit, self.buy_cu_price)
    }
    pub fn sell_landed(&self) -> u64 {
        self.base_fee + Self::prio(self.sell_cu_limit, self.sell_cu_price) + self.sell_tip
    }
    pub fn sell_failed(&self) -> u64 {
        self.base_fee + Self::prio(self.sell_cu_limit, self.sell_cu_price)
    }
}

/// Small deterministic RNG (splitmix64) so every replay is reproducible.
#[derive(Clone, Debug)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f64(&mut self) -> f64 {
        (self.u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
