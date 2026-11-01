//! Research lab: data checks, causal replay (backtest) and reports.
//!
//!   lab check  --data data/pilot                 decoder + curve-continuity validation
//!   lab replay --data data/pilot --out reports/x  full causal replay through the brain
//!              [--config cfg.toml] [--stress 0|1|2|3] [--no-mayhem]

use anyhow::Result;
use clap::{Parser, Subcommand};
use rustc_hash::FxHashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use ucore::brain::{Brain, BrainCfg, LaunchRow};
use ucore::curve::Curve;
use ucore::event::Ev;
use ucore::fill::LatencyModel;
use ucore::pk::Pk;
use ucore::policy::ClosedTrade;
use ucore::store::StoreReader;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Check {
        #[arg(long)]
        data: PathBuf,
    },
    /// Compare live fills with the model's shadow prediction for the same launch+variant.
    Compare {
        #[arg(long, default_value = "data/journal.jsonl")]
        journal: PathBuf,
    },
    /// Run a grid of gate / latency settings over the same data in one pass.
    Sweep {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        from_slot: u64,
        #[arg(long, default_value_t = u64::MAX)]
        to_slot: u64,
    },
    Replay {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        stress: u32,
        #[arg(long)]
        no_mayhem: bool,
        /// Print the effective config as TOML and exit.
        #[arg(long)]
        dump_config: bool,
        /// Only replay blocks with slot in [from_slot, to_slot).
        #[arg(long, default_value_t = 0)]
        from_slot: u64,
        #[arg(long, default_value_t = u64::MAX)]
        to_slot: u64,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Check { data } => check(data),
        Cmd::Compare { journal } => compare(journal),
        Cmd::Sweep { data, out, from_slot, to_slot } => sweep(data, out, from_slot, to_slot),
        Cmd::Replay { data, out, config, stress, no_mayhem, dump_config, from_slot, to_slot } => {
            let mut cfg: BrainCfg = match config {
                Some(p) => toml::from_str(&fs::read_to_string(p)?)?,
                None => BrainCfg::default(),
            };
            if stress > 0 {
                cfg.latency = LatencyModel::stressed(stress);
            }
            if no_mayhem {
                cfg.shadow_mayhem = false;
            }
            if dump_config {
                println!("{}", toml::to_string_pretty(&cfg)?);
                return Ok(());
            }
            replay(data, out, cfg, from_slot, to_slot)
        }
    }
}

