//! Research simulator for identity-free entries: at sampled decision points it records the
//! order-flow features and simulates several generic round trips with the landing model.
//! Output rows feed model fitting (lab features → research/flow_model.py).

use crate::event::*;
use crate::fill::{CostModel, LatencyModel};
use crate::flow::features;
use crate::mint::{DumpCfg, MintTracker};
use crate::pk::Pk;
use crate::policy::{DumpMode, Entry, ExecCtx, ExitRules, PosState, Position};
use crate::pump::secs;
use rustc_hash::FxHashMap;

pub fn label_rules() -> Vec<ExitRules> {
    let r = |name: &str, tp: Vec<(f64, f64)>, sl: f64, trail: Option<(f64, f64)>, hold_s: u64| ExitRules {
        name: name.into(),
        entry: Entry::Create,
        tp,
        sl,
        sl_arm_slots: 0,
        trail,
        max_hold_slots: secs(hold_s),
        idle_slots: secs(10),
        on_dump: DumpMode::Off,
        dump_clock_q: None,
        progress_exit: None,
    };
    vec![
        r("h5", vec![], -0.25, None, 5),
        r("h15", vec![(0.25, 1.0)], -0.15, None, 15),
        r("h30", vec![(0.50, 1.0)], -0.20, Some((0.20, 0.15)), 30),
        r("h60", vec![(1.00, 1.0)], -0.30, Some((0.40, 0.25)), 60),
    ]
}

pub struct FlowRow {
    pub time: i64,
    pub slot: u64,
    pub mint: Pk,
    /// 1 = decision taken right after a buy by a tagged wallet (research: where do winners buy?)
    pub tag: u8,
    pub feats: Vec<f32>,
    /// per label: (net return excl. fixed costs, fixed lamports, exit reason, failed entry)
    pub res: Vec<(f64, u64, &'static str, bool)>,
}

struct Acc {
    row: FlowRow,
    left: usize,
}

pub struct FlowSim {
    pub book: crate::book::OperatorBook,
    peak_pending: Vec<(Pk, u64)>,
    pub mints: FxHashMap<Pk, MintTracker>,
    positions: FxHashMap<u64, (Position, usize, u64)>,
    by_mint: FxHashMap<Pk, Vec<u64>>,
    rows: FxHashMap<u64, Acc>,
    last_eval: FxHashMap<Pk, (u64, u32)>,
    pub out: Vec<FlowRow>,
    pub labels: Vec<ExitRules>,
    pub latency: LatencyModel,
    pub costs: CostModel,
    pub dump: DumpCfg,
    pub stake: u64,
    pub buy_slip: f64,
    pub eval_every_slots: u64,
    pub max_evals_per_coin: u32,
    pub min_age_slots: u64,
    pub max_age_slots: u64,
    pub min_progress: f64,
    pub max_progress: f64,
    next_id: u64,
    now_time: i64,
    /// Wallets whose buys force a decision row (tag = 1).
    pub tag_wallets: rustc_hash::FxHashSet<Pk>,
    /// Fraction of regular (untagged) decision rows kept (deterministic by mint and slot).
    pub regular_keep: f64,
}

impl FlowSim {
    pub fn new(stake: u64, latency: LatencyModel) -> FlowSim {
        FlowSim {
            book: crate::book::OperatorBook::default(),
            peak_pending: Vec::new(),
            mints: FxHashMap::default(),
            positions: FxHashMap::default(),
            by_mint: FxHashMap::default(),
            rows: FxHashMap::default(),
            last_eval: FxHashMap::default(),
            out: Vec::new(),
            labels: label_rules(),
            latency,
            costs: CostModel::default(),
            dump: DumpCfg::default(),
            stake,
            buy_slip: 0.30,
            eval_every_slots: secs(1),
            max_evals_per_coin: 120,
            min_age_slots: secs(2),
            max_age_slots: secs(600),
            min_progress: 0.0,
            max_progress: 0.85,
            next_id: 1,
            now_time: 0,
            tag_wallets: rustc_hash::FxHashSet::default(),
            regular_keep: 1.0,
        }
    }

