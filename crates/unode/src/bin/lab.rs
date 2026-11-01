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
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Check { data } => check(data),
        Cmd::Replay { data, out, config, stress, no_mayhem, dump_config } => {
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
            replay(data, out, cfg)
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
                Ev::TokXfer(_) => xfers += 1,
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
    write!(w, "mint,symbol,dev,op,op_launches_before,how,create_slot,create_time_utc,mayhem,block0_buyers,block0_sol,dev_buy_sol,crowd,block0_end_mcap_sol,entry_mcap_sol,p2s,p5s,p10s,p30s,p60s,p3m,p10m,first_dump_slots,dump_kind,max_buy_sol,n_trades,completed,chosen,honest,gate_pass,gate_reason,gate_n,gate_lcb,real_ret")?;
    for n in names {
        write!(w, ",ret_{n}")?;
    }
    writeln!(w)?;
    for r in rows {
        let sym: String = r.symbol.chars().filter(|c| c.is_alphanumeric()).take(16).collect();
        write!(
            w,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.mint, sym, r.dev, r.op, r.op_launches_before, r.how, r.create_slot, r.create_time, r.mayhem, r.block0_buyers,
            f(r.block0_sol), f(r.dev_buy_sol), r.crowd, f(r.block0_end_mcap), f(r.ref_entry_mcap), f(r.p2s), f(r.p5s), f(r.p10s),
            f(r.p30s), f(r.p60s), f(r.p3m), f(r.p10m), r.first_dump_slots, r.dump_kind, f(r.max_buy_sol), r.n_trades, r.completed,
            r.chosen, f(r.honest), r.gate_pass, r.gate_reason, r.gate_n, f(r.gate_lcb), f(r.real_ret)
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

fn replay(data: PathBuf, out: PathBuf, cfg: BrainCfg) -> Result<()> {
    fs::create_dir_all(&out)?;
    fs::write(out.join("config.toml"), toml::to_string_pretty(&cfg)?)?;
    let names: Vec<String> = cfg.variants.iter().map(|v| v.name.clone()).collect();
    let mut brain = Brain::new(cfg);
    let t0 = std::time::Instant::now();
    let mut n_ev = 0u64;
    let (mut first_t, mut last_t) = (i64::MAX, 0i64);
    for blk in StoreReader::open(&data)? {
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
