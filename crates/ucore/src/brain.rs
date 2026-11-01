//! The brain: consumes the ordered event stream, learns operators from shadow trades on
//! every launch, and opens real positions only for operators whose selection-honest track
//! record clears the gate. Identical code runs in replay (Sim) and production (Live).

use crate::book::{GateCfg, GateDecision, LaunchRec, OperatorBook};
use crate::event::*;
use crate::fill::{CostModel, LatencyModel};
use crate::mint::{DumpCfg, MintTracker, Signals};
use crate::pk::Pk;
use crate::policy::{default_variants, ClosedTrade, ExecCtx, ExitRules, PosState, Position, Side};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecMode {
    /// Real positions are filled by the landing model (backtest / paper).
    Sim,
    /// Real positions are filled by the chain; brain emits intents.
    Live,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskCfg {
    pub bankroll_lamports: u64,
    pub stake_lamports: u64,
    pub max_stake_frac: f64,
    pub max_open: usize,
    pub daily_loss_limit_lamports: u64,
    pub max_consec_losses: u32,
    pub pause_slots_after_streak: u64,
    pub allow_mayhem: bool,
    /// Never enter above this market cap (SOL) when the operator has no first-candle history.
    pub default_max_entry_mcap: f64,
}

impl Default for RiskCfg {
    fn default() -> Self {
        RiskCfg {
            bankroll_lamports: 5_000_000_000,
            stake_lamports: 100_000_000,
            max_stake_frac: 0.05,
            max_open: 3,
            daily_loss_limit_lamports: 500_000_000,
            max_consec_losses: 5,
            pause_slots_after_streak: 9_000 * 3,
            allow_mayhem: false,
            default_max_entry_mcap: 80.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainCfg {
    pub mode: ExecMode,
    pub real_trading: bool,
    pub shadow_stake_lamports: u64,
    pub shadow_mayhem: bool,
    pub gate: GateCfg,
    pub dump: DumpCfg,
    pub latency: LatencyModel,
    pub costs: CostModel,
    pub buy_slip: f64,
    pub sell_slip: f64,
    pub dump_sell_slip: f64,
    pub risk: RiskCfg,
    /// Launch rows are emitted (and trackers dropped) this many slots after creation.
    pub resolve_slots: u64,
    /// Late attribution through recurring insiders is allowed until create_slot + this.
    pub late_attr_slots: u64,
    pub variants: Vec<ExitRules>,
    pub seed: u64,
    /// Hub wallets given by the operator of this system (exchange hot wallets etc.).
    pub static_hubs: Vec<String>,
}

impl Default for BrainCfg {
    fn default() -> Self {
        BrainCfg {
            mode: ExecMode::Sim,
            real_trading: true,
            shadow_stake_lamports: 100_000_000,
            shadow_mayhem: false,
            gate: GateCfg::default(),
            dump: DumpCfg::default(),
            latency: LatencyModel::default(),
            costs: CostModel::default(),
            buy_slip: 4.0,
            sell_slip: 0.25,
            dump_sell_slip: 0.6,
            risk: RiskCfg::default(),
            resolve_slots: 1500,
            late_attr_slots: 0,
            variants: default_variants(),
            seed: 7,
            static_hubs: Vec::new(),
        }
    }
}

/// Instructions for the live executor.
#[derive(Clone, Debug, Serialize)]
pub enum Intent {
    Buy { pos: u64, mint: Pk, lamports: u64, min_tokens: u64, fee_bps: u32, reason: String },
    Sell { pos: u64, mint: Pk, tokens: u64, min_lamports: u64, reason: String, urgent: bool },
}

#[derive(Clone, Debug)]
struct Launch {
    op: u32,
    how: &'static str,
    chosen: usize,
    gate: Option<GateDecision>,
    shadows: Vec<u64>,
    shadow_results: Vec<f32>,
    real: Option<u64>,
    learned: bool,
    block0_buyers: Vec<Pk>,
    real_ret: Option<f64>,
}

/// One row per launch for the research "play sheet".
#[derive(Clone, Debug, Serialize)]
pub struct LaunchRow {
    pub mint: String,
    pub symbol: String,
    pub name: String,
    pub dev: String,
    pub op: u32,
    pub op_launches_before: usize,
    pub how: String,
    pub create_slot: u64,
    pub create_time: i64,
    pub mayhem: bool,
    pub block0_buyers: usize,
    pub block0_sol: f64,
    pub dev_buy_sol: f64,
    pub crowd: u16,
    pub block0_end_mcap: f64,
    pub ref_entry_mcap: f64,
    pub p2s: f64,
    pub p5s: f64,
    pub p10s: f64,
    pub p30s: f64,
    pub p60s: f64,
    pub p3m: f64,
    pub p10m: f64,
    pub first_dump_slots: i64,
    pub dump_kind: String,
    pub max_buy_sol: f64,
    pub n_trades: u32,
    pub completed: bool,
    pub chosen: usize,
    pub honest: f64,
    pub gate_pass: bool,
    pub gate_reason: String,
    pub gate_n: usize,
    pub gate_lcb: f64,
    pub real_ret: f64,
    pub results: Vec<f32>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct Counters {
    pub launches: u64,
    pub tradable: u64,
    pub attributed_known: u64,
    pub late_attributed: u64,
    pub gate_pass: u64,
    pub real_entries: u64,
    pub real_refused: u64,
    pub risk_blocked: u64,
    pub how: FxHashMap<String, u64>,
    pub gate_reasons: FxHashMap<String, u64>,
}

pub struct Brain {
    pub cfg: BrainCfg,
    pub book: OperatorBook,
    pub mints: FxHashMap<Pk, MintTracker>,
    launches: FxHashMap<Pk, Launch>,
    positions: FxHashMap<u64, Position>,
    by_mint: FxHashMap<Pk, Vec<u64>>,
    next_id: u64,
    pending_decisions: Vec<(Pk, u64, u32, bool)>,
    pub prior: f64,
    pub now_slot: u64,
    pub now_time: i64,
    // risk state
    pub bankroll: i64,
    pub day: i64,
    pub day_pnl: i64,
    pub consec_losses: u32,
    pub paused_until: u64,
    open_real: FxHashSet<u64>,
    // outputs
    pub closed_real: Vec<ClosedTrade>,
    pub rows: Vec<LaunchRow>,
    pub intents: Vec<Intent>,
    pub counters: Counters,
}

fn seed_of(mint: &Pk, v: u64, salt: u64) -> u64 {
    u64::from_le_bytes(mint.0[..8].try_into().unwrap()) ^ (v << 40) ^ salt.wrapping_mul(0x9E37_79B9)
}

impl Brain {
    pub fn new(cfg: BrainCfg) -> Brain {
        let mut book = OperatorBook::default();
        for h in &cfg.static_hubs {
            if let Ok(p) = Pk::from_b58(h) {
                book.static_hubs.insert(p);
            }
        }
        let bankroll = cfg.risk.bankroll_lamports as i64;
        Brain {
            cfg,
            book,
            mints: FxHashMap::default(),
            launches: FxHashMap::default(),
            positions: FxHashMap::default(),
            by_mint: FxHashMap::default(),
            next_id: 1,
            pending_decisions: Vec::new(),
            prior: -0.15,
            now_slot: 0,
            now_time: 0,
            bankroll,
            day: 0,
            day_pnl: 0,
            consec_losses: 0,
            paused_until: 0,
            open_real: FxHashSet::default(),
            closed_real: Vec::new(),
            rows: Vec::new(),
            intents: Vec::new(),
            counters: Counters::default(),
        }
    }

    pub fn open_positions(&self) -> impl Iterator<Item = &Position> {
        self.positions.values()
    }
    pub fn position(&self, id: u64) -> Option<&Position> {
        self.positions.get(&id)
    }

    pub fn on_event(&mut self, ev: &Ev) {
        if !self.pending_decisions.is_empty() {
            let (s, t, _) = ev.order_key();
            let (_, slot, txi, _) = self.pending_decisions[0];
            if s != slot || t != txi {
                for (mint, slot, _, late) in std::mem::take(&mut self.pending_decisions) {
                    self.decide(mint, slot, late);
                }
            }
        }
        if let Some(m) = ev.meta() {
            self.now_slot = self.now_slot.max(m.slot);
            if m.block_time > 0 {
                self.now_time = m.block_time;
            }
        }
        match ev {
            Ev::Fund(f) => {
                if f.to_pre == 0 {
                    self.book.on_fund(f.from, f.to, f.lamports, f.m.slot);
                }
            }
            Ev::Create(c) => self.on_create(c),
            Ev::Trade(t) => self.on_trade(t),
            Ev::TokXfer(x) => {
                if let Some(mt) = self.mints.get_mut(&x.mint) {
                    let s = mt.on_xfer(x, &self.cfg.dump);
                    self.after_mint_event(x.mint, x.m.slot, s);
                }
            }
            Ev::Complete(c) => {
                if let Some(mt) = self.mints.get_mut(&c.mint) {
                    mt.completed = true;
                    self.after_mint_event(c.mint, c.m.slot, Signals::default());
                }
            }
            Ev::Block(b) => self.on_block(b),
        }
    }

    fn on_create(&mut self, c: &Create) {
        self.counters.launches += 1;
        let mt = MintTracker::new(c);
        if !mt.tradable || (mt.mayhem && !self.cfg.shadow_mayhem) {
            return;
        }
        self.counters.tradable += 1;
        let (op, how) = self.book.attribute_launch(c.user, c.m.slot);
        if how != "new" {
            self.counters.attributed_known += 1;
        }
        *self.counters.how.entry(how.to_string()).or_default() += 1;
        self.mints.insert(c.mint, mt);
        self.launches.insert(
            c.mint,
            Launch {
                op,
                how,
                chosen: self.cfg.gate.default_variant,
                gate: None,
                shadows: Vec::new(),
                shadow_results: vec![f32::NAN; self.cfg.variants.len()],
                real: None,
                learned: false,
                block0_buyers: Vec::new(),
                real_ret: None,
            },
        );
        // decide once the whole create transaction (incl. the dev buy) has been applied
        self.pending_decisions.push((c.mint, c.m.slot, c.m.tx_idx, false));
    }

    fn exec_ctx(cfg: &BrainCfg) -> ExecCtx<'_> {
        ExecCtx {
            latency: &cfg.latency,
            costs: &cfg.costs,
            buy_slip: cfg.buy_slip,
            sell_slip: cfg.sell_slip,
            dump_sell_slip: cfg.dump_sell_slip,
        }
    }

    fn decide(&mut self, mint: Pk, trigger_slot: u64, late: bool) {
        let Some(l) = self.launches.get(&mint).cloned() else { return };
        let nvar = self.cfg.variants.len();
        let prior = self.prior;
        let gate = match self.book.stats_of(l.op) {
            Some(s) => s.gate(trigger_slot, &self.cfg.gate, prior, nvar),
            None => return,
        };
        let insiders = self.book.insider_wallets(l.op);
        let Some(mt) = self.mints.get_mut(&mint) else { return };
        mt.operator_wallets.extend(insiders);
        let curve = mt.curve;
        let fee_bps = mt.fee_bps;
        let mayhem = mt.mayhem;
        let create_slot = mt.create_slot;
        let max_mcap = gate.max_mcap.unwrap_or(self.cfg.risk.default_max_entry_mcap);
        let max_price = max_mcap * 1e9 / crate::pump::TOKEN_SUPPLY as f64;
        let chosen = if gate.n > 0 { gate.variant } else { self.cfg.gate.default_variant };
        let clock = gate.clock_slots;
        let cx = Self::exec_ctx(&self.cfg);

        // shadows: every variant, once per launch (at the create trigger)
        let mut new_shadows = Vec::new();
        if !late {
            for v in 0..nvar {
                let id = self.next_id;
                self.next_id += 1;
                let mut p = Position::new_entry(
                    id,
                    mint,
                    v,
                    true,
                    self.cfg.shadow_stake_lamports,
                    trigger_slot,
                    &curve,
                    fee_bps,
                    Some(max_price),
                    &cx,
                    seed_of(&mint, v as u64, self.cfg.seed),
                );
                p.operator = Some(l.op);
                if self.cfg.variants[v].dump_clock_q.is_some() {
                    p.clock_deadline = clock.map(|c| create_slot + c.saturating_sub(1));
                }
                new_shadows.push(p);
            }
        }
        // real entry
        let mut real = None;
        if self.cfg.real_trading && gate.pass && l.real.is_none() {
            self.counters.gate_pass += 1;
            let stake = self.cfg.risk.stake_lamports.min((self.bankroll.max(0) as f64 * self.cfg.risk.max_stake_frac) as u64);
            let op_busy = self
                .open_real
                .iter()
                .any(|id| self.positions.get(id).and_then(|p| p.operator) == Some(l.op));
            let blocked = (mayhem && !self.cfg.risk.allow_mayhem)
                || self.open_real.len() >= self.cfg.risk.max_open
                || op_busy
                || trigger_slot < self.paused_until
                || self.day_pnl <= -(self.cfg.risk.daily_loss_limit_lamports as i64)
                || stake < 10_000_000;
            if blocked {
                self.counters.risk_blocked += 1;
            } else {
                let id = self.next_id;
                self.next_id += 1;
                let mut p = Position::new_entry(
                    id,
                    mint,
                    chosen,
                    false,
                    stake,
                    trigger_slot,
                    &curve,
                    fee_bps,
                    Some(max_price),
                    &cx,
                    seed_of(&mint, chosen as u64, self.cfg.seed ^ 0xABCD),
                );
                p.operator = Some(l.op);
                if self.cfg.variants[chosen].dump_clock_q.is_some() {
                    p.clock_deadline = clock.map(|c| create_slot + c.saturating_sub(1));
                }
                if self.cfg.mode == ExecMode::Live {
                    let o = p.pending.as_ref().unwrap();
                    self.intents.push(Intent::Buy {
                        pos: id,
                        mint,
                        lamports: o.amount,
                        min_tokens: o.min_out,
                        fee_bps,
                        reason: format!("op{} {} lcb={:.3} n={}", l.op, self.cfg.variants[chosen].name, gate.lcb, gate.n),
                    });
                }
                self.counters.real_entries += 1;
                real = Some(p);
            }
        } else if !gate.pass {
            *self.counters.gate_reasons.entry(gate.reason.to_string()).or_default() += 1;
        }
        let ids: Vec<u64> = new_shadows.iter().map(|p| p.id).collect();
        for p in new_shadows {
            self.by_mint.entry(mint).or_default().push(p.id);
            self.positions.insert(p.id, p);
        }
        let real_id = real.as_ref().map(|p| p.id);
        if let Some(p) = real {
            self.by_mint.entry(mint).or_default().push(p.id);
            self.open_real.insert(p.id);
            self.positions.insert(p.id, p);
        }
        if let Some(lm) = self.launches.get_mut(&mint) {
            lm.shadows.extend(ids);
            if lm.gate.is_none() || real_id.is_some() {
                lm.gate = Some(gate);
            }
            lm.chosen = chosen;
            if real_id.is_some() {
                lm.real = real_id;
            }
        }
    }

    fn on_trade(&mut self, t: &Trade) {
        let Some(mt) = self.mints.get_mut(&t.mint) else { return };
        // 1) pending orders that land before this trade
        let ids = self.by_mint.get(&t.mint).cloned().unwrap_or_default();
        {
            let cx = Self::exec_ctx(&self.cfg);
            for id in &ids {
                if let Some(p) = self.positions.get_mut(id) {
                    if self.cfg.mode == ExecMode::Live && !p.shadow {
                        continue;
                    }
                    let lands = p.pending.as_mut().map(|o| o.lands_before_trade(t.m.slot)).unwrap_or(false);
                    if lands {
                        let was_entering = p.state == PosState::Entering;
                        p.fill(&mt.curve, mt.fee_bps, t.m.slot, &cx);
                        if was_entering && p.state == PosState::Open && p.variant == self.cfg.gate.default_variant && p.shadow {
                            mt.set_ref_entry(t.m.slot, mt.curve.price());
                        }
                    }
                }
            }
        }
        // 2) apply the trade
        let create_slot = mt.create_slot;
        let s = mt.on_trade(t, &self.cfg.dump);
        // 3) late attribution through recurring insiders
        if t.is_buy && t.m.slot <= create_slot + self.cfg.late_attr_slots {
            if let Some(l) = self.launches.get(&t.mint) {
                if l.real.is_none() && l.how == "new" {
                    if let Some(x) = self.book.insider_op(&t.user) {
                        if x != l.op {
                            let dev_op = l.op;
                            // evidence of a shared operator: merge
                            let root = self.book_union(dev_op, x);
                            if let Some(lm) = self.launches.get_mut(&t.mint) {
                                lm.op = root;
                                lm.how = "insider";
                            }
                            self.counters.late_attributed += 1;
                            self.pending_decisions.push((t.mint, t.m.slot, t.m.tx_idx, true));
                        }
                    }
                }
            }
        }
        self.after_mint_event(t.mint, t.m.slot, s);
    }

    fn book_union(&mut self, a: u32, b: u32) -> u32 {
        // union via a synthetic link: operators are node ids in the same union-find
        self.book.merge_ops(a, b)
    }

    fn after_mint_event(&mut self, mint: Pk, slot: u64, s: Signals) {
        let Some(mt) = self.mints.get(&mint) else { return };
        let Some(ids) = self.by_mint.get(&mint) else { return };
        let cx = Self::exec_ctx(&self.cfg);
        for id in ids {
            if let Some(p) = self.positions.get_mut(id) {
                let before = p.pending.is_some();
                p.check_exits(&self.cfg.variants[p.variant], mt, slot, Some(s), &cx);
                if !before && p.pending.is_some() && self.cfg.mode == ExecMode::Live && !p.shadow {
                    let o = p.pending.as_ref().unwrap();
                    self.intents.push(Intent::Sell {
                        pos: p.id,
                        mint,
                        tokens: o.amount,
                        min_lamports: o.min_out,
                        reason: o.reason.to_string(),
                        urgent: s.any_dump(),
                    });
                }
            }
        }
    }

    fn on_block(&mut self, b: &BlockStat) {
        for (mint, slot, _, late) in std::mem::take(&mut self.pending_decisions) {
            self.decide(mint, slot, late);
        }
        self.now_slot = self.now_slot.max(b.slot);
        if b.block_time > 0 {
            self.now_time = b.block_time;
        }
        let day = self.now_time / 86_400;
        if day != self.day {
            self.day = day;
            self.day_pnl = 0;
        }
        let live = self.cfg.mode == ExecMode::Live;
        // 1) block-end landings, block-end bookkeeping, time-based exits
        {
            let cx = Self::exec_ctx(&self.cfg);
            for (mint, ids) in self.by_mint.iter() {
                let Some(mt) = self.mints.get_mut(mint) else { continue };
                for id in ids {
                    let Some(p) = self.positions.get_mut(id) else { continue };
                    if live && !p.shadow {
                        continue;
                    }
                    if p.pending.as_ref().map(|o| o.lands_at_block_end(b.slot)).unwrap_or(false) {
                        let was_entering = p.state == PosState::Entering;
                        p.fill(&mt.curve, mt.fee_bps, b.slot, &cx);
                        if was_entering && p.state == PosState::Open && p.variant == self.cfg.gate.default_variant && p.shadow {
                            mt.set_ref_entry(b.slot, mt.curve.price());
                        }
                    }
                }
            }
            for mt in self.mints.values_mut() {
                mt.on_block_end(b.slot);
            }
            for (mint, ids) in self.by_mint.iter() {
                let Some(mt) = self.mints.get(mint) else { continue };
                for id in ids {
                    if let Some(p) = self.positions.get_mut(id) {
                        let before = p.pending.is_some();
                        p.check_exits(&self.cfg.variants[p.variant], mt, b.slot + 1, None, &cx);
                        if live && !p.shadow && !before && p.pending.is_some() {
                            let o = p.pending.as_ref().unwrap();
                            self.intents.push(Intent::Sell {
                                pos: p.id,
                                mint: *mint,
                                tokens: o.amount,
                                min_lamports: o.min_out,
                                reason: o.reason.to_string(),
                                urgent: false,
                            });
                        }
                    }
                }
            }
        }
        // 2) collect closed positions
        let closed: Vec<u64> = self.positions.iter().filter(|(_, p)| p.state == PosState::Closed).map(|(id, _)| *id).collect();
        for id in closed {
            let p = self.positions.remove(&id).unwrap();
            if let Some(v) = self.by_mint.get_mut(&p.mint) {
                v.retain(|x| *x != id);
            }
            let rec = p.close_record();
            if let Some(l) = self.launches.get_mut(&p.mint) {
                if p.shadow {
                    l.shadow_results[p.variant] = rec.ret as f32;
                } else {
                    l.real_ret = Some(rec.ret);
                }
            }
            if !p.shadow {
                self.open_real.remove(&id);
                self.bankroll += rec.pnl;
                self.day_pnl += rec.pnl;
                if rec.pnl <= 0 {
                    self.consec_losses += 1;
                    if self.consec_losses >= self.cfg.risk.max_consec_losses {
                        self.paused_until = b.slot + self.cfg.risk.pause_slots_after_streak;
                        self.consec_losses = 0;
                    }
                } else {
                    self.consec_losses = 0;
                }
                if rec.failed_entry {
                    self.counters.real_refused += 1;
                }
                self.closed_real.push(rec);
            }
        }
        // 3) learning + resolution
        let mut learn: Vec<Pk> = Vec::new();
        let mut done: Vec<Pk> = Vec::new();
        for (mint, l) in self.launches.iter() {
            let Some(mt) = self.mints.get(mint) else { continue };
            let age = mt.age_slots(b.slot);
            let open = self.by_mint.get(mint).map(|v| !v.is_empty()).unwrap_or(false);
            if !l.learned && !l.shadows.is_empty() && !open && (mt.first_dump.is_some() || age >= 300) {
                learn.push(*mint);
            }
            if age >= self.cfg.resolve_slots && !open {
                done.push(*mint);
            }
        }
        for mint in learn {
            self.learn(mint);
        }
        for mint in done {
            if !self.launches.get(&mint).map(|l| l.learned).unwrap_or(true) {
                self.learn(mint);
            }
            self.resolve(mint);
        }
    }

    fn learn(&mut self, mint: Pk) {
        let Some(l) = self.launches.get_mut(&mint) else { return };
        let Some(mt) = self.mints.get(&mint) else { return };
        l.learned = true;
        let honest = l.shadow_results.get(l.chosen).copied().unwrap_or(f32::NAN);
        if honest.is_finite() {
            self.prior = 0.998 * self.prior + 0.002 * honest as f64;
        }
        let mut b0: Vec<(Pk, u64)> = mt.block0.iter().map(|(k, v)| (*k, *v)).collect();
        b0.sort_by(|a, b| b.1.cmp(&a.1));
        let buyers: Vec<Pk> = b0.iter().take(24).map(|x| x.0).collect();
        l.block0_buyers = buyers.clone();
        let rec = LaunchRec {
            mint,
            slot: mt.create_slot,
            res: l.shadow_results.clone(),
            honest,
            chosen: l.chosen,
            dump_slots: mt.first_dump.map(|(s, _)| s.saturating_sub(mt.create_slot) as u32),
            block0_end_mcap: mt.block0_end.map(|c| c.mcap_sol() as f32).unwrap_or(f32::NAN),
            crowd: mt.crowd,
            block0_sol: mt.block0.values().sum::<u64>() as f32 / 1e9,
        };
        let op = l.op;
        self.book.record(op, rec, &buyers);
    }

    fn resolve(&mut self, mint: Pk) {
        let Some(l) = self.launches.remove(&mint) else { return };
        let Some(mt) = self.mints.remove(&mint) else { return };
        self.by_mint.remove(&mint);
        let op_launches_before = self
            .book
            .stats_of(l.op)
            .map(|s| s.launches.iter().filter(|x| x.slot < mt.create_slot).count())
            .unwrap_or(0);
        let g = l.gate.clone();
        let p = |i: usize| mt.ref_peaks[i];
        self.rows.push(LaunchRow {
            mint: mint.to_b58(),
            symbol: mt.symbol.clone(),
            name: mt.name.clone(),
            dev: mt.dev.to_b58(),
            op: self.book.find(l.op),
            op_launches_before,
            how: l.how.to_string(),
            create_slot: mt.create_slot,
            create_time: mt.create_time,
            mayhem: mt.mayhem,
            block0_buyers: mt.block0.len(),
            block0_sol: mt.block0.values().sum::<u64>() as f64 / 1e9,
            dev_buy_sol: mt.dev_buy_sol as f64 / 1e9,
            crowd: mt.crowd,
            block0_end_mcap: mt.block0_end.map(|c| c.mcap_sol()).unwrap_or(f64::NAN),
            ref_entry_mcap: mt.ref_entry.map(|(_, p0)| p0 * crate::pump::TOKEN_SUPPLY as f64 / 1e9).unwrap_or(f64::NAN),
            p2s: p(0),
            p5s: p(1),
            p10s: p(2),
            p30s: p(3),
            p60s: p(4),
            p3m: p(5),
            p10m: p(6),
            first_dump_slots: mt.first_dump.map(|(s, _)| (s - mt.create_slot) as i64).unwrap_or(-1),
            dump_kind: mt.first_dump.map(|(_, k)| k.to_string()).unwrap_or_default(),
            max_buy_sol: mt.max_buy_lamports as f64 / 1e9,
            n_trades: mt.n_trades,
            completed: mt.completed,
            chosen: l.chosen,
            honest: l.shadow_results.get(l.chosen).copied().unwrap_or(f32::NAN) as f64,
            gate_pass: g.as_ref().map(|g| g.pass).unwrap_or(false),
            gate_reason: g.as_ref().map(|g| g.reason.to_string()).unwrap_or_default(),
            gate_n: g.as_ref().map(|g| g.n).unwrap_or(0),
            gate_lcb: g.as_ref().map(|g| g.lcb).unwrap_or(f64::NAN),
            real_ret: l.real_ret.unwrap_or(f64::NAN),
            results: l.shadow_results.clone(),
        });
    }

    // ------------------------------------------------------------------ live hooks

    /// Live executor reports our buy landed.
    pub fn on_live_buy_fill(&mut self, pos: u64, lamports_spent: u64, sol_net: u64, tokens: u64, slot: u64, fixed_cost: u64) {
        let Some(mt) = self.mints.get(&self.positions.get(&pos).map(|p| p.mint).unwrap_or_default()) else { return };
        let mcap = mt.curve.mcap_sol();
        if let Some(p) = self.positions.get_mut(&pos) {
            p.pending = None;
            p.state = PosState::Open;
            p.cost = lamports_spent;
            p.tokens0 = tokens;
            p.tokens = tokens;
            // the live curve already includes our trade; no counterfactual shift needed
            p.shift = 0;
            let _ = sol_net;
            p.fill_slot = slot;
            p.entry_price = lamports_spent as f64 / tokens.max(1) as f64;
            p.entry_mcap = mcap;
            p.fixed_costs += fixed_cost;
        }
    }
    /// Live executor reports the buy failed / expired.
    pub fn on_live_buy_failed(&mut self, pos: u64, slot: u64, fixed_cost: u64) {
        if let Some(p) = self.positions.get_mut(&pos) {
            p.pending = None;
            p.failed_entry = true;
            p.state = PosState::Closed;
            p.exit_reason = "entry_failed";
            p.close_slot = slot;
            p.fixed_costs += fixed_cost;
        }
    }
    pub fn on_live_sell_fill(&mut self, pos: u64, tokens: u64, lamports_out: u64, slot: u64, fixed_cost: u64) {
        if let Some(p) = self.positions.get_mut(&pos) {
            let reason = p.pending.as_ref().map(|o| o.reason).unwrap_or("sell");
            p.pending = None;
            p.proceeds += lamports_out;
            p.tokens = p.tokens.saturating_sub(tokens);
            p.fixed_costs += fixed_cost;
            if p.tokens == 0 {
                p.state = PosState::Closed;
                p.exit_reason = reason;
                p.close_slot = slot;
            }
        }
    }
    /// Sell did not land: clear pending so exits re-fire on the next event (with fresh limits).
    pub fn on_live_sell_failed(&mut self, pos: u64, fixed_cost: u64) {
        if let Some(p) = self.positions.get_mut(&pos) {
            if let Some(o) = &p.pending {
                if o.side == Side::Sell {
                    p.pending = None;
                }
            }
            p.fixed_costs += fixed_cost;
        }
    }
    /// Emergency: request exit of every real position.
    pub fn panic_sell_all(&mut self, reason: &'static str) {
        let cx = Self::exec_ctx(&self.cfg);
        let ids: Vec<u64> = self.open_real.iter().copied().collect();
        for id in ids {
            if let Some(p) = self.positions.get_mut(&id) {
                if let Some(mt) = self.mints.get(&p.mint) {
                    p.request_sell(1.0, reason, &mt.curve, mt.fee_bps, self.now_slot, true, &cx);
                    if self.cfg.mode == ExecMode::Live {
                        if let Some(o) = &p.pending {
                            self.intents.push(Intent::Sell { pos: id, mint: p.mint, tokens: o.amount, min_lamports: o.min_out, reason: reason.into(), urgent: true });
                        }
                    }
                }
            }
        }
    }
    /// Drop simulated real positions and reset the risk ledger (after warm-up replay).
    pub fn reset_real(&mut self, bankroll_lamports: u64) {
        let ids: Vec<u64> = self.open_real.drain().collect();
        for id in ids {
            if let Some(p) = self.positions.remove(&id) {
                if let Some(v) = self.by_mint.get_mut(&p.mint) {
                    v.retain(|x| *x != id);
                }
            }
        }
        for l in self.launches.values_mut() {
            l.real = None;
        }
        self.closed_real.clear();
        self.intents.clear();
        self.bankroll = bankroll_lamports as i64;
        self.day_pnl = 0;
        self.consec_losses = 0;
        self.paused_until = 0;
        self.counters = Counters::default();
    }
    /// Token program of a tracked mint.
    pub fn token_program(&self, mint: &Pk) -> Option<Pk> {
        self.mints.get(mint).map(|m| m.token_program)
    }
    pub fn real_open_count(&self) -> usize {
        self.open_real.len()
    }
}