    fn cx(&self) -> ExecCtx<'_> {
        ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: self.buy_slip, sell_slip: 0.25, dump_sell_slip: 0.6 }
    }

    pub fn on_event(&mut self, ev: &Ev) {
        if let Some(m) = ev.meta() {
            if m.block_time > 0 {
                self.now_time = m.block_time;
            }
        }
        match ev {
            Ev::Create(c) => {
                let mut mt = MintTracker::new(c);
                if mt.tradable && !mt.mayhem {
                    // same attribution as the brain (pre-launch facts only)
                    let (op, how) = self.book.attribute_launch(c.user, c.m.slot);
                    mt.ctx = self.book.launch_ctx(op, how, &c.user, c.m.slot);
                    self.peak_pending.push((c.mint, c.m.slot));
                    self.mints.insert(c.mint, mt);
                }
            }
            Ev::Trade(t) => self.on_trade(t),
            Ev::TokXfer(x) => {
                if let Some(mt) = self.mints.get_mut(&x.mint) {
                    mt.on_xfer(x, &self.dump);
                }
            }
            Ev::Complete(c) => {
                if let Some(mt) = self.mints.get_mut(&c.mint) {
                    mt.completed = true;
                }
                self.check(c.mint, c.m.slot, c.m.tx_idx);
            }
            Ev::Block(b) => self.on_block(b),
            Ev::Pool(_) | Ev::Dbc(_) => {}
            Ev::Fund(f) => {
                if f.to_pre == 0 {
                    self.book.on_fund(f.from, f.to, f.lamports, f.m.slot);
                }
            }
        }
    }

    fn on_trade(&mut self, t: &Trade) {
        let ids = self.by_mint.get(&t.mint).cloned().unwrap_or_default();
        {
            let Some(mt) = self.mints.get(&t.mint) else { return };
            let curve = mt.curve;
            let fee = mt.fee_bps;
            let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: self.buy_slip, sell_slip: 0.25, dump_sell_slip: 0.6 };
            for id in &ids {
                if let Some((p, _, _)) = self.positions.get_mut(id) {
                    if p.pending.as_mut().map(|o| o.lands_before_trade(t.m.slot, t.m.tx_idx)).unwrap_or(false) {
                        p.fill(&curve, fee, t.m.slot, &cx);
                    }
                }
            }
        }
        let Some(mt) = self.mints.get_mut(&t.mint) else { return };
        mt.on_trade(t, &self.dump);
        self.check(t.mint, t.m.slot, t.m.tx_idx);
        // decision point
        let Some(mt) = self.mints.get(&t.mint) else { return };
        let age = mt.age_slots(t.m.slot);
        let pr = mt.curve.progress();
        if age < self.min_age_slots || age > self.max_age_slots || mt.completed || pr > self.max_progress || pr < self.min_progress {
            return;
        }
        let tagged = t.is_buy && self.tag_wallets.contains(&t.user);
        let le = self.last_eval.entry(t.mint).or_insert((0, 0));
        let regular = !(le.1 >= self.max_evals_per_coin || (le.0 != 0 && t.m.slot < le.0 + self.eval_every_slots));
        if regular {
            *le = (t.m.slot, le.1 + 1);
        }
        let keep = self.regular_keep >= 1.0 || {
            let h = u64::from_le_bytes(t.mint.0[8..16].try_into().unwrap()) ^ t.m.slot.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            ((h >> 11) as f64 / (1u64 << 53) as f64) < self.regular_keep
        };
        if !tagged && !(regular && keep) {
            return;
        }
        let feats = features(mt, t.m.slot, &mut self.book);
        let curve = mt.curve;
        let fee = mt.fee_bps;
        let row_id = self.next_id;
        self.next_id += 1;
        let n = self.labels.len();
        let mut new = Vec::with_capacity(n);
        {
            let cx = self.cx();
            for (li, _) in self.labels.iter().enumerate() {
                let id = self.next_id + li as u64;
                let seed = u64::from_le_bytes(t.mint.0[..8].try_into().unwrap()) ^ t.m.slot.rotate_left(17) ^ (li as u64) << 50;
                let p = Position::new_entry(id, t.mint, li, true, self.stake, t.m.slot, t.m.tx_idx, &curve, fee, None, &cx, seed);
                new.push((id, p, li));
            }
        }
        self.next_id += n as u64;
        for (id, p, li) in new {
            self.positions.insert(id, (p, li, row_id));
            self.by_mint.entry(t.mint).or_default().push(id);
        }
        self.rows.insert(
            row_id,
            Acc {
                row: FlowRow { time: self.now_time, slot: t.m.slot, mint: t.mint, tag: tagged as u8, feats, res: vec![(f64::NAN, 0, "", false); n] },
                left: n,
            },
        );
    }

    fn check(&mut self, mint: Pk, slot: u64, tx: u32) {
        let Some(mt) = self.mints.get(&mint) else { return };
        let Some(ids) = self.by_mint.get(&mint) else { return };
        let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: self.buy_slip, sell_slip: 0.25, dump_sell_slip: 0.6 };
        for id in ids {
            if let Some((p, li, _)) = self.positions.get_mut(id) {
                p.check_exits_at(&self.labels[*li], mt, slot, tx, None, &cx);
            }
        }
    }

    fn on_block(&mut self, b: &BlockStat) {
        {
            let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: self.buy_slip, sell_slip: 0.25, dump_sell_slip: 0.6 };
            for (mint, ids) in self.by_mint.iter() {
                let Some(mt) = self.mints.get(mint) else { continue };
                for id in ids {
                    if let Some((p, li, _)) = self.positions.get_mut(id) {
                        if p.pending.as_ref().map(|o| o.lands_at_block_end(b.slot)).unwrap_or(false) {
                            p.fill(&mt.curve, mt.fee_bps, b.slot, &cx);
                        }
                        p.check_exits_at(&self.labels[*li], mt, b.slot + 1, u32::MAX, None, &cx);
                    }
                }
            }
        }
        for mt in self.mints.values_mut() {
            mt.on_block_end(b.slot);
        }
        // operator outcomes become known 10 minutes after launch (same as the brain)
        while let Some((mint, s0)) = self.peak_pending.first().copied() {
            if b.slot < s0 + secs(600) {
                break;
            }
            self.peak_pending.remove(0);
            if let Some(mt) = self.mints.get(&mint) {
                let (op, peak) = (mt.ctx.op, mt.peak_after_b0());
                self.book.record_peak(op, peak);
            }
        }
        // collect closed positions
        let closed: Vec<u64> = self.positions.iter().filter(|(_, (p, _, _))| p.state == PosState::Closed).map(|(k, _)| *k).collect();
        for id in closed {
            let (p, li, row) = self.positions.remove(&id).unwrap();
            if let Some(v) = self.by_mint.get_mut(&p.mint) {
                v.retain(|x| *x != id);
            }
            let net = (p.proceeds as f64 - p.cost as f64) / self.stake as f64;
            if let Some(acc) = self.rows.get_mut(&row) {
                acc.row.res[li] = (net, p.fixed_costs, p.exit_reason, p.failed_entry);
                acc.left -= 1;
                if acc.left == 0 {
                    let acc = self.rows.remove(&row).unwrap();
                    self.out.push(acc.row);
                }
            }
        }
        // prune finished coins
        if b.slot % 500 == 0 {
            let old: Vec<Pk> = self
                .mints
                .iter()
                .filter(|(k, m)| m.age_slots(b.slot) > self.max_age_slots + secs(300) && self.by_mint.get(*k).map(|v| v.is_empty()).unwrap_or(true))
                .map(|(k, _)| *k)
                .collect();
            for k in old {
                self.mints.remove(&k);
                self.by_mint.remove(&k);
                self.last_eval.remove(&k);
            }
        }
    }
}