fn check(data: PathBuf) -> Result<()> {
    let mut last: FxHashMap<Pk, Curve> = FxHashMap::default();
    let (mut blocks, mut trades, mut creates, mut ok, mut bad, mut unknown, mut funds, mut xfers) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut first_slot, mut last_slot, mut prev_slot, mut gaps) = (u64::MAX, 0u64, 0u64, 0u64);
    let mut by_ix: FxHashMap<u8, u64> = FxHashMap::default();
    let mut mayhem = 0u64;
    let mut examples = Vec::new();
    let mut fee_bps_hist: FxHashMap<u32, u64> = FxHashMap::default();
    let mut bad_kind: FxHashMap<(bool, bool, u8), u64> = FxHashMap::default();
    let (mut xfer_pda, mut xfer_big) = (0u64, 0u64);
    let mut xfer_by_mint: FxHashMap<Pk, u64> = FxHashMap::default();
    let mut rd = StoreReader::open(&data)?;
    for blk in rd.by_ref() {
        for e in &blk {
            match e {
                Ev::Block(b) => {
                    blocks += 1;
                    first_slot = first_slot.min(b.slot);
                    last_slot = last_slot.max(b.slot);
                    if prev_slot != 0 && b.slot > prev_slot + 1 {
                        gaps += b.slot - prev_slot - 1;
                    }
                    prev_slot = b.slot;
                }
                Ev::Create(c) => {
                    creates += 1;
                    if c.mayhem {
                        mayhem += 1;
                    }
                    last.insert(c.mint, Curve { vsol: c.vsol, vtok: c.vtok, rsol: 0, rtok: c.rtok });
                }
                Ev::Trade(t) => {
                    trades += 1;
                    *by_ix.entry(t.ix).or_default() += 1;
                    *fee_bps_hist.entry(t.fee_bps).or_default() += 1;
                    // reconstruct the pre-state from this event and compare to the previous post-state
                    let pre = if t.is_buy {
                        Curve { vsol: t.vsol - t.sol, vtok: t.vtok + t.tok, rsol: t.rsol.saturating_sub(t.sol), rtok: t.rtok + t.tok }
                    } else {
                        Curve { vsol: t.vsol + t.sol, vtok: t.vtok - t.tok, rsol: t.rsol + t.sol, rtok: t.rtok - t.tok }
                    };
                    match last.get(&t.mint) {
                        Some(prev) => {
                            if prev.vsol == pre.vsol && prev.vtok == pre.vtok {
                                ok += 1;
                            } else {
                                bad += 1;
                                *bad_kind.entry((t.is_buy, t.mayhem, t.ix)).or_default() += 1;
                                if examples.len() < 8 {
                                    examples.push(format!(
                                        "{} slot {} tx {} ix {} buy {} mayhem {} solq {} sol {} tok {} fee {} bps {} | prev vsol {} vtok {} rsol {} | post vsol {} vtok {} rsol {} | sig {}",
                                        t.mint, t.m.slot, t.m.tx_idx, t.ix, t.is_buy, t.mayhem, t.sol_quote, t.sol, t.tok, t.fee, t.fee_bps, prev.vsol, prev.vtok, prev.rsol, t.vsol, t.vtok, t.rsol, t.m.sig
                                    ));
                                }
                            }
                        }
                        None => unknown += 1,
                    }
                    last.insert(t.mint, Curve { vsol: t.vsol, vtok: t.vtok, rsol: t.rsol, rtok: t.rtok });
                }
                Ev::Fund(_) => funds += 1,
                Ev::TokXfer(x) => {
                    xfers += 1;
                    if !x.from.is_on_curve() || !x.to.is_on_curve() {
                        xfer_pda += 1;
                    }
                    if x.amount as f64 >= 0.12 * ucore::pump::TOKEN_SUPPLY as f64 {
                        xfer_big += 1;
                    }
                    *xfer_by_mint.entry(x.mint).or_default() += 1;
                }
                Ev::Complete(_) => {}
            }
        }
    }
    println!("slots {first_slot}..{last_slot} blocks {blocks} missing_slots {gaps} (skipped by leaders or fetch failures)");
    println!("creates {creates} (mayhem {mayhem}) trades {trades} fundings {funds} token_xfers {xfers}");
    println!("curve continuity on known mints: ok {ok} mismatch {bad} ({:.4}%) first-seen {unknown}", 100.0 * bad as f64 / (ok + bad).max(1) as f64);
    let mut ix: Vec<_> = by_ix.into_iter().collect();
    ix.sort();
    println!("trade instruction codes: {ix:?}");
    let mut fb: Vec<_> = fee_bps_hist.into_iter().collect();
    fb.sort_by(|a, b| b.1.cmp(&a.1));
    println!("total fee bps (top): {:?}", &fb[..fb.len().min(8)]);
    println!("mismatch by (is_buy, mayhem, ix): {bad_kind:?}");
    for e in examples {
        println!("  mismatch: {e}");
    }
    let mut top: Vec<_> = xfer_by_mint.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    println!("token transfers: {xfers}, touching a PDA/program account {xfer_pda}, ≥12% of supply {xfer_big}; top mints {:?}", &top[..top.len().min(3)]);
    println!("frames {} truncated {}", rd.frames_read, rd.truncated_frames);
    Ok(())
}

fn f(x: f64) -> String {
    if x.is_finite() {
        format!("{x:.4}")
    } else {
        String::new()
    }
}

