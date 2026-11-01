//! Signal lab: many independent entry mechanisms evaluated side by side on the same event
//! stream, each with several exit styles, all under the landing model and full costs.
//! One output row per (signal fire, exit style). Nothing here copy-trades: signals react to
//! market structure, launch structure and wallet-derived *context*, never to "wallet X bought".

use crate::book::OperatorBook;
use crate::event::*;
use crate::fill::{CostModel, LatencyModel};
use crate::mint::{DumpCfg, MintTracker};
use crate::pk::Pk;
use crate::policy::{DumpMode, Entry, ExecCtx, ExitRules, PosState, Position};
use crate::pump::{secs, TOKEN_SUPPLY};
use rustc_hash::{FxHashMap, FxHashSet};

pub const SIGNALS: &[&str] = &[
    "base_create",       // control: every coin at the create tx
    "base_b0",           // control: every coin at first-candle close
    "b0_supply55",       // FOMO: bot runs the coin when its bundle holds ≥ ~600M/1.07B
    "b0_fresh_bundle",   // FOMO: fresh-wallet bundles appear on coins they intend to run
    "b0_runmode",        // supply ≥ 45% AND fresh bundle
    "b0_bigbundle_lowcrowd",
    "b0_team_not_dev",   // Play-finder: the dev never buys, the dev team does
    "b0_exch_fresh_dev", // dev funded by an exchange with an exact non-round amount, recently
    "op_quality_create", // operator's past launches mostly reached 2×: snipe at create
    "op_quality_b0",     // same, at first-candle close
    "delay5_minmc",      // F-Project: snipe delay 5 s + minimum market cap
    "dip20",             // F-Project: buy the manipulated −20% candle after a pump
    "devsell_repump",    // F-Project: dev sells, insiders still hold, re-pump follows
    "sniper_exhaust",    // flipper wave over, buyers return
    "repump",            // −50% from peak, then fresh buying pressure
    "cluster_buy",       // ≥3 near-identical buys from distinct wallets in 2 slots (KOL/group)
    "cluster_late",      // same, after 20 s (KOL arrives after retail)
    "whale_impulse",     // one ≥3 SOL buy at a low market cap
    "uniq_surge",        // ≥15 unique buyers in 15 s with strong net inflow
    "quiet_accum",       // price flat for 30 s while net inflow is strong (absorption)
    "organic",           // dispersed holders, many unique buyers, positive flow
    "grad_sprint",       // progress crosses 80% with strong inflow
    "milestone50",       // progress crosses 50%
];

/// Parameter grid for the "run mode" family (bundle supply share × fresh-wallet share).
pub const RM_SUPPLY: [f64; 4] = [0.30, 0.40, 0.50, 0.60];
pub const RM_FRESH: [f64; 3] = [0.3, 0.5, 0.7];
pub const RM_WINDOW_S: [u64; 3] = [1800, 7200, 21600];
pub const RM_MIN_N: [usize; 2] = [2, 3];

pub fn signal_names() -> Vec<String> {
    let mut v: Vec<String> = SIGNALS.iter().map(|s| s.to_string()).collect();
    for trig in ["g0", "b0"] {
        for s in RM_SUPPLY {
            for f in RM_FRESH {
                for w in RM_WINDOW_S {
                    for n in RM_MIN_N {
                        v.push(format!("rm_{trig}_s{s}_f{f}_w{w}_n{n}"));
                    }
                }
            }
        }
    }
    v
}

