//! Exit rules, positions, and order landing against the event stream.
//!
//! The same code drives (a) shadow positions on every launch (learning), (b) simulated
//! real positions in the backtest, and (c) in live mode the exit *decisions* of real
//! positions (fills then come from the chain instead of the landing model).

use crate::curve::Curve;
use crate::fill::{CostModel, Landing, LatencyModel, Rng};
use crate::mint::{MintTracker, Signals};
use crate::pk::Pk;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DumpMode {
    /// Ignore dump signals (rely on TP/SL/time).
    Off,
    /// Only strong signals: big sell, consolidation, operator wallet, transfer-fed seller.
    Hard,
    /// Any dump-like signal including create-slot buyers selling.
    All,
}

impl DumpMode {
    pub fn fires(&self, s: &Signals) -> bool {
        match self {
            DumpMode::Off => false,
            DumpMode::Hard => s.big_sell || s.consolidation || s.operator_sell || s.nonbuyer_sell,
            DumpMode::All => s.any_dump(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Entry {
    /// React to the create transaction (dev wallet / funding known, bundle not yet seen).
    #[default]
    Create,
    /// React to the end of the create slot ("first candle" closed, bundle visible).
    Block0End,
    /// React to the dev wallet's first sell (operators that re-pump after a fake dev exit).
    DevSell,
    /// React to the first hard dump signal (buy the crash for a rebound).
    AfterDump,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExitRules {
    pub name: String,
    #[serde(default)]
    pub entry: Entry,
    /// Take-profit ladder: (gain on cost basis, fraction of the *initial* tokens to sell).
    pub tp: Vec<(f64, f64)>,
    /// Stop loss on position value (negative, e.g. -0.35).
    pub sl: f64,
    /// Stop loss only arms after this many slots ("phantom SL").
    pub sl_arm_slots: u64,
    /// Trailing stop: (activation gain, giveback from peak gain).
    pub trail: Option<(f64, f64)>,
    /// Hard time stop measured from our fill.
    pub max_hold_slots: u64,
    /// Exit if the coin has no trade for this many slots.
    pub idle_slots: u64,
    /// Which dump signals force an immediate full exit.
    pub on_dump: DumpMode,
    /// Exit at this quantile of the operator's historical dump time (slots after create).
    pub dump_clock_q: Option<f64>,
}

/// The policy family evaluated in shadow on every launch.
pub fn default_variants() -> Vec<ExitRules> {
    use DumpMode::*;
    let v = |name: &str, tp: Vec<(f64, f64)>, sl: f64, trail: Option<(f64, f64)>, hold: u64, dump: DumpMode, clock: Option<f64>| ExitRules {
        name: name.into(),
        entry: if name.starts_with("b1_") {
            Entry::Block0End
        } else if name.starts_with("ds_") {
            Entry::DevSell
        } else if name.starts_with("dp_") {
            Entry::AfterDump
        } else {
            Entry::Create
        },
        tp,
        sl,
        sl_arm_slots: 0,
        trail,
        max_hold_slots: hold,
        idle_slots: 50,
        on_dump: dump,
        dump_clock_q: clock,
    };
    vec![
        v("t2s", vec![], -0.60, None, 5, Off, None),
        v("scalp40", vec![(0.40, 1.0)], -0.25, None, 15, Hard, None),
        v("tp70", vec![(0.70, 1.0)], -0.30, None, 40, Hard, None),
        v("tp100", vec![(1.00, 1.0)], -0.35, None, 75, Hard, None),
        v("tp150", vec![(1.50, 1.0)], -0.40, None, 150, Hard, None),
        v("tp100_raw", vec![(1.00, 1.0)], -0.40, None, 150, Off, None),
        v("tp200_raw", vec![(2.00, 1.0)], -0.50, None, 300, Off, None),
        v("trail60", vec![(1.50, 1.0)], -0.35, Some((0.60, 0.25)), 150, Hard, None),
        v("ladder", vec![(0.60, 0.5), (1.50, 0.5)], -0.35, Some((0.60, 0.30)), 225, Hard, None),
        v("clock30", vec![(2.00, 1.0)], -0.40, None, 300, Hard, Some(0.30)),
        v("tp70_all", vec![(0.70, 1.0)], -0.30, None, 40, All, None),
        v("t60s", vec![], -0.50, None, 150, Off, None),
        // entries after the first block closed (bundle and first candle visible)
        v("b1_t2s", vec![], -0.60, None, 5, Off, None),
        v("b1_scalp40", vec![(0.40, 1.0)], -0.25, None, 15, Hard, None),
        v("b1_tp70", vec![(0.70, 1.0)], -0.30, None, 40, Hard, None),
        v("b1_tp100", vec![(1.00, 1.0)], -0.35, None, 75, Hard, None),
        v("b1_trail60", vec![(1.50, 1.0)], -0.35, Some((0.60, 0.25)), 150, Hard, None),
        v("b1_t60s", vec![], -0.50, None, 150, Off, None),
        // lifecycle entries (latency-tolerant)
        v("ds_tp50", vec![(0.50, 1.0)], -0.30, None, 75, Hard, None),
        v("ds_tp100", vec![(1.00, 1.0)], -0.35, Some((0.5, 0.25)), 150, Off, None),
        v("dp_tp40", vec![(0.40, 1.0)], -0.25, None, 75, Off, None),
        v("dp_tp100", vec![(1.00, 1.0)], -0.40, Some((0.5, 0.3)), 300, Off, None),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug)]
pub struct PendingOrder {
    pub side: Side,
    pub trigger_slot: u64,
    /// tx index of the trigger; consecutive transactions right after it that trade the same
    /// coin are treated as one atomic group (Jito bundle) we can never land inside.
    pub trigger_tx: u32,
    pub group_open: bool,
    pub landing: Landing,
    pub seen: u32,
    /// Buy: lamports to spend (incl. fees). Sell: tokens to sell.
    pub amount: u64,
    /// Buy: min tokens out. Sell: min lamports out.
    pub min_out: u64,
    pub reason: &'static str,
    pub attempts: u32,
}

impl PendingOrder {
    pub fn landing_slot(&self) -> u64 {
        match self.landing {
            Landing::SameSlot { .. } => self.trigger_slot,
            Landing::Later { offset, .. } => self.trigger_slot + offset as u64,
        }
    }
    /// Should we land *before* a trade of this coin at (`slot`, `tx_idx`)? Updates counters.
    pub fn lands_before_trade(&mut self, slot: u64, tx_idx: u32) -> bool {
        if slot == self.trigger_slot && self.group_open {
            if tx_idx <= self.trigger_tx + 1 {
                // still inside the trigger's atomic group
                self.trigger_tx = self.trigger_tx.max(tx_idx);
                return false;
            }
            self.group_open = false;
        }
        let ls = self.landing_slot();
        if slot < ls {
            return false;
        }
        if slot > ls {
            return true;
        }
        let need = match self.landing {
            Landing::SameSlot { between } => between,
            Landing::Later { ahead, .. } => ahead,
        };
        if self.seen >= need {
            true
        } else {
            self.seen += 1;
            false
        }
    }
    /// Block boundary: land at end of landing slot if still pending.
    pub fn lands_at_block_end(&self, slot: u64) -> bool {
        slot >= self.landing_slot()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PosState {
    Entering,
    Open,
    Closed,
}

#[derive(Clone, Debug)]
pub struct Position {
    pub id: u64,
    pub mint: Pk,
    pub variant: usize,
    pub shadow: bool,
    pub stake: u64,
    pub decision_slot: u64,
    pub state: PosState,
    pub pending: Option<PendingOrder>,
    pub fill_slot: u64,
    pub entry_price: f64,
    pub entry_mcap: f64,
    /// Lamports spent on the buy, incl. curve fees (cost basis).
    pub cost: u64,
    pub tokens0: u64,
    pub tokens: u64,
    /// Net lamports we added to the curve (counterfactual shift).
    pub shift: i64,
    pub proceeds: u64,
    pub fixed_costs: u64,
    pub peak_gain: f64,
    pub tp_next: usize,
    pub exit_reason: &'static str,
    pub close_slot: u64,
    pub failed_entry: bool,
    pub rng: Rng,
    /// Operator dump-clock deadline (absolute slot), if any.
    pub clock_deadline: Option<u64>,
    pub operator: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub id: u64,
    pub mint: Pk,
    pub variant: usize,
    pub shadow: bool,
    pub operator: Option<u32>,
    pub decision_slot: u64,
    pub fill_slot: u64,
    pub close_slot: u64,
    pub stake: u64,
    pub cost: u64,
    pub proceeds: u64,
    pub fixed_costs: u64,
    pub pnl: i64,
    pub ret: f64,
    pub entry_mcap: f64,
    pub peak_gain: f64,
    pub exit_reason: String,
    pub failed_entry: bool,
}

pub struct ExecCtx<'a> {
    pub latency: &'a LatencyModel,
    pub costs: &'a CostModel,
    pub buy_slip: f64,
    pub sell_slip: f64,
    pub dump_sell_slip: f64,
}

impl Position {
    #[allow(clippy::too_many_arguments)]
    pub fn new_entry(
        id: u64,
        mint: Pk,
        variant: usize,
        shadow: bool,
        stake: u64,
        trigger_slot: u64,
        trigger_tx: u32,
        decision_curve: &Curve,
        fee_bps: u32,
        max_price: Option<f64>,
        cx: &ExecCtx,
        seed: u64,
    ) -> Position {
        let mut rng = Rng::new(seed);
        let landing = cx.latency.sample(&mut rng);
        let (tok_now, _, _) = decision_curve.buy_quote_in(stake, fee_bps);
        let mut min_out = (tok_now as f64 * (1.0 - cx.buy_slip)) as u64;
        if let Some(pmax) = max_price {
            // tokens we would get if the average price were pmax
            let net = stake as f64 * 10_000.0 / (10_000.0 + fee_bps as f64);
            min_out = min_out.max((net / pmax) as u64);
        }
        Position {
            id,
            mint,
            variant,
            shadow,
            stake,
            decision_slot: trigger_slot,
            state: PosState::Entering,
            pending: Some(PendingOrder {
                side: Side::Buy,
                trigger_slot,
                trigger_tx,
                group_open: true,
                landing,
                seen: 0,
                amount: stake,
                min_out,
                reason: "entry",
                attempts: 1,
            }),
            fill_slot: 0,
            entry_price: 0.0,
            entry_mcap: 0.0,
            cost: 0,
            tokens0: 0,
            tokens: 0,
            shift: 0,
            proceeds: 0,
            fixed_costs: 0,
            peak_gain: 0.0,
            tp_next: 0,
            exit_reason: "",
            close_slot: 0,
            failed_entry: false,
            rng,
            clock_deadline: None,
            operator: None,
        }
    }

    /// Value (lamports, net of curve fees) of selling all remaining tokens now.
    pub fn mark(&self, curve: &Curve, fee_bps: u32) -> u64 {
        if self.tokens == 0 {
            return 0;
        }
        curve.shifted(self.shift).sell(self.tokens, fee_bps).0
    }

    pub fn gain(&self, curve: &Curve, fee_bps: u32) -> f64 {
        if self.cost == 0 {
            return 0.0;
        }
        (self.proceeds + self.mark(curve, fee_bps)) as f64 / self.cost as f64 - 1.0
    }

    /// Execute the pending order against `curve` (state at our landing point).
    pub fn fill(&mut self, curve: &Curve, fee_bps: u32, slot: u64, cx: &ExecCtx) {
        let Some(mut o) = self.pending.take() else { return };
        let c = curve.shifted(self.shift);
        match o.side {
            Side::Buy => {
                let (tok, sol_net, fee) = c.buy_quote_in(o.amount, fee_bps);
                if tok == 0 || tok < o.min_out {
                    self.fixed_costs += cx.costs.buy_failed();
                    self.failed_entry = true;
                    self.state = PosState::Closed;
                    self.exit_reason = "entry_refused";
                    self.close_slot = slot;
                    return;
                }
                self.fixed_costs += cx.costs.buy_landed();
                self.cost = sol_net + fee;
                self.tokens0 = tok;
                self.tokens = tok;
                self.shift += sol_net as i64;
                self.fill_slot = slot;
                self.entry_price = (sol_net + fee) as f64 / tok as f64;
                self.entry_mcap = c.mcap_sol();
                self.state = PosState::Open;
            }
            Side::Sell => {
                let amt = o.amount.min(self.tokens);
                let (out, gross, _fee) = c.sell(amt, fee_bps);
                if out < o.min_out && o.attempts < 6 {
                    self.fixed_costs += cx.costs.sell_failed();
                    // retry immediately with a fresh landing and wider limit
                    o.attempts += 1;
                    o.trigger_slot = slot + 1;
                    o.group_open = false;
                    o.landing = cx.latency.sample(&mut self.rng);
                    o.seen = 0;
                    let slip = (cx.dump_sell_slip + 0.1 * o.attempts as f64).min(0.98);
                    o.min_out = (out as f64 * (1.0 - slip)) as u64;
                    self.pending = Some(o);
                    return;
                }
                self.fixed_costs += cx.costs.sell_landed();
                self.proceeds += out;
                self.tokens -= amt;
                self.shift -= gross as i64;
                if self.tokens == 0 {
                    self.state = PosState::Closed;
                    self.exit_reason = o.reason;
                    self.close_slot = slot;
                }
            }
        }
    }

    pub fn request_sell(&mut self, fraction_of_initial: f64, reason: &'static str, curve: &Curve, fee_bps: u32, slot: u64, dump: bool, cx: &ExecCtx) {
        self.request_sell_at(fraction_of_initial, reason, curve, fee_bps, slot, u32::MAX, dump, cx)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn request_sell_at(&mut self, fraction_of_initial: f64, reason: &'static str, curve: &Curve, fee_bps: u32, slot: u64, tx: u32, dump: bool, cx: &ExecCtx) {
        if self.pending.is_some() || self.state != PosState::Open || self.tokens == 0 {
            return;
        }
        let amt = ((self.tokens0 as f64 * fraction_of_initial) as u64).min(self.tokens).max(1);
        let amt = if self.tokens - amt < self.tokens0 / 50 { self.tokens } else { amt };
        let expect = curve.shifted(self.shift).sell(amt, fee_bps).0;
        let slip = if dump { cx.dump_sell_slip } else { cx.sell_slip };
        self.pending = Some(PendingOrder {
            side: Side::Sell,
            trigger_slot: slot,
            trigger_tx: tx,
            group_open: tx != u32::MAX,
            landing: cx.latency.sample(&mut self.rng),
            seen: 0,
            amount: amt,
            min_out: (expect as f64 * (1.0 - slip)) as u64,
            reason,
            attempts: 1,
        });
    }

    /// Evaluate exit rules after an event on this coin (or a block tick when `sig` is None).
    pub fn check_exits(&mut self, r: &ExitRules, mt: &MintTracker, slot: u64, sig: Option<Signals>, cx: &ExecCtx) {
        self.check_exits_at(r, mt, slot, u32::MAX, sig, cx)
    }
    pub fn check_exits_at(&mut self, r: &ExitRules, mt: &MintTracker, slot: u64, tx: u32, sig: Option<Signals>, cx: &ExecCtx) {
        if self.state != PosState::Open || self.pending.is_some() {
            return;
        }
        let curve = &mt.curve;
        let fee = mt.fee_bps;
        let g = self.gain(curve, fee);
        if g > self.peak_gain {
            self.peak_gain = g;
        }
        if mt.completed {
            self.request_sell_at(1.0, "migrated", curve, fee, slot, tx, true, cx);
            return;
        }
        if let Some(s) = sig {
            if r.on_dump.fires(&s) {
                self.request_sell_at(1.0, s.label(), curve, fee, slot, tx, true, cx);
                return;
            }
        }
        let held = slot.saturating_sub(self.fill_slot);
        if held >= r.sl_arm_slots && g <= r.sl {
            self.request_sell_at(1.0, "stop_loss", curve, fee, slot, tx, true, cx);
            return;
        }
        if let Some((act, give)) = r.trail {
            if self.peak_gain >= act && g <= self.peak_gain - give {
                self.request_sell_at(1.0, "trailing", curve, fee, slot, tx, false, cx);
                return;
            }
        }
        if self.tp_next < r.tp.len() && g >= r.tp[self.tp_next].0 {
            let frac = r.tp[self.tp_next].1;
            self.tp_next += 1;
            let last = self.tp_next == r.tp.len() && r.trail.is_none();
            self.request_sell_at(if last { 1.0 } else { frac }, "take_profit", curve, fee, slot, tx, false, cx);
            return;
        }
        if held >= r.max_hold_slots {
            self.request_sell_at(1.0, "time_stop", curve, fee, slot, tx, false, cx);
            return;
        }
        if let Some(d) = self.clock_deadline {
            if slot >= d {
                self.request_sell_at(1.0, "dump_clock", curve, fee, slot, tx, false, cx);
                return;
            }
        }
        if slot.saturating_sub(mt.last_trade_slot) >= r.idle_slots && held >= r.idle_slots {
            self.request_sell_at(1.0, "idle", curve, fee, slot, tx, false, cx);
        }
    }

    pub fn close_record(&self) -> ClosedTrade {
        let pnl = self.proceeds as i64 - self.cost as i64 - self.fixed_costs as i64;
        ClosedTrade {
            id: self.id,
            mint: self.mint,
            variant: self.variant,
            shadow: self.shadow,
            operator: self.operator,
            decision_slot: self.decision_slot,
            fill_slot: self.fill_slot,
            close_slot: self.close_slot,
            stake: self.stake,
            cost: self.cost,
            proceeds: self.proceeds,
            fixed_costs: self.fixed_costs,
            pnl,
            ret: pnl as f64 / self.stake.max(1) as f64,
            entry_mcap: self.entry_mcap,
            peak_gain: self.peak_gain,
            exit_reason: self.exit_reason.to_string(),
            failed_entry: self.failed_entry,
        }
    }
}