fn write_launches(path: &PathBuf, rows: &[LaunchRow], names: &[String]) -> Result<()> {
    let mut w = std::io::BufWriter::new(fs::File::create(path)?);
    write!(w, "mint,symbol,dev,op,op_launches_before,how,create_slot,create_time_utc,mayhem,block0_buyers,block0_sol,dev_buy_sol,crowd,block0_end_mcap_sol,entry_mcap_sol,p2s,p5s,p10s,p30s,p60s,p3m,p10m,first_dump_slots,dump_kind,max_buy_sol,n_trades,completed,chosen,honest,gate_pass,gate_reason,gate_n,gate_lcb,real_ret,real_variant,fund_from,fund_sol,fund_age_slots,fund_hub,ctx_key,ctx_pass,ctx_n,ctx_lcb")?;
    for n in names {
        write!(w, ",ret_{n}")?;
    }
    writeln!(w)?;
    for r in rows {
        let sym: String = r.symbol.chars().filter(|c| c.is_alphanumeric()).take(16).collect();
        write!(
            w,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.mint, sym, r.dev, r.op, r.op_launches_before, r.how, r.create_slot, r.create_time, r.mayhem, r.block0_buyers,
            f(r.block0_sol), f(r.dev_buy_sol), r.crowd, f(r.block0_end_mcap), f(r.ref_entry_mcap), f(r.p2s), f(r.p5s), f(r.p10s),
            f(r.p30s), f(r.p60s), f(r.p3m), f(r.p10m), r.first_dump_slots, r.dump_kind, f(r.max_buy_sol), r.n_trades, r.completed,
            r.chosen, f(r.honest), r.gate_pass, r.gate_reason, r.gate_n, f(r.gate_lcb), f(r.real_ret),
            r.real_variant, r.fund_from, f(r.fund_sol), r.fund_age_slots, r.fund_hub, r.ctx_key, r.ctx_pass, r.ctx_n, f(r.ctx_lcb)
        )?;
        for x in &r.results {
            write!(w, ",{}", f(*x as f64))?;
        }
        writeln!(w)?;
    }
    Ok(())
}

fn write_trades(path: &PathBuf, t: &[ClosedTrade], names: &[String]) -> Result<()> {
    let mut w = std::io::BufWriter::new(fs::File::create(path)?);
    writeln!(w, "id,mint,variant,operator,decision_slot,fill_slot,close_slot,hold_slots,stake_sol,cost_sol,proceeds_sol,fixed_costs_sol,pnl_sol,ret,entry_mcap_sol,peak_gain,exit_reason,failed_entry")?;
    for x in t {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            x.id,
            x.mint,
            names[x.variant],
            x.operator.unwrap_or(0),
            x.decision_slot,
            x.fill_slot,
            x.close_slot,
            x.close_slot.saturating_sub(x.fill_slot),
            f(x.stake as f64 / 1e9),
            f(x.cost as f64 / 1e9),
            f(x.proceeds as f64 / 1e9),
            f(x.fixed_costs as f64 / 1e9),
            f(x.pnl as f64 / 1e9),
            f(x.ret),
            f(x.entry_mcap),
            f(x.peak_gain),
            x.exit_reason,
            x.failed_entry
        )?;
    }
    Ok(())
}

struct Agg {
    n: usize,
    mean: f64,
    win: f64,
    med: f64,
}
fn agg(mut v: Vec<f64>) -> Agg {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return Agg { n: 0, mean: f64::NAN, win: f64::NAN, med: f64::NAN };
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Agg { n, mean: v.iter().sum::<f64>() / n as f64, win: v.iter().filter(|x| **x > 0.0).count() as f64 / n as f64, med: v[n / 2] }
}