/// Long-hold exits: the edge must dwarf a ~10% entry premium and 3–8 slot sell delays.
pub fn exit_grid_long() -> Vec<ExitRules> {
    let mut v = Vec::new();
    for tp in [0.5, 1.0, 2.0, 9.0] {
        for (ttag, trail) in [("notr", None), ("tr50", Some((0.5, 0.25))), ("tr100", Some((1.0, 0.4)))] {
            for sl in [-0.30, -0.50] {
                for hold in [15u64, 60, 180, 600] {
                    v.push(ExitRules {
                        name: format!("L_tp{}_{}_sl{}_h{hold}", if tp > 5.0 { "none".into() } else { format!("{tp}") }, ttag, -sl),
                        entry: Entry::Create,
                        tp: if tp > 5.0 { vec![] } else { vec![(tp, 1.0)] },
                        sl,
                        sl_arm_slots: 0,
                        trail,
                        max_hold_slots: secs(hold),
                        idle_slots: secs(60),
                        on_dump: DumpMode::Off,
                        dump_clock_q: None,
                        progress_exit: None,
                    });
                }
            }
        }
    }
    v
}

/// Exit set for the launch-structure family: hold/trail × dump reaction × SL × hold time.
pub fn exit_grid_lse() -> Vec<ExitRules> {
    let mut v = Vec::new();
    for (tag, trail) in [("hold", None), ("trail", Some((0.20, 0.10))), ("trail15", Some((0.15, 0.08)))] {
        for (dtag, dump) in [("off", DumpMode::Off), ("hard", DumpMode::Hard), ("all", DumpMode::All)] {
            for sl in [-0.10, -0.15] {
                for hold in [8u64, 15] {
                    v.push(ExitRules {
                        name: format!("lse_{tag}_{dtag}_sl{}_h{hold}", -sl),
                        entry: Entry::Create,
                        tp: vec![],
                        sl,
                        sl_arm_slots: 0,
                        trail,
                        max_hold_slots: secs(hold),
                        idle_slots: secs(15),
                        on_dump: dump,
                        dump_clock_q: None,
                        progress_exit: None,
                    });
                }
            }
        }
    }
    v
}

/// Dense exit grid for focused studies: TP × SL × trailing × max hold.
pub fn exit_grid() -> Vec<ExitRules> {
    let mut v = Vec::new();
    for tp in [0.15, 0.20, 0.30, 0.40, 9.0] {
        for sl in [-0.10, -0.15, -0.25] {
            for tr in [None, Some((0.15, 0.08)), Some((0.20, 0.10)), Some((0.30, 0.15))] {
                for hold in [15u64, 45] {
                    let name = format!(
                        "x_tp{}_sl{}_{}_h{hold}",
                        if tp > 5.0 { "none".to_string() } else { format!("{tp}") },
                        -sl,
                        tr.map(|(a, g)| format!("tr{a}-{g}")).unwrap_or("notr".into())
                    );
                    v.push(ExitRules {
                        name,
                        entry: Entry::Create,
                        tp: if tp > 5.0 { vec![] } else { vec![(tp, 1.0)] },
                        sl,
                        sl_arm_slots: 0,
                        trail: tr,
                        max_hold_slots: secs(hold),
                        idle_slots: secs(15),
                        on_dump: DumpMode::Off,
                        dump_clock_q: None,
                        progress_exit: None,
                    });
                }
            }
        }
    }
    v
}

pub fn exit_styles() -> Vec<ExitRules> {
    let r = |name: &str, tp: Vec<(f64, f64)>, sl: f64, trail: Option<(f64, f64)>, hold_s: u64, dump: DumpMode, pe: Option<f64>| ExitRules {
        name: name.into(),
        entry: Entry::Create,
        tp,
        sl,
        sl_arm_slots: 0,
        trail,
        max_hold_slots: secs(hold_s),
        idle_slots: secs(15),
        on_dump: dump,
        dump_clock_q: None,
        progress_exit: pe,
    };
    vec![
        r("tp20", vec![(0.20, 1.0)], -0.12, None, 15, DumpMode::Off, None),
        r("tp40", vec![(0.40, 1.0)], -0.20, None, 30, DumpMode::Off, None),
        r("tr20", vec![(1.00, 1.0)], -0.20, Some((0.20, 0.10)), 60, DumpMode::Off, None),
        r("q5", vec![], -0.25, None, 5, DumpMode::Off, None),
        r("tp30", vec![(0.30, 1.0)], -0.15, None, 20, DumpMode::Off, None),
        r("tp60t", vec![(0.60, 1.0)], -0.25, Some((0.30, 0.15)), 60, DumpMode::Off, None),
        r("tp60t_d", vec![(0.60, 1.0)], -0.25, Some((0.30, 0.15)), 60, DumpMode::Hard, None),
        r("tp150", vec![(1.50, 1.0)], -0.35, Some((0.60, 0.30)), 180, DumpMode::Off, None),
        r("grad", vec![], -0.25, Some((0.5, 0.25)), 240, DumpMode::Hard, Some(0.97)),
    ]
}

