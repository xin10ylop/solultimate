//! Operator book: who is behind each launch, and how following them has worked out.
//!
//! * Identity graph (union-find) over wallets and "funding signatures". Links are only
//!   added from information that exists *before* the launch: dev wallet reuse, the dev's
//!   first funding (non-hub sender = treasury / previous dev / mother), and for exchange
//!   hot wallets the (exchange, exact non-round amount) signature.
//! * Recurring create-slot buyers become insider wallets of an operator.
//! * Every launch is shadow-traded with each exit variant; per operator we keep the
//!   *selection-honest* track record (return of the variant chosen before each launch) and
//!   gate real entries on a shrunk lower confidence bound of that record.

use crate::pk::Pk;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKey {
    Wallet(Pk),
    /// (hub sender, amount in units of 0.0001 SOL)
    FundSig(Pk, u64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GateCfg {
    /// Minimum resolved launches with an honest result.
    pub min_n: usize,
    /// Required shrunk lower confidence bound of mean return per trade.
    pub min_lcb: f64,
    pub z: f64,
    /// Pseudo-observations of the population prior.
    pub prior_n: f64,
    pub min_win_rate: f64,
    /// Mean of the last `recent_k` honest results must exceed this.
    pub recent_k: usize,
    pub min_recent_mean: f64,
    /// Max consecutive honest losses before an operator is benched.
    pub max_consec_losses: usize,
    /// Only use launches within this many slots (operators change their setup).
    pub lookback_slots: u64,
    /// Variant used when an operator has no history yet.
    pub default_variant: usize,
    /// Entry price limit: allow up to this multiple of the operator's p75 first-candle mcap.
    pub first_candle_mult: f64,
}

impl Default for GateCfg {
    fn default() -> Self {
        GateCfg {
            min_n: 6,
            min_lcb: 0.03,
            z: 1.0,
            prior_n: 4.0,
            min_win_rate: 0.35,
            recent_k: 4,
            min_recent_mean: 0.0,
            max_consec_losses: 4,
            lookback_slots: 7 * crate::pump::SLOTS_PER_DAY,
            default_variant: 3,
            first_candle_mult: 1.35,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchRec {
    pub mint: Pk,
    pub slot: u64,
    /// Shadow return per variant (NaN = not run).
    pub res: Vec<f32>,
    /// Return of the variant selected *before* this launch (selection-honest).
    pub honest: f32,
    pub chosen: usize,
    /// Slots from create to the first dump signal.
    pub dump_slots: Option<u32>,
    pub block0_end_mcap: f32,
    pub crowd: u16,
    pub block0_sol: f32,
}

#[derive(Clone, Debug, Default)]
pub struct OpStats {
    pub launches: Vec<LaunchRec>,
    pub n_created: u32,
    pub first_slot: u64,
    pub last_slot: u64,
    pub devs: u32,
    /// Insider wallet → number of this operator's launches it bought in the create slot.
    pub insiders: FxHashMap<Pk, u16>,
    /// Outcomes of resolved launches: count, sum of ln(peak after first candle), count ≥ 2×.
    pub peak_n: u32,
    pub peak_sum: f64,
    pub peak_2x: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct GateDecision {
    pub pass: bool,
    pub variant: usize,
    pub n: usize,
    pub mean: f64,
    pub lcb: f64,
    pub win_rate: f64,
    pub reason: &'static str,
    pub max_mcap: Option<f64>,
    pub clock_slots: Option<u64>,
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i]
}

impl OpStats {
    fn recent(&self, now: u64, lookback: u64) -> impl Iterator<Item = &LaunchRec> {
        self.launches.iter().filter(move |l| now.saturating_sub(l.slot) <= lookback)
    }

    /// Variant with the best shrunk mean over recent launches (among `allowed`, if given).
    pub fn best_variant(&self, now: u64, cfg: &GateCfg, prior: f64, nvar: usize) -> usize {
        self.best_variant_in(now, cfg, prior, nvar, None)
    }
    pub fn best_variant_in(&self, now: u64, cfg: &GateCfg, prior: f64, nvar: usize, allowed: Option<&[usize]>) -> usize {
        let mut best = (allowed.and_then(|a| a.first().copied()).unwrap_or(cfg.default_variant), f64::NEG_INFINITY);
        for v in 0..nvar {
            if let Some(a) = allowed {
                if !a.contains(&v) {
                    continue;
                }
            }
            let xs: Vec<f64> = self
                .recent(now, cfg.lookback_slots)
                .filter_map(|l| l.res.get(v).copied())
                .filter(|x| x.is_finite())
                .map(|x| x as f64)
                .collect();
            if xs.is_empty() {
                continue;
            }
            // rank playbooks by the same risk-adjusted bound the gate uses, so the
            // operator's record favours consistent playbooks over lottery tickets
            let n = xs.len() as f64;
            let mean = xs.iter().sum::<f64>() / n;
            let sd = if xs.len() > 1 { (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt() } else { 0.5 };
            let shrunk = (mean * n + cfg.prior_n * prior) / (n + cfg.prior_n);
            let m = shrunk - cfg.z * sd / n.sqrt();
            if m > best.1 {
                best = (v, m);
            }
        }
        best.0
    }

    pub fn gate(&self, now: u64, cfg: &GateCfg, prior: f64, nvar: usize) -> GateDecision {
        self.gate_in(now, cfg, prior, nvar, None)
    }
    pub fn gate_in(&self, now: u64, cfg: &GateCfg, prior: f64, nvar: usize, allowed: Option<&[usize]>) -> GateDecision {
        let variant = self.best_variant_in(now, cfg, prior, nvar, allowed);
        let hs: Vec<f64> = self
            .recent(now, cfg.lookback_slots)
            .map(|l| l.honest as f64)
            .filter(|x| x.is_finite())
            .collect();
        let n = hs.len();
        let mut d = GateDecision {
            pass: false,
            variant,
            n,
            mean: f64::NAN,
            lcb: f64::NAN,
            win_rate: f64::NAN,
            reason: "",
            max_mcap: None,
            clock_slots: None,
        };
        let mut fc: Vec<f64> = self
            .recent(now, cfg.lookback_slots)
            .map(|l| l.block0_end_mcap as f64)
            .filter(|x| x.is_finite() && *x > 0.0)
            .collect();
        if fc.len() >= 3 {
            d.max_mcap = Some(quantile(&mut fc, 0.75) * cfg.first_candle_mult);
        }
        let mut dumps: Vec<f64> = self
            .recent(now, cfg.lookback_slots)
            .filter_map(|l| l.dump_slots.map(|x| x as f64))
            .collect();
        if dumps.len() >= 4 {
            d.clock_slots = Some(quantile(&mut dumps, 0.30).max(1.0) as u64);
        }
        if n < cfg.min_n {
            d.reason = "few_launches";
            return d;
        }
        let nf = n as f64;
        let mean = hs.iter().sum::<f64>() / nf;
        let var = hs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (nf - 1.0).max(1.0);
        let shrunk = (mean * nf + prior * cfg.prior_n) / (nf + cfg.prior_n);
        let lcb = shrunk - cfg.z * var.sqrt() / nf.sqrt();
        let wins = hs.iter().filter(|x| **x > 0.0).count() as f64 / nf;
        d.mean = mean;
        d.lcb = lcb;
        d.win_rate = wins;
        let k = cfg.recent_k.min(n);
        let recent_mean = hs[n - k..].iter().sum::<f64>() / k as f64;
        let mut consec = 0;
        for x in hs.iter().rev() {
            if *x <= 0.0 {
                consec += 1;
            } else {
                break;
            }
        }
        d.reason = if lcb < cfg.min_lcb {
            "low_lcb"
        } else if wins < cfg.min_win_rate {
            "low_win_rate"
        } else if recent_mean < cfg.min_recent_mean {
            "recent_decay"
        } else if consec >= cfg.max_consec_losses {
            "loss_streak"
        } else {
            d.pass = true;
            "pass"
        };
        d
    }
}

#[derive(Clone, Debug)]
struct FundRec {
    from: Pk,
    lamports: u64,
    slot: u64,
}

#[derive(Clone, Debug, Default)]
struct FunderStat {
    fresh: f64,
    last_decay: u64,
}

pub struct OperatorBook {
    parent: Vec<u32>,
    nodes: FxHashMap<NodeKey, u32>,
    pub stats: FxHashMap<u32, OpStats>,
    funding: FxHashMap<Pk, FundRec>,
    /// funder → wallets it freshly funded (wallet, slot), same TTL as `funding`
    funded: FxHashMap<Pk, Vec<(Pk, u64)>>,
    funders: FxHashMap<Pk, FunderStat>,
    /// insider wallet → (operator root at last update, count, total create-slot appearances)
    insider_index: FxHashMap<Pk, (u32, u16, u16)>,
    pub hub_min_fresh: f64,
    pub funding_ttl_slots: u64,
    last_prune: u64,
    pub static_hubs: rustc_hash::FxHashSet<Pk>,
}

impl Default for OperatorBook {
    fn default() -> Self {
        OperatorBook {
            parent: Vec::new(),
            nodes: FxHashMap::default(),
            stats: FxHashMap::default(),
            funding: FxHashMap::default(),
            funded: FxHashMap::default(),
            funders: FxHashMap::default(),
            insider_index: FxHashMap::default(),
            hub_min_fresh: 40.0,
            funding_ttl_slots: crate::pump::SLOTS_PER_DAY,
            last_prune: 0,
            static_hubs: Default::default(),
        }
    }
}

/// Amount looks hand-picked by a script (not a round human number).
pub fn non_round(lamports: u64) -> bool {
    lamports % 100_000_000 != 0 // not a multiple of 0.1 SOL
}

impl OperatorBook {
    fn node(&mut self, k: NodeKey) -> u32 {
        if let Some(i) = self.nodes.get(&k) {
            return *i;
        }
        let i = self.parent.len() as u32;
        self.parent.push(i);
        self.nodes.insert(k, i);
        i
    }
    pub fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let p = self.parent[x as usize];
            self.parent[x as usize] = self.parent[p as usize];
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) -> u32 {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return ra;
        }
        // keep the root that has more history
        let (keep, gone) = {
            let la = self.stats.get(&ra).map(|s| s.launches.len() + s.n_created as usize).unwrap_or(0);
            let lb = self.stats.get(&rb).map(|s| s.launches.len() + s.n_created as usize).unwrap_or(0);
            if la >= lb { (ra, rb) } else { (rb, ra) }
        };
        self.parent[gone as usize] = keep;
        if let Some(g) = self.stats.remove(&gone) {
            let k = self.stats.entry(keep).or_default();
            k.launches.extend(g.launches);
            k.launches.sort_by_key(|l| l.slot);
            k.n_created += g.n_created;
            k.devs += g.devs;
            k.first_slot = if k.first_slot == 0 { g.first_slot } else { k.first_slot.min(g.first_slot.max(1)) };
            k.last_slot = k.last_slot.max(g.last_slot);
            for (w, c) in g.insiders {
                *k.insiders.entry(w).or_default() += c;
            }
            k.peak_n += g.peak_n;
            k.peak_sum += g.peak_sum;
            k.peak_2x += g.peak_2x;
        }
        keep
    }

    /// Merge two operators (e.g. shared recurring insider wallets). Returns the new root.
    pub fn merge_ops(&mut self, a: u32, b: u32) -> u32 {
        self.union(a, b)
    }

    pub fn is_hub(&self, w: &Pk) -> bool {
        self.static_hubs.contains(w) || self.funders.get(w).map(|f| f.fresh >= self.hub_min_fresh).unwrap_or(false)
    }

    /// Record a fresh-wallet funding (call for every `Fund` event with to_pre == 0).
    pub fn on_fund(&mut self, from: Pk, to: Pk, lamports: u64, slot: u64) {
        let f = self.funders.entry(from).or_default();
        // exponential decay with ~1 day half-life
        if f.last_decay == 0 {
            f.last_decay = slot;
        }
        let dt = slot.saturating_sub(f.last_decay);
        if dt > 9_000 {
            f.fresh *= 0.5f64.powf(dt as f64 / crate::pump::SLOTS_PER_DAY as f64);
            f.last_decay = slot;
        }
        f.fresh += 1.0;
        if !self.funding.contains_key(&to) {
            self.funding.insert(to, FundRec { from, lamports, slot });
            self.funded.entry(from).or_default().push((to, slot));
        }
        if slot.saturating_sub(self.last_prune) > 9_000 {
            self.last_prune = slot;
            let ttl = self.funding_ttl_slots;
            self.funding.retain(|_, r| slot.saturating_sub(r.slot) <= ttl);
            self.funded.retain(|_, v| {
                v.retain(|(_, s)| slot.saturating_sub(*s) <= ttl);
                !v.is_empty()
            });
            self.funders.retain(|_, f| f.fresh > 0.05 || slot.saturating_sub(f.last_decay) < crate::pump::SLOTS_PER_DAY);
        }
    }

    /// Attribute a new launch by `dev` at `slot`. Returns (operator root, how it was linked).
    pub fn attribute_launch(&mut self, dev: Pk, slot: u64) -> (u32, &'static str) {
        let dn = self.node(NodeKey::Wallet(dev));
        let dr = self.find(dn);
        let known_dev = self.stats.contains_key(&dr);
        let mut how = if known_dev { "dev_reuse" } else { "new" };
        if let Some(fr) = self.funding.get(&dev).cloned() {
            if fr.slot <= slot {
                let link = if self.is_hub(&fr.from) {
                    if non_round(fr.lamports) {
                        Some(NodeKey::FundSig(fr.from, fr.lamports / 100_000))
                    } else {
                        None
                    }
                } else {
                    Some(NodeKey::Wallet(fr.from))
                };
                if let Some(k) = link {
                    let existed = self.nodes.contains_key(&k);
                    let fnode = self.node(k);
                    let fr_root = self.find(fnode);
                    let had_stats = existed && self.stats.contains_key(&fr_root);
                    self.union(dn, fnode);
                    if had_stats {
                        how = match k {
                            NodeKey::FundSig(..) => "exchange_amount",
                            NodeKey::Wallet(_) => "funder",
                        };
                    }
                    // second hop: the funder's own funder (mother → sub-mother → dev)
                    if let NodeKey::Wallet(f) = k {
                        if let Some(fr2) = self.funding.get(&f).cloned() {
                            if !self.is_hub(&fr2.from) {
                                let n2 = self.node(NodeKey::Wallet(fr2.from));
                                let r2 = self.find(n2);
                                let had2 = self.stats.contains_key(&r2);
                                self.union(fnode, n2);
                                if had2 && how == "new" {
                                    how = "funder2";
                                }
                            }
                        }
                    }
                }
            }
        }
        let root = self.find(dn);
        let s = self.stats.entry(root).or_default();
        s.n_created += 1;
        if !known_dev {
            s.devs += 1;
        }
        if s.first_slot == 0 {
            s.first_slot = slot;
        }
        s.last_slot = slot;
        (root, how)
    }

    /// Wallets freshly funded by `funder` in [from_slot, to_slot].
    pub fn funded_by(&self, funder: &Pk, from_slot: u64, to_slot: u64) -> impl Iterator<Item = &Pk> {
        self.funded.get(funder).into_iter().flatten().filter(move |(_, s)| *s >= from_slot && *s <= to_slot).map(|(w, _)| w)
    }

    /// First fresh-wallet funding seen for `w`: (sender, lamports, slot).
    pub fn funding_of(&self, w: &Pk) -> Option<(Pk, u64, u64)> {
        self.funding.get(w).map(|f| (f.from, f.lamports, f.slot))
    }

    /// How the dev wallet was funded: 0 unseen, 1 hub round amount, 2 hub non-round amount,
    /// 3 non-hub sender (treasury / mother / previous dev).
    pub fn funding_kind(&self, dev: &Pk) -> u8 {
        match self.funding.get(dev) {
            None => 0,
            Some(f) if self.is_hub(&f.from) => {
                if non_round(f.lamports) {
                    2
                } else {
                    1
                }
            }
            Some(_) => 3,
        }
    }

    /// Operator root for an existing wallet, if it has one.
    pub fn op_of_wallet(&mut self, w: &Pk) -> Option<u32> {
        let n = *self.nodes.get(&NodeKey::Wallet(*w))?;
        let r = self.find(n);
        self.stats.contains_key(&r).then_some(r)
    }

    /// Strong insider of an operator: seen in ≥2 of its launches and ≥70% of all its create-slot appearances.
    pub fn insider_op(&mut self, w: &Pk) -> Option<u32> {
        let (op, c, tot) = *self.insider_index.get(w)?;
        if c >= 2 && c as f64 >= 0.7 * tot as f64 {
            let r = self.find(op);
            return Some(r);
        }
        None
    }

    pub fn insider_wallets(&mut self, op: u32) -> Vec<Pk> {
        let r = self.find(op);
        let Some(s) = self.stats.get(&r) else { return vec![] };
        let cands: Vec<(Pk, u16)> = s.insiders.iter().filter(|(_, c)| **c >= 2).map(|(w, c)| (*w, *c)).collect();
        let mut out = Vec::new();
        for (w, c) in cands {
            if let Some((_, _, tot)) = self.insider_index.get(&w) {
                if c as f64 >= 0.7 * *tot as f64 {
                    out.push(w);
                }
            }
        }
        out
    }

    /// Store a resolved launch.
    pub fn record(&mut self, op: u32, rec: LaunchRec, block0_buyers: &[Pk]) {
        let r = self.find(op);
        for w in block0_buyers {
            let e0 = self.insider_index.entry(*w).or_insert((r, 0, 0)).0;
            let er = self.find(e0);
            let e = self.insider_index.get_mut(w).unwrap();
            if er == r {
                e.0 = r;
                e.1 += 1;
            }
            e.2 += 1;
        }
        let s = self.stats.entry(r).or_default();
        for w in block0_buyers {
            *s.insiders.entry(*w).or_default() += 1;
        }
        s.launches.push(rec);
        if s.launches.len() > 400 {
            s.launches.drain(..100);
        }
    }

    /// Record a resolved launch's peak (after the first candle) for its operator.
    pub fn record_peak(&mut self, op: u32, peak: f64) {
        let r = self.find(op);
        let s = self.stats.entry(r).or_default();
        s.peak_n += 1;
        s.peak_sum += peak.max(1.0).ln();
        if peak >= 2.0 {
            s.peak_2x += 1;
        }
    }

    /// (resolved launches, mean ln peak, share that reached 2×) for an operator.
    pub fn peak_stats(&mut self, op: u32) -> (u32, f64, f64) {
        let r = self.find(op);
        match self.stats.get(&r) {
            Some(s) if s.peak_n > 0 => (s.peak_n, s.peak_sum / s.peak_n as f64, s.peak_2x as f64 / s.peak_n as f64),
            _ => (0, 0.0, 0.0),
        }
    }

    /// Build the wallet-derived context for a launch attributed with `attribute_launch`.
    pub fn launch_ctx(&mut self, op: u32, how: &str, dev: &Pk, slot: u64) -> crate::mint::LaunchCtx {
        let fund = self.funding_of(dev).filter(|f| f.2 <= slot);
        let created = self.stats_of(op).map(|s| s.n_created).unwrap_or(1);
        crate::mint::LaunchCtx {
            op,
            how: crate::mint::how_code(how),
            fund_kind: self.funding_kind(dev),
            fund_sol: fund.map(|f| f.1 as f32 / 1e9).unwrap_or(0.0),
            fund_age_s: fund.map(|f| (slot - f.2) as f32 * crate::pump::SLOT_MS as f32 / 1000.0).unwrap_or(-1.0),
            op_created_before: created.saturating_sub(1),
        }
    }

    pub fn stats_of(&mut self, op: u32) -> Option<&OpStats> {
        let r = self.find(op);
        self.stats.get(&r)
    }

    pub fn n_operators_with(&self, min_launches: usize) -> usize {
        self.stats.values().filter(|s| s.launches.len() >= min_launches).count()
    }
}