fn replay(data: PathBuf, out: PathBuf, cfg: BrainCfg, from_slot: u64, to_slot: u64) -> Result<()> {
    fs::create_dir_all(&out)?;
    fs::write(out.join("config.toml"), toml::to_string_pretty(&cfg)?)?;
    let names: Vec<String> = cfg.variants.iter().map(|v| v.name.clone()).collect();
    let mut brain = Brain::new(cfg);
    let t0 = std::time::Instant::now();
    let mut n_ev = 0u64;
    let (mut first_t, mut last_t) = (i64::MAX, 0i64);
    for blk in StoreReader::open(&data)? {
        let bs = blk.last().map(|e| e.slot()).unwrap_or(0);
        if bs < from_slot || bs >= to_slot {
            continue;
        }
        for e in &blk {
            n_ev += 1;
            if let Ev::Block(b) = e {
                if b.block_time > 0 {
                    first_t = first_t.min(b.block_time);
                    last_t = last_t.max(b.block_time);
                }
            }
            brain.on_event(e);
        }
    }
    let el = t0.elapsed().as_secs_f64();
    write_launches(&out.join("launches.csv"), &brain.rows, &names)?;
    write_trades(&out.join("trades.csv"), &brain.closed_real, &names)?;

    write_operators(&out.join("operators.csv"), &mut brain, &names)?;

    // ---------------- summary
    let rows = &brain.rows;
    let mut s = String::new();
    let hours = (last_t - first_t) as f64 / 3600.0;
    writeln!(s, "# Replay summary\n")?;
    writeln!(s, "- events: {n_ev} replayed in {el:.1}s; chain time covered: {hours:.2} h")?;
    let c = &brain.counters;
    writeln!(s, "- launches: {} (tradable SOL-paired: {}), resolved rows: {}", c.launches, c.tradable, rows.len())?;
    writeln!(s, "- attributed to an operator seen before: {} ({:.1}%), late via insiders: {}", c.attributed_known, 100.0 * c.attributed_known as f64 / c.tradable.max(1) as f64, c.late_attributed)?;
    let mut how: Vec<_> = c.how.iter().collect();
    how.sort_by(|a, b| b.1.cmp(a.1));
    writeln!(s, "- attribution paths: {how:?}")?;
    writeln!(s, "- operators with ≥2 / ≥5 / ≥10 resolved launches: {} / {} / {}", brain.book.n_operators_with(2), brain.book.n_operators_with(5), brain.book.n_operators_with(10))?;
    writeln!(s, "- population prior (EMA of honest shadow returns): {:.3}", brain.prior)?;
    let mut gr: Vec<_> = c.gate_reasons.iter().collect();
    gr.sort_by(|a, b| b.1.cmp(a.1));
    writeln!(s, "- gate: pass {} | blocked by risk {} | reasons {gr:?}", c.gate_pass, c.risk_blocked)?;

    writeln!(s, "\n## Shadow results by exit variant (all resolved launches, stake-normalized, after fees+tips)\n")?;
    writeln!(s, "| variant | n | mean ret | median | win rate |\n|---|---|---|---|---|")?;
    for (i, n) in names.iter().enumerate() {
        let a = agg(rows.iter().map(|r| r.results.get(i).copied().unwrap_or(f32::NAN) as f64).collect());
        writeln!(s, "| {n} | {} | {:.3} | {:.3} | {:.1}% |", a.n, a.mean, a.med, 100.0 * a.win)?;
    }
    let repeat: Vec<&LaunchRow> = rows.iter().filter(|r| r.op_launches_before >= 2).collect();
    writeln!(s, "\n### Same, launches by operators with ≥2 prior launches ({} rows)\n", repeat.len())?;
    writeln!(s, "| variant | n | mean ret | median | win rate |\n|---|---|---|---|---|")?;
    for (i, n) in names.iter().enumerate() {
        let a = agg(repeat.iter().map(|r| r.results.get(i).copied().unwrap_or(f32::NAN) as f64).collect());
        writeln!(s, "| {n} | {} | {:.3} | {:.3} | {:.1}% |", a.n, a.mean, a.med, 100.0 * a.win)?;
    }
    let a = agg(rows.iter().map(|r| r.honest).collect());
    writeln!(s, "\nSelection-honest shadow (variant chosen before each launch): n={} mean {:.3} win {:.1}%", a.n, a.mean, 100.0 * a.win)?;

    // P-window profile (like a play sheet), realistic entry
    writeln!(s, "\n## Price path after a realistic entry (median of max gain within window)\n")?;
    let pw = |sel: &dyn Fn(&LaunchRow) -> f64, rs: &[&LaunchRow]| agg(rs.iter().map(|r| sel(r)).collect()).med;
    let all: Vec<&LaunchRow> = rows.iter().collect();
    writeln!(s, "| set | n | 2s | 5s | 10s | 30s | 60s | 3m | 10m |\n|---|---|---|---|---|---|---|---|---|")?;
    for (label, set) in [("all", &all), ("repeat operators", &repeat)] {
        writeln!(
            s,
            "| {label} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            set.iter().filter(|r| r.p2s.is_finite()).count(),
            pw(&|r| r.p2s, set),
            pw(&|r| r.p5s, set),
            pw(&|r| r.p10s, set),
            pw(&|r| r.p30s, set),
            pw(&|r| r.p60s, set),
            pw(&|r| r.p3m, set),
            pw(&|r| r.p10m, set)
        )?;
    }

    // real trades
    let t = &brain.closed_real;
    writeln!(s, "\n## Real (gated) trades\n")?;
    let filled: Vec<&ClosedTrade> = t.iter().filter(|x| !x.failed_entry).collect();
    let pnl: f64 = t.iter().map(|x| x.pnl as f64 / 1e9).sum();
    let a = agg(filled.iter().map(|x| x.ret).collect());
    writeln!(s, "- entries attempted {} | refused by price limit {} | filled {}", t.len(), t.len() - filled.len(), filled.len())?;
    writeln!(s, "- win rate {:.1}% | mean ret {:.3} | median {:.3} | total PnL {:.4} SOL", 100.0 * a.win, a.mean, a.med, pnl)?;
    let mut eq = 0.0f64;
    let mut peak = 0.0f64;
    let mut mdd = 0.0f64;
    let mut by_day: std::collections::BTreeMap<i64, (usize, f64)> = Default::default();
    let mut by_reason: FxHashMap<String, (usize, f64)> = FxHashMap::default();
    for x in t {
        eq += x.pnl as f64 / 1e9;
        peak = peak.max(eq);
        mdd = mdd.max(peak - eq);
        let e = by_reason.entry(x.exit_reason.clone()).or_default();
        e.0 += 1;
        e.1 += x.pnl as f64 / 1e9;
    }
    for r in rows {
        if r.real_ret.is_finite() {
            let e = by_day.entry(r.create_time / 86_400).or_default();
            e.0 += 1;
            e.1 += r.real_ret;
        }
    }
    writeln!(s, "- max drawdown {mdd:.4} SOL")?;
    writeln!(s, "\n| exit reason | n | PnL SOL |\n|---|---|---|")?;
    let mut br: Vec<_> = by_reason.into_iter().collect();
    br.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (k, (n, p)) in br {
        writeln!(s, "| {k} | {n} | {p:.4} |")?;
    }
    writeln!(s, "\n| UTC day | trades | sum of returns |\n|---|---|---|")?;
    for (d, (n, r)) in by_day {
        writeln!(s, "| {d} | {n} | {r:.3} |")?;
    }

    // go-live checklist (see docs/RUNBOOK.md §2)
    {
        let rets: Vec<f64> = filled.iter().map(|x| x.ret).collect();
        let n = rets.len();
        let mean = if n > 0 { rets.iter().sum::<f64>() / n as f64 } else { f64::NAN };
        let mut rng = ucore::fill::Rng::new(12345);
        let mut boots: Vec<f64> = (0..2000)
            .map(|_| {
                if n == 0 {
                    return f64::NAN;
                }
                (0..n).map(|_| rets[(rng.u64() % n as u64) as usize]).sum::<f64>() / n as f64
            })
            .collect();
        boots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let lb = boots.get(50).copied().unwrap_or(f64::NAN);
        let mut day_pnl: std::collections::BTreeMap<i64, f64> = Default::default();
        let mut op_pnl: FxHashMap<u32, f64> = FxHashMap::default();
        let slot_day = |slot: u64| -> i64 {
            // map decision slot to UTC day using the rows' create times
            rows.iter().find(|r| r.create_slot <= slot && slot - r.create_slot < 3000).map(|r| r.create_time / 86_400).unwrap_or(0)
        };
        for x in t {
            *day_pnl.entry(slot_day(x.decision_slot)).or_default() += x.pnl as f64 / 1e9;
            *op_pnl.entry(x.operator.unwrap_or(0)).or_default() += x.pnl as f64 / 1e9;
        }
        let total: f64 = day_pnl.values().sum();
        let pos_days = day_pnl.values().filter(|v| **v > 0.0).count();
        let max_day = day_pnl.values().cloned().fold(0.0f64, f64::max);
        let max_op = op_pnl.values().cloned().fold(0.0f64, f64::max);
        let share = |x: f64| if total > 0.0 { x / total } else { f64::NAN };
        let bankroll = brain.cfg.risk.bankroll_lamports as f64 / 1e9;
        let ok = |b: bool| if b { "PASS" } else { "FAIL" };
        writeln!(s, "\n## Go-live checklist (thresholds from docs/RUNBOOK.md)\n")?;
        writeln!(s, "| check | value | threshold | |\n|---|---|---|---|")?;
        writeln!(s, "| real trades | {n} | ≥ 60 | {} |", ok(n >= 60))?;
        writeln!(s, "| mean return / trade | {mean:.4} | > 0.04 | {} |", ok(mean > 0.04))?;
        writeln!(s, "| bootstrap 95% lower bound | {lb:.4} | > 0 | {} |", ok(lb > 0.0))?;
        writeln!(s, "| profitable days | {pos_days}/{} | ≥ 70% | {} |", day_pnl.len(), ok(!day_pnl.is_empty() && pos_days as f64 >= 0.7 * day_pnl.len() as f64))?;
        writeln!(s, "| best day share of PnL | {:.2} | < 0.35 | {} |", share(max_day), ok(share(max_day) < 0.35))?;
        writeln!(s, "| best operator share of PnL | {:.2} | < 0.30 | {} |", share(max_op), ok(share(max_op) < 0.30))?;
        writeln!(s, "| max drawdown / bankroll | {:.3} | < 0.25 | {} |", mdd / bankroll, ok(mdd / bankroll < 0.25))?;
        writeln!(s, "\nStress levels are separate runs (`--stress 1`, `--stress 2`).")?;
    }

    // operator leaderboard
    writeln!(s, "\n## Operators with most resolved launches\n")?;
    writeln!(s, "| op | launches | devs | honest n | honest mean | win | gate |\n|---|---|---|---|---|---|---|")?;
    let mut ops: Vec<(u32, usize)> = brain.book.stats.iter().map(|(k, v)| (*k, v.launches.len())).collect();
    ops.sort_by(|a, b| b.1.cmp(&a.1));
    let prior = brain.prior;
    let nvar = names.len();
    let gate_cfg = brain.cfg.gate.clone();
    let now = brain.now_slot;
    for (op, _) in ops.into_iter().take(25) {
        let st = brain.book.stats.get(&op).unwrap();
        let hs = agg(st.launches.iter().map(|l| l.honest as f64).collect());
        let g = st.gate(now, &gate_cfg, prior, nvar);
        writeln!(s, "| {op} | {} | {} | {} | {:.3} | {:.0}% | {} ({}) |", st.launches.len(), st.devs, hs.n, hs.mean, 100.0 * hs.win, g.pass, g.reason)?;
    }
    fs::write(out.join("summary.md"), &s)?;
    println!("{s}");
    Ok(())
}