pub struct SigRow {
    pub signal: u16,
    pub exit: u8,
    pub mint: Pk,
    pub time: i64,
    pub slot: u64,
    pub age_s: f32,
    pub mcap: f32,
    pub net: f64,
    pub fixed: u64,
    pub failed: bool,
    pub reason: &'static str,
    pub hold_slots: u64,
    pub op: u32,
    pub dev: Pk,
    pub funder: Pk,
    pub how: u8,
    pub fund_kind: u8,
    pub op_created_before: u32,
    pub b0n: u32,
}

#[derive(Default, Clone)]
struct SigState {
    fired: FxHashSet<u16>,
    g0_done: bool,
    g0_end_tx: u32,
    b0_done: bool,
    b0_close_price: f64,
    peak: f64,
    dumped50: bool,
    prog50: bool,
    prog80: bool,
    last_heavy: u64,
    dev_sold_frac: f64,
    b0_tokens: i128,
}

pub struct SigLab {
    pub book: OperatorBook,
    pub mints: FxHashMap<Pk, MintTracker>,
    st: FxHashMap<Pk, SigState>,
    positions: FxHashMap<u64, (Position, u16, u8, i64, u64, f32, f32)>,
    by_mint: FxHashMap<Pk, Vec<u64>>,
    pending_create: Vec<(Pk, u64, u32)>,
    peak_pending: Vec<(Pk, u64)>,
    pub out: Vec<SigRow>,
    pub exits: Vec<ExitRules>,
    pub latency: LatencyModel,
    pub costs: CostModel,
    pub dump: DumpCfg,
    pub stake: u64,
    next_id: u64,
    now_time: i64,
    pub fires: Vec<u64>,
    pub names: Vec<String>,
    index: FxHashMap<String, u16>,
    /// Only fire signals whose name starts with one of these prefixes (empty = all).
    pub only: Vec<String>,
    /// Price limit for create-tx triggers (max adverse price increase; 1e9 = none).
    pub create_slip: f64,
}

impl SigLab {
    pub fn new(stake: u64, latency: LatencyModel) -> SigLab {
        SigLab {
            book: OperatorBook::default(),
            mints: FxHashMap::default(),
            st: FxHashMap::default(),
            positions: FxHashMap::default(),
            by_mint: FxHashMap::default(),
            pending_create: Vec::new(),
            peak_pending: Vec::new(),
            out: Vec::new(),
            exits: exit_styles(),
            latency,
            costs: CostModel::default(),
            dump: DumpCfg::default(),
            stake,
            next_id: 1,
            now_time: 0,
            fires: vec![0; signal_names().len()],
            index: signal_names().into_iter().enumerate().map(|(i, n)| (n, i as u16)).collect(),
            names: signal_names(),
            only: Vec::new(),
            create_slip: 2.0,
        }
    }

    fn fire(&mut self, name: &str, mint: Pk, slot: u64, tx: u32, slip: f64) {
        let Some(&sid) = self.index.get(name) else { return };
        if !self.only.is_empty() && !self.only.iter().any(|p| name.starts_with(p.as_str())) {
            return;
        }
        let Some(st) = self.st.get_mut(&mint) else { return };
        if !st.fired.insert(sid) {
            return;
        }
        let Some(mt) = self.mints.get(&mint) else { return };
        let curve = mt.curve;
        let fee = mt.fee_bps;
        let age = (mt.age_slots(slot) * crate::pump::SLOT_MS) as f32 / 1000.0;
        let mcap = curve.mcap_sol() as f32;
        self.fires[sid as usize] += 1;
        let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: slip, sell_slip: 0.25, dump_sell_slip: 0.6 };
        for (ei, _) in self.exits.iter().enumerate() {
            let id = self.next_id;
            self.next_id += 1;
            // landing draw depends on (coin, exit, trigger), not on which grid cell fired,
            // so grid cells that fire at the same trigger see the same landing
            let seed = u64::from_le_bytes(mint.0[..8].try_into().unwrap()) ^ ((ei as u64) << 52) ^ slot ^ (tx as u64) << 20;
            let p = Position::new_entry(id, mint, ei, true, self.stake, slot, tx, &curve, fee, None, &cx, seed);
            self.positions.insert(id, (p, sid, ei as u8, self.now_time, slot, age, mcap));
            self.by_mint.entry(mint).or_default().push(id);
        }
    }

    pub fn on_event(&mut self, ev: &Ev) {
        if !self.pending_create.is_empty() {
            let (s, t, _) = ev.order_key();
            let (_, ps, pt) = self.pending_create[0];
            if s != ps || t != pt {
                for (mint, slot, tx) in std::mem::take(&mut self.pending_create) {
                    self.at_create_end(mint, slot, tx);
                }
            }
        }
        if let Some(m) = ev.meta() {
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
            Ev::Create(c) => {
                let mut mt = MintTracker::new(c);
                if mt.tradable && !mt.mayhem {
                    let (op, how) = self.book.attribute_launch(c.user, c.m.slot);
                    mt.ctx = self.book.launch_ctx(op, how, &c.user, c.m.slot);
                    self.mints.insert(c.mint, mt);
                    self.st.insert(c.mint, SigState::default());
                    self.peak_pending.push((c.mint, c.m.slot));
                    self.pending_create.push((c.mint, c.m.slot, c.m.tx_idx));
                }
            }
            Ev::Trade(t) => self.on_trade(t),
            Ev::TokXfer(x) => {
                if let Some(mt) = self.mints.get_mut(&x.mint) {
                    let s = mt.on_xfer(x, &self.dump);
                    self.check(x.mint, x.m.slot, x.m.tx_idx, Some(s));
                }
            }
            Ev::Complete(c) => {
                if let Some(mt) = self.mints.get_mut(&c.mint) {
                    mt.completed = true;
                }
                self.check(c.mint, c.m.slot, c.m.tx_idx, None);
            }
            Ev::Block(b) => self.on_block(b),
        }
    }

    fn at_create_end(&mut self, mint: Pk, slot: u64, tx: u32) {
        self.fire("base_create", mint, slot, tx, self.create_slip);
        let Some(mt) = self.mints.get(&mint) else { return };
        let op = mt.ctx.op;
        let (n, _, f2) = self.book.peak_stats(op);
        if n >= 3 && f2 >= 0.5 {
            self.fire("op_quality_create", mint, slot, tx, self.create_slip);
        }
    }

    /// Evaluate the run-mode grid with the create-slot buyers seen so far.
    fn runmode(&mut self, trig: &str, mint: Pk, trig_slot: u64, trig_tx: u32) {
        let Some(mt) = self.mints.get(&mint) else { return };
        let cs = mt.create_slot;
        let supply = mt.insider_share();
        let b0n = mt.block0.len();
        let mut fresh = [0usize; 3];
        for w in mt.block0.keys() {
            if let Some((_, _, fs)) = self.book.funding_of(w) {
                for (i, win) in RM_WINDOW_S.iter().enumerate() {
                    if fs <= cs && fs + secs(*win) >= cs {
                        fresh[i] += 1;
                    }
                }
            }
        }
        let mut names = Vec::new();
        for s in RM_SUPPLY {
            if supply < s {
                continue;
            }
            for f in RM_FRESH {
                for (i, w) in RM_WINDOW_S.iter().enumerate() {
                    let fs = if b0n > 0 { fresh[i] as f64 / b0n as f64 } else { 0.0 };
                    if fs < f {
                        continue;
                    }
                    for n in RM_MIN_N {
                        if b0n >= n {
                            names.push(format!("rm_{trig}_s{s}_f{f}_w{w}_n{n}"));
                        }
                    }
                }
            }
        }
        for n in names {
            self.fire(&n, mint, trig_slot, trig_tx, 1.0);
        }
    }

    fn at_b0_end(&mut self, mint: Pk, slot: u64) {
        if !self.st.get(&mint).map(|s| s.g0_done).unwrap_or(true) {
            // the creator's group ran to the end of the slot
            let tx = self.st.get(&mint).map(|s| s.g0_end_tx).unwrap_or(u32::MAX);
            if let Some(st) = self.st.get_mut(&mint) {
                st.g0_done = true;
            }
            self.runmode("g0", mint, slot, tx);
        }
        self.runmode("b0", mint, slot + 1, u32::MAX);
        let Some(mt) = self.mints.get(&mint) else { return };
        let cs = mt.create_slot;
        let supply = mt.insider_share();
        let b0n = mt.block0.len();
        let b0_sol = mt.block0.values().sum::<u64>() as f64 / 1e9;
        let mut fresh = 0usize;
        for w in mt.block0.keys() {
            if let Some((_, _, fs)) = self.book.funding_of(w) {
                if fs <= cs && fs + secs(7200) >= cs {
                    fresh += 1;
                }
            }
        }
        let fresh_share = if b0n > 0 { fresh as f64 / b0n as f64 } else { 0.0 };
        let dev_buy = mt.dev_buy_sol as f64 / 1e9;
        let ctx = mt.ctx;
        let b0_tokens: i128 = mt.block0.keys().map(|w| mt.holdings.get(w).copied().unwrap_or(0).max(0)).sum();
        let price = mt.curve.price();
        let (n, _, f2) = self.book.peak_stats(ctx.op);
        if let Some(st) = self.st.get_mut(&mint) {
            st.b0_done = true;
            st.b0_close_price = price;
            st.peak = price;
            st.b0_tokens = b0_tokens;
        }
        let t = slot + 1;
        let u = u32::MAX;
        self.fire("base_b0", mint, t, u, 1.0);
        if supply >= 0.55 {
            self.fire("b0_supply55", mint, t, u, 1.0);
        }
        if fresh_share >= 0.6 && b0n >= 3 {
            self.fire("b0_fresh_bundle", mint, t, u, 1.0);
        }
        if supply >= 0.45 && fresh_share >= 0.5 && b0n >= 2 {
            self.fire("b0_runmode", mint, t, u, 1.0);
        }
        if b0_sol >= 5.0 && b0n.saturating_sub(fresh) <= 1 {
            self.fire("b0_bigbundle_lowcrowd", mint, t, u, 1.0);
        }
        if dev_buy < 0.001 && b0_sol >= 2.0 {
            self.fire("b0_team_not_dev", mint, t, u, 1.0);
        }
        if ctx.fund_kind == 2 && ctx.fund_age_s >= 0.0 && ctx.fund_age_s <= 1800.0 && dev_buy >= 1.0 {
            self.fire("b0_exch_fresh_dev", mint, t, u, 1.0);
        }
        if n >= 3 && f2 >= 0.5 {
            self.fire("op_quality_b0", mint, t, u, 1.0);
        }
    }

    fn on_trade(&mut self, t: &Trade) {
        // landings before this trade
        if let Some(ids) = self.by_mint.get(&t.mint).cloned() {
            if let Some(mt) = self.mints.get(&t.mint) {
                let (curve, fee) = (mt.curve, mt.fee_bps);
                let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: 1.0, sell_slip: 0.25, dump_sell_slip: 0.6 };
                for id in ids {
                    if let Some((p, ..)) = self.positions.get_mut(&id) {
                        if p.pending.as_mut().map(|o| o.lands_before_trade(t.m.slot, t.m.tx_idx)).unwrap_or(false) {
                            p.fill(&curve, fee, t.m.slot, &cx);
                        }
                    }
                }
            }
        }
        // creator's atomic group: consecutive txs from the create tx in the create slot
        let mut g0_close: Option<u32> = None;
        if let (Some(mt), Some(st)) = (self.mints.get(&t.mint), self.st.get_mut(&t.mint)) {
            if t.m.slot == mt.create_slot && !st.g0_done {
                if st.g0_end_tx == 0 {
                    st.g0_end_tx = mt.create_tx_idx;
                }
                if t.m.tx_idx <= st.g0_end_tx + 1 {
                    st.g0_end_tx = st.g0_end_tx.max(t.m.tx_idx);
                } else {
                    st.g0_done = true;
                    g0_close = Some(st.g0_end_tx);
                }
            }
        }
        if let Some(gtx) = g0_close {
            // decide on the group as seen *before* this later trade (it lands ahead of us)
            self.runmode("g0", t.mint, t.m.slot, gtx);
        }
        let Some(mt) = self.mints.get_mut(&t.mint) else { return };
        let sgn = mt.on_trade(t, &self.dump);
        self.check(t.mint, t.m.slot, t.m.tx_idx, Some(sgn));
        self.signals_on_trade(t);
    }

    fn signals_on_trade(&mut self, t: &Trade) {
        let Some(mt) = self.mints.get(&t.mint) else { return };
        let slot = t.m.slot;
        let tx = t.m.tx_idx;
        let age = mt.age_slots(slot);
        if mt.completed || age > secs(1800) {
            return;
        }
        let price = mt.curve.price();
        let prog = mt.curve.progress();
        let mcap = mt.curve.mcap_sol();
        let init_mcap = mt.init_price * TOKEN_SUPPLY as f64 / 1e9;
        // windowed stats
        let win = |w: u64| {
            let from = slot.saturating_sub(secs(w));
            let (mut net, mut nb, mut ns, mut flip, mut sells) = (0i64, 0u32, 0u32, 0u64, 0u64);
            let mut uniq: FxHashSet<[u8; 4]> = FxHashSet::default();
            for x in mt.recent.iter().rev() {
                if x.slot < from {
                    break;
                }
                if x.is_buy {
                    net += x.sol as i64;
                    nb += 1;
                    uniq.insert(x.user.0[..4].try_into().unwrap());
                } else {
                    net -= x.sol as i64;
                    ns += 1;
                    sells += x.sol;
                    if x.flip {
                        flip += x.sol;
                    }
                }
            }
            (net as f64 / 1e9, nb, ns, uniq.len(), if sells > 0 { flip as f64 / sells as f64 } else { 0.0 })
        };
        let first_price_since = |w: u64| {
            let from = slot.saturating_sub(secs(w));
            mt.recent.iter().find(|x| x.slot >= from).map(|x| x.price).unwrap_or(price)
        };
        let dev = mt.dev;
        let dev_max = mt.dev_max_hold.max(1);
        let dev_now = mt.holdings.get(&dev).copied().unwrap_or(0).max(0);
        let b0_now: i128 = mt.block0.keys().map(|w| mt.holdings.get(w).copied().unwrap_or(0).max(0)).sum();
        let in_b0 = mt.block0.contains_key(&t.user);
        let created = mt.create_slot;
        let st = self.st.get(&t.mint).cloned().unwrap_or_default();
        let mut fires: Vec<(&'static str, f64)> = Vec::new();

        // ---- simple state updates
        let mut st2 = st.clone();
        if st.b0_done {
            st2.peak = st.peak.max(price);
            if price <= 0.5 * st2.peak {
                st2.dumped50 = true;
            }
        }
        if mt.dev_max_hold > 0 {
            st2.dev_sold_frac = 1.0 - dev_now as f64 / dev_max as f64;
        }
        // ---- timing-based
        if age >= secs(5) && age <= secs(8) && mcap >= 1.5 * init_mcap {
            fires.push(("delay5_minmc", 0.3));
        }
        if st.b0_done && age <= secs(60) && st2.peak >= 1.4 * st.b0_close_price && price <= 0.8 * st2.peak {
            fires.push(("dip20", 0.3));
        }
        if !t.is_buy && t.user == dev && age <= secs(30) && st2.dev_sold_frac >= 0.5 && st.b0_tokens > 0 && b0_now as f64 >= 0.7 * st.b0_tokens as f64 {
            fires.push(("devsell_repump", 0.3));
        }
        if slot > created {
            let heavy_due = slot >= st.last_heavy + secs(1);
            if heavy_due {
                st2.last_heavy = slot;
                let (n5, b5, _, u5, _) = win(5);
                let (n15, _b15, _s15, u15, flip15) = win(15);
                if st.b0_done && age <= secs(120) && price <= 0.7 * st2.peak && flip15 >= 0.5 && n5 > 0.0 && b5 >= 3 {
                    fires.push(("sniper_exhaust", 0.3));
                }
                if st2.dumped50 && age <= secs(600) && n5 >= 1.0 && b5 >= 3 && u5 >= 3 {
                    fires.push(("repump", 0.3));
                }
                if age >= secs(10) && age <= secs(120) && u15 >= 15 && n15 >= 3.0 {
                    fires.push(("uniq_surge", 0.3));
                }
                if age >= secs(30) {
                    let (n30, b30, _, _, _) = win(30);
                    let p30 = first_price_since(30);
                    if (price / p30 - 1.0).abs() <= 0.10 && n30 >= 2.0 && b30 >= 10 {
                        fires.push(("quiet_accum", 0.3));
                    }
                }
                if age >= secs(60) {
                    let (n60, _, _, u60, _) = win(60);
                    if u60 >= 25 && n60 > 0.0 {
                        let top1 = mt.holdings.values().copied().filter(|h| *h > 0).max().unwrap_or(0) as f64 / TOKEN_SUPPLY as f64;
                        if top1 < 0.05 {
                            fires.push(("organic", 0.3));
                        }
                    }
                }
                if !st.prog80 && prog >= 0.8 {
                    st2.prog80 = true;
                    if n15 >= 3.0 {
                        fires.push(("grad_sprint", 0.3));
                    }
                }
            }
            if !st.prog50 && prog >= 0.5 {
                st2.prog50 = true;
                fires.push(("milestone50", 0.3));
            }
            // cluster of near-identical buys from distinct wallets (group / KOL bundle)
            if t.is_buy && t.sol >= 200_000_000 {
                let lo = (t.sol as f64 * 0.9) as u64;
                let hi = (t.sol as f64 * 1.1) as u64;
                let mut users: Vec<[u8; 4]> = Vec::new();
                for x in mt.recent.iter().rev() {
                    if x.slot + 2 < slot {
                        break;
                    }
                    if x.is_buy && x.sol >= lo && x.sol <= hi {
                        let k: [u8; 4] = x.user.0[..4].try_into().unwrap();
                        if !users.contains(&k) {
                            users.push(k);
                        }
                    }
                }
                if users.len() >= 3 {
                    fires.push(("cluster_buy", 0.3));
                    if age >= secs(20) {
                        fires.push(("cluster_late", 0.3));
                    }
                }
            }
            if t.is_buy && t.sol >= 3_000_000_000 && !in_b0 && t.user != dev && age >= secs(3) && mcap <= 60.0 {
                fires.push(("whale_impulse", 0.3));
            }
        }
        self.st.insert(t.mint, st2);
        for (name, slip) in fires {
            self.fire(name, t.mint, slot, tx, slip);
        }
    }

    fn check(&mut self, mint: Pk, slot: u64, tx: u32, s: Option<crate::mint::Signals>) {
        let Some(mt) = self.mints.get(&mint) else { return };
        let Some(ids) = self.by_mint.get(&mint) else { return };
        let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: 1.0, sell_slip: 0.25, dump_sell_slip: 0.6 };
        for id in ids {
            if let Some((p, _, ei, ..)) = self.positions.get_mut(id) {
                p.check_exits_at(&self.exits[*ei as usize], mt, slot, tx, s, &cx);
            }
        }
    }

    fn on_block(&mut self, b: &BlockStat) {
        for (mint, slot, tx) in std::mem::take(&mut self.pending_create) {
            self.at_create_end(mint, slot, tx);
        }
        {
            let cx = ExecCtx { latency: &self.latency, costs: &self.costs, buy_slip: 1.0, sell_slip: 0.25, dump_sell_slip: 0.6 };
            for (mint, ids) in self.by_mint.iter() {
                let Some(mt) = self.mints.get(mint) else { continue };
                for id in ids {
                    if let Some((p, _, ei, ..)) = self.positions.get_mut(id) {
                        if p.pending.as_ref().map(|o| o.lands_at_block_end(b.slot)).unwrap_or(false) {
                            p.fill(&mt.curve, mt.fee_bps, b.slot, &cx);
                        }
                        p.check_exits_at(&self.exits[*ei as usize], mt, b.slot + 1, u32::MAX, None, &cx);
                    }
                }
            }
        }
        let b0: Vec<Pk> = self.mints.iter().filter(|(_, m)| m.create_slot == b.slot).map(|(k, _)| *k).collect();
        for mt in self.mints.values_mut() {
            mt.on_block_end(b.slot);
        }
        for mint in b0 {
            self.at_b0_end(mint, b.slot);
        }
        // operator outcomes become known 10 minutes after launch
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
        let closed: Vec<u64> = self.positions.iter().filter(|(_, v)| v.0.state == PosState::Closed).map(|(k, _)| *k).collect();
        for id in closed {
            let (p, sid, ei, time, slot, age, mcap) = self.positions.remove(&id).unwrap();
            if let Some(v) = self.by_mint.get_mut(&p.mint) {
                v.retain(|x| *x != id);
            }
            self.out.push(SigRow {
                signal: sid,
                exit: ei,
                mint: p.mint,
                time,
                slot,
                age_s: age,
                mcap,
                net: (p.proceeds as f64 - p.cost as f64) / self.stake as f64,
                fixed: p.fixed_costs,
                failed: p.failed_entry,
                reason: p.exit_reason,
                hold_slots: p.close_slot.saturating_sub(p.fill_slot),
                op: self.mints.get(&p.mint).map(|m| self.book.find(m.ctx.op)).unwrap_or(0),
                dev: self.mints.get(&p.mint).map(|m| m.dev).unwrap_or_default(),
                funder: self.mints.get(&p.mint).and_then(|m| self.book.funding_of(&m.dev)).map(|f| f.0).unwrap_or_default(),
                how: self.mints.get(&p.mint).map(|m| m.ctx.how).unwrap_or(0),
                fund_kind: self.mints.get(&p.mint).map(|m| m.ctx.fund_kind).unwrap_or(0),
                op_created_before: self.mints.get(&p.mint).map(|m| m.ctx.op_created_before).unwrap_or(0),
                b0n: self.mints.get(&p.mint).map(|m| m.block0.len() as u32).unwrap_or(0),
            });
        }
        if b.slot % 500 == 0 {
            let old: Vec<Pk> = self
                .mints
                .iter()
                .filter(|(k, m)| m.age_slots(b.slot) > secs(2400) && self.by_mint.get(*k).map(|v| v.is_empty()).unwrap_or(true))
                .map(|(k, _)| *k)
                .collect();
            for k in old {
                self.mints.remove(&k);
                self.st.remove(&k);
                self.by_mint.remove(&k);
            }
        }
    }
}