fn compare(journal: PathBuf) -> Result<()> {
    use serde_json::Value;
    let txt = fs::read_to_string(&journal)?;
    let mut closed: FxHashMap<String, Value> = FxHashMap::default();
    let mut launches: FxHashMap<String, Value> = FxHashMap::default();
    let (mut failed, mut expired, mut orphans) = (0, 0, 0);
    for line in txt.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let k = v["kind"].as_str().unwrap_or("");
        let x = &v["v"];
        match k {
            "closed" if x["shadow"] == false => {
                closed.insert(x["mint"].as_str().unwrap_or("").to_string(), x.clone());
            }
            "launch" => {
                launches.insert(x["mint"].as_str().unwrap_or("").to_string(), x.clone());
            }
            "tx_failed" => failed += 1,
            "tx_expired" => expired += 1,
            "orphan" => orphans += 1,
            _ => {}
        }
    }
    println!("real trades {} | failed txs {failed} | expired {expired} | orphans {orphans}", closed.len());
    println!("{:<46} {:>9} {:>9} {:>9} {:>9}  reason", "mint", "real_ret", "model", "real_mc", "model_mc");
    let (mut sr, mut sm, mut n) = (0.0, 0.0, 0);
    for (mint, c) in &closed {
        let real = c["ret"].as_f64().unwrap_or(f64::NAN);
        let v = c["variant"].as_u64().unwrap_or(0) as usize;
        let model = launches
            .get(mint)
            .and_then(|l| l["results"].as_array().and_then(|a| a.get(v)).and_then(|x| x.as_f64()))
            .unwrap_or(f64::NAN);
        let model_mc = launches.get(mint).and_then(|l| l["ref_entry_mcap"].as_f64()).unwrap_or(f64::NAN);
        println!(
            "{:<46} {:>9.3} {:>9.3} {:>9.2} {:>9.2}  {}",
            mint,
            real,
            model,
            c["entry_mcap"].as_f64().unwrap_or(f64::NAN),
            model_mc,
            c["exit_reason"].as_str().unwrap_or("")
        );
        if real.is_finite() && model.is_finite() {
            sr += real;
            sm += model;
            n += 1;
        }
    }
    if n > 0 {
        println!("\nmean real {:.4} vs model {:.4} over {n} paired trades (gap {:+.4})", sr / n as f64, sm / n as f64, (sr - sm) / n as f64);
        println!("A persistent negative gap means the landing/fee model is too optimistic: fix it before raising stakes.");
    }
    Ok(())
}

fn med(mut v: Vec<f64>) -> f64 {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// One row per operator with ≥3 resolved launches: the automated "play sheet".
fn write_operators(path: &PathBuf, brain: &mut Brain, names: &[String]) -> Result<()> {
    let mut by_op: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    for (i, r) in brain.rows.iter().enumerate() {
        let root = brain.book.find(r.op);
        by_op.entry(root).or_default().push(i);
    }
    let mut ops: Vec<(u32, Vec<usize>)> = by_op.into_iter().filter(|(_, v)| v.len() >= 3).collect();
    ops.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let mut w = std::io::BufWriter::new(fs::File::create(path)?);
    writeln!(w, "op,launches,devs,first_utc,last_utc,links,top_funder,top_funder_share,fund_sol_min,fund_sol_max,med_fund_age_s,med_dev_buy_sol,med_block0_sol,med_block0_buyers,med_entry_mcap,p2s,p5s,p10s,p30s,p60s,p3m,honest_n,honest_mean,honest_win,best_variant,best_mean,best_win,example_symbols")?;
    for (op, idx) in ops {
        let rs: Vec<&LaunchRow> = idx.iter().map(|i| &brain.rows[*i]).collect();
        let devs: std::collections::BTreeSet<&str> = rs.iter().map(|r| r.dev.as_str()).collect();
        let mut links: FxHashMap<&str, usize> = FxHashMap::default();
        for r in &rs {
            *links.entry(r.how.as_str()).or_default() += 1;
        }
        let mut lk: Vec<_> = links.into_iter().collect();
        lk.sort();
        let lks: Vec<String> = lk.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        let mut fcount: FxHashMap<&str, usize> = FxHashMap::default();
        for r in &rs {
            if !r.fund_from.is_empty() {
                *fcount.entry(r.fund_from.as_str()).or_default() += 1;
            }
        }
        let top = fcount.iter().max_by_key(|x| *x.1).map(|(k, v)| (k.to_string(), *v));
        let (top_f, top_share) = top.clone().map(|(k, v)| (k, v as f64 / rs.len() as f64)).unwrap_or_default();
        let famt: Vec<f64> = rs.iter().filter(|r| Some(r.fund_from.as_str()) == top.as_ref().map(|t| t.0.as_str())).map(|r| r.fund_sol).collect();
        let fmin = famt.iter().cloned().fold(f64::INFINITY, f64::min);
        let fmax = famt.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let fage = med(rs.iter().filter(|r| r.fund_age_slots >= 0).map(|r| r.fund_age_slots as f64 * 0.4).collect());
        let hon = agg(rs.iter().map(|r| r.honest).collect());
        let mut best = (0usize, f64::NEG_INFINITY, 0.0);
        for v in 0..names.len() {
            let a = agg(rs.iter().map(|r| r.results.get(v).copied().unwrap_or(f32::NAN) as f64).collect());
            if a.n > 0 && a.mean > best.1 {
                best = (v, a.mean, a.win);
            }
        }
        let syms: Vec<String> = rs.iter().take(4).map(|r| r.symbol.chars().filter(|c| c.is_alphanumeric()).take(10).collect()).collect();
        writeln!(
            w,
            "{op},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            rs.len(),
            devs.len(),
            rs.iter().map(|r| r.create_time).min().unwrap_or(0),
            rs.iter().map(|r| r.create_time).max().unwrap_or(0),
            lks.join(" "),
            top_f,
            f(top_share),
            f(fmin),
            f(fmax),
            f(fage),
            f(med(rs.iter().map(|r| r.dev_buy_sol).collect())),
            f(med(rs.iter().map(|r| r.block0_sol).collect())),
            f(med(rs.iter().map(|r| r.block0_buyers as f64).collect())),
            f(med(rs.iter().map(|r| r.ref_entry_mcap).collect())),
            f(med(rs.iter().map(|r| r.p2s).collect())),
            f(med(rs.iter().map(|r| r.p5s).collect())),
            f(med(rs.iter().map(|r| r.p10s).collect())),
            f(med(rs.iter().map(|r| r.p30s).collect())),
            f(med(rs.iter().map(|r| r.p60s).collect())),
            f(med(rs.iter().map(|r| r.p3m).collect())),
            hon.n,
            f(hon.mean),
            f(hon.win),
            names[best.0],
            f(best.1),
            f(best.2),
            syms.join(" ")
        )?;
    }
    Ok(())
}

fn sweep(data: PathBuf, out: PathBuf, from_slot: u64, to_slot: u64) -> Result<()> {
    fs::create_dir_all(&out)?;
    let mut grid: Vec<(String, BrainCfg)> = Vec::new();
    for &min_n in &[4usize, 6, 10] {
        for &min_lcb in &[0.0f64, 0.03, 0.08] {
            for &z in &[1.0f64, 1.64] {
                for &stress in &[0u32, 1] {
                    let mut c = BrainCfg::default();
                    c.gate.min_n = min_n;
                    c.gate.min_lcb = min_lcb;
                    c.gate.z = z;
                    if stress > 0 {
                        c.latency = LatencyModel::stressed(stress);
                    }
                    grid.push((format!("n{min_n}_lcb{min_lcb}_z{z}_s{stress}"), c));
                }
            }
        }
    }
    let mut brains: Vec<Brain> = grid.iter().map(|(_, c)| Brain::new(c.clone())).collect();
    for blk in StoreReader::open(&data)? {
        let bs = blk.last().map(|e| e.slot()).unwrap_or(0);
        if bs < from_slot || bs >= to_slot {
            continue;
        }
        for e in &blk {
            for b in brains.iter_mut() {
                b.on_event(e);
            }
        }
    }
    let mut res: Vec<(String, usize, f64, f64, f64, f64)> = Vec::new();
    for ((name, _), b) in grid.iter().zip(brains.iter()) {
        let filled: Vec<f64> = b.closed_real.iter().filter(|t| !t.failed_entry).map(|t| t.ret).collect();
        let a = agg(filled.clone());
        let pnl: f64 = b.closed_real.iter().map(|t| t.pnl as f64 / 1e9).sum();
        let n = filled.len();
        let mut rng = ucore::fill::Rng::new(99);
        let mut boots: Vec<f64> = (0..1000)
            .map(|_| if n == 0 { f64::NAN } else { (0..n).map(|_| filled[(rng.u64() % n as u64) as usize]).sum::<f64>() / n as f64 })
            .collect();
        boots.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        res.push((name.clone(), n, a.mean, a.win, boots.get(25).copied().unwrap_or(f64::NAN), pnl));
    }
    res.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal));
    let mut s = String::from("| config | trades | mean ret | win | boot 97.5% LB | PnL SOL |\n|---|---|---|---|---|---|\n");
    for r in &res {
        s += &format!("| {} | {} | {:.4} | {:.1}% | {:.4} | {:.4} |\n", r.0, r.1, r.2, 100.0 * r.3, r.4, r.5);
    }
    s += "\nTune on the training window only; confirm the chosen row once on held-out days.\n";
    fs::write(out.join("sweep.md"), &s)?;
    println!("{s}");
    Ok(())
}
