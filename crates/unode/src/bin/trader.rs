//! Live trader. Same brain as the backtest; three modes:
//!   paper  – live stream, fills simulated by the landing model, nothing is sent
//!   live   – real transactions (start with canary stakes)
//! Safety: kill file, stream-lag guard, wallet blast-radius cap, daily loss limit and
//! loss-streak pause (brain), late-fill orphan selling, leftover-token sweep.

use anyhow::{anyhow, bail, Result};
use clap::Parser;
use ed25519_dalek::SigningKey;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use ucore::brain::{Brain, BrainCfg, ExecMode, Intent};
use ucore::event::{BlockStat, Ev};
use ucore::pk::{Pk, Sig};
use ucore::store::StoreReader;
use ucore::tx::{decode_tx, DecodeCfg, DecodeState, TIP_ACCOUNTS};
use ucore::txbuild::{build_tx, buy_ixs, sell_ixs, TxParams, MAX_TX_SIZE};
use unode::grpc::{now_us, run_source, GrpcCfg, SourceMsg};
use unode::rpc::Rpc;
use unode::sender::{EndpointCfg, Sender};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TraderCfg {
    /// "paper" or "live"
    mode: String,
    wallet_keypair: Option<PathBuf>,
    rpc_url: String,
    grpc_endpoint: String,
    grpc_x_token: Option<String>,
    #[serde(default = "d_true")]
    firehose: bool,
    #[serde(default)]
    warmup_data: Vec<PathBuf>,
    #[serde(default = "d_kill")]
    kill_file: PathBuf,
    /// Refuse to trade live if the hot wallet holds more than this (limits blast radius).
    #[serde(default = "d_max_wallet")]
    max_wallet_lamports: u64,
    #[serde(default = "d_lag")]
    max_stream_lag_ms: u64,
    #[serde(default = "d_expire")]
    expire_slots: u64,
    #[serde(default)]
    senders: Vec<EndpointCfg>,
    #[serde(default)]
    tip_accounts: Vec<String>,
    #[serde(default)]
    telegram_bot_token: Option<String>,
    #[serde(default)]
    telegram_chat_id: Option<String>,
    #[serde(default = "d_journal")]
    journal: PathBuf,
    #[serde(default)]
    brain: BrainCfg,
}
fn d_true() -> bool {
    true
}
fn d_kill() -> PathBuf {
    PathBuf::from("STOP")
}
fn d_max_wallet() -> u64 {
    10_000_000_000
}
fn d_lag() -> u64 {
    1500
}
fn d_expire() -> u64 {
    20
}
fn d_journal() -> PathBuf {
    PathBuf::from("data/journal.jsonl")
}

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "config/trader.toml")]
    config: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Buy,
    Sell,
}

struct Inflight {
    pos: u64,
    side: Side,
    sent_slot: u64,
    tokens: u64,
    tip: u64,
}

struct Journal(std::fs::File);
impl Journal {
    fn log(&mut self, kind: &str, v: serde_json::Value) {
        let line = serde_json::json!({"t": now_us(), "kind": kind, "v": v});
        let _ = writeln!(self.0, "{line}");
    }
}

fn load_key(p: &PathBuf) -> Result<SigningKey> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(p)?.permissions().mode();
        if mode & 0o077 != 0 {
            bail!("keypair {} is readable by group/others (mode {:o}); run chmod 600", p.display(), mode & 0o777);
        }
    }
    let bytes: Vec<u8> = serde_json::from_str(&std::fs::read_to_string(p)?)?;
    if bytes.len() != 64 {
        bail!("keypair must be a 64-byte JSON array (solana-keygen format)");
    }
    let k = SigningKey::from_bytes(&bytes[..32].try_into().unwrap());
    if k.verifying_key().to_bytes() != bytes[32..64] {
        bail!("keypair public half does not match secret");
    }
    Ok(k)
}

async fn telegram(cfg: &TraderCfg, http: &reqwest::Client, text: String) {
    if let (Some(t), Some(c)) = (&cfg.telegram_bot_token, &cfg.telegram_chat_id) {
        let url = format!("https://api.telegram.org/bot{t}/sendMessage");
        let _ = http.post(url).json(&serde_json::json!({"chat_id": c, "text": text})).send().await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?)).init();
    let args = Args::parse();
    let cfg: TraderCfg = toml::from_str(&std::fs::read_to_string(&args.config)?)?;
    let live = match cfg.mode.as_str() {
        "live" => true,
        "paper" => false,
        m => bail!("unknown mode {m}"),
    };
    let rpc = Rpc::new(&cfg.rpc_url)?;
    let http = reqwest::Client::new();
    let key = match &cfg.wallet_keypair {
        Some(p) => Some(load_key(p)?),
        None if live => bail!("live mode needs wallet_keypair"),
        None => None,
    };
    let wallet = key.as_ref().map(|k| Pk(k.verifying_key().to_bytes()));
    let mut bankroll = cfg.brain.risk.bankroll_lamports;
    if let (true, Some(w)) = (live, wallet) {
        let bal = rpc.balance(&w).await?;
        tracing::info!("wallet {w} balance {:.4} SOL", bal as f64 / 1e9);
        if bal > cfg.max_wallet_lamports {
            bail!("wallet holds {:.3} SOL > max_wallet_lamports; keep only trading capital in the hot wallet", bal as f64 / 1e9);
        }
        if bal < cfg.brain.risk.stake_lamports * 3 {
            bail!("wallet balance too low for the configured stake");
        }
        bankroll = bal;
    }

    // ---------------- warm-up: learn operators from recorded history
    let mut brain = Brain::new(cfg.brain.clone());
    brain.cfg.mode = ExecMode::Sim;
    let t0 = Instant::now();
    let mut n = 0u64;
    for d in &cfg.warmup_data {
        for blk in StoreReader::open(d)? {
            for e in &blk {
                brain.on_event(e);
                n += 1;
            }
        }
    }
    tracing::info!(
        "warm-up: {n} events in {:.1}s; operators ≥5 launches: {}; prior {:.3}",
        t0.elapsed().as_secs_f64(),
        brain.book.n_operators_with(5),
        brain.prior
    );
    brain.reset_real(bankroll);
    brain.cfg.mode = if live { ExecMode::Live } else { ExecMode::Sim };

    let mut journal = Journal(std::fs::OpenOptions::new().create(true).append(true).open(&cfg.journal)?);
    journal.log("start", serde_json::json!({"mode": cfg.mode, "wallet": wallet.map(|w| w.to_b58()), "bankroll": bankroll}));

    // ---------------- infrastructure
    let sender = if live { Some(Sender::new(&cfg.senders)?) } else { None };
    if let Some(s) = &sender {
        s.spawn_keepalive();
    }
    let tips: Vec<Pk> = if cfg.tip_accounts.is_empty() {
        TIP_ACCOUNTS[8..].to_vec()
    } else {
        cfg.tip_accounts.iter().map(|s| s.parse()).collect::<Result<_>>()?
    };
    let blockhash: Arc<RwLock<Option<[u8; 32]>>> = Arc::new(RwLock::new(None));
    {
        let bh = blockhash.clone();
        let rpc = rpc.clone();
        tokio::spawn(async move {
            loop {
                if let Ok((h, _)) = rpc.latest_blockhash().await {
                    *bh.write().unwrap() = Some(h);
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        });
    }
    let (src_tx, mut src_rx) = mpsc::channel::<SourceMsg>(100_000);
    let gcfg = GrpcCfg {
        endpoint: cfg.grpc_endpoint.clone(),
        x_token: cfg.grpc_x_token.clone(),
        commitment: "processed".into(),
        firehose: cfg.firehose,
        include_failed: true,
        extra_accounts: wallet.map(|w| vec![w.to_b58()]).unwrap_or_default(),
    };
    tokio::spawn(run_source(gcfg, src_tx));
    // sent signatures come back from the executor tasks
    let (sent_tx, mut sent_rx) = mpsc::unbounded_channel::<(Sig, Inflight)>();
    // (position, on-chain token balance) from reconciliation checks
    let (recon_tx, mut recon_rx) = mpsc::unbounded_channel::<(u64, u64)>();
    let mut sell_attempts: FxHashMap<u64, u32> = FxHashMap::default();
    // fully sold token accounts whose rent (~0.002 SOL each) we reclaim in batches
    let mut empty_atas: Vec<(Pk, Pk)> = Vec::new();
    let mut last_reclaim = Instant::now();

    let dcfg = DecodeCfg::default();
    let mut dst = DecodeState::default();
    let mut inflight: FxHashMap<Sig, Inflight> = FxHashMap::default();
    let mut dead_buys: FxHashMap<Sig, u64> = FxHashMap::default();
    let mut last_msg = Instant::now();
    let mut last_check = Instant::now();
    let mut lag_paused = false;
    let mut killed = false;
    let mut evs: Vec<Ev> = Vec::new();
    let mut tip_rr = 0usize;
    let mut cur_slot = 0u64;
    telegram(&cfg, &http, format!("solultimate trader started in {} mode", cfg.mode)).await;

    loop {
        let msg = tokio::select! {
            m = src_rx.recv() => m,
            s = sent_rx.recv() => {
                if let Some((sig, inf)) = s { inflight.insert(sig, inf); }
                continue;
            }
            r = recon_rx.recv() => {
                if let Some((pos, bal)) = r {
                    tracing::warn!("reconcile position {pos}: on-chain balance {bal}");
                    journal.log("reconcile", serde_json::json!({"pos": pos, "balance": bal}));
                    brain.on_live_reconcile(pos, bal, cur_slot);
                    sell_attempts.remove(&pos);
                }
                continue;
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => None,
        };
        if let Some(m) = msg {
            last_msg = Instant::now();
            match m {
                SourceMsg::Tx(mut tv) => {
                    cur_slot = cur_slot.max(tv.slot);
                    tv.block_time = (now_us() / 1_000_000) as i64;
                    // our own transactions
                    if let Some(inf) = inflight.remove(&tv.sig) {
                        let fixed = tv.fee + if tv.failed { 0 } else { inf.tip };
                        if tv.failed {
                            match inf.side {
                                Side::Buy => brain.on_live_buy_failed(inf.pos, tv.slot, fixed),
                                Side::Sell => {
                                    brain.on_live_sell_failed(inf.pos, fixed);
                                    let n = sell_attempts.entry(inf.pos).or_default();
                                    *n += 1;
                                    if *n >= 3 {
                                        spawn_reconcile(&rpc, &brain, inf.pos, wallet, recon_tx.clone());
                                    }
                                }
                            }
                            journal.log("tx_failed", serde_json::json!({"pos": inf.pos, "sig": tv.sig.to_b58(), "side": format!("{:?}", inf.side)}));
                        } else {
                            let mut tmp = Vec::new();
                            let mut st2 = DecodeState::default();
                            decode_tx(&tv, &dcfg, &mut st2, &mut tmp);
                            for e in &tmp {
                                if let Ev::Trade(t) = e {
                                    if Some(t.user) == wallet {
                                        if t.is_buy {
                                            brain.on_live_buy_fill(inf.pos, t.sol + t.fee, t.sol, t.tok, tv.slot, fixed);
                                            journal.log("buy_fill", serde_json::json!({"pos": inf.pos, "mint": t.mint.to_b58(), "cost": t.sol + t.fee, "tokens": t.tok, "slot": tv.slot}));
                                            telegram(&cfg, &http, format!("BUY {} cost {:.4} SOL", t.mint, (t.sol + t.fee) as f64 / 1e9)).await;
                                        } else {
                                            brain.on_live_sell_fill(inf.pos, t.tok, t.sol.saturating_sub(t.fee), tv.slot, fixed);
                                            sell_attempts.remove(&inf.pos);
                                            if brain.position(inf.pos).map(|p| p.tokens == 0).unwrap_or(true) {
                                                let tp = brain.token_program(&t.mint).unwrap_or(ucore::pump::TOKEN_2022);
                                                empty_atas.push((t.mint, tp));
                                            }
                                            journal.log("sell_fill", serde_json::json!({"pos": inf.pos, "mint": t.mint.to_b58(), "out": t.sol.saturating_sub(t.fee), "tokens": t.tok}));
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(pos) = dead_buys.remove(&tv.sig) {
                        // a buy we gave up on landed late: we hold tokens nobody manages → sell now
                        if !tv.failed {
                            tracing::error!("late fill for position {pos}; selling orphan");
                            journal.log("orphan", serde_json::json!({"pos": pos, "sig": tv.sig.to_b58()}));
                            let mut tmp = Vec::new();
                            let mut st2 = DecodeState::default();
                            decode_tx(&tv, &dcfg, &mut st2, &mut tmp);
                            for e in &tmp {
                                if let Ev::Trade(t) = e {
                                    if Some(t.user) == wallet && t.is_buy {
                                        let min = brain.mints.get(&t.mint).map(|m| m.curve.sell(t.tok, m.fee_bps).0 / 2).unwrap_or(0);
                                        brain.intents.push(Intent::Sell { pos: 0, mint: t.mint, tokens: t.tok, min_lamports: min, reason: "orphan".into(), urgent: true });
                                    }
                                }
                            }
                        }
                    }
                    evs.clear();
                    decode_tx(&tv, &dcfg, &mut dst, &mut evs);
                    for e in &evs {
                        brain.on_event(e);
                    }
                    brain.flush_decisions();
                }
                SourceMsg::BlockMeta { slot, parent, block_time, tx_count } => {
                    cur_slot = cur_slot.max(slot);
                    brain.on_event(&Ev::Block(BlockStat { slot, parent_slot: parent, block_time, n_tx: tx_count as u32, ..Default::default() }));
                    // expire buys that never showed up
                    let expired: Vec<Sig> = inflight.iter().filter(|(_, i)| slot > i.sent_slot + cfg.expire_slots).map(|(s, _)| *s).collect();
                    for s in expired {
                        let inf = inflight.remove(&s).unwrap();
                        match inf.side {
                            Side::Buy => {
                                brain.on_live_buy_failed(inf.pos, slot, 0);
                                dead_buys.insert(s, inf.pos);
                            }
                            Side::Sell => {
                                brain.on_live_sell_failed(inf.pos, 0);
                                let n = sell_attempts.entry(inf.pos).or_default();
                                *n += 1;
                                if *n >= 3 {
                                    spawn_reconcile(&rpc, &brain, inf.pos, wallet, recon_tx.clone());
                                }
                            }
                        }
                        journal.log("tx_expired", serde_json::json!({"pos": inf.pos, "sig": s.to_b58()}));
                    }
                }
                SourceMsg::Disconnected(why) => {
                    journal.log("disconnected", serde_json::json!({"why": why}));
                }
            }
        }

        // ---------------- safety checks
        if last_check.elapsed() > Duration::from_millis(500) {
            last_check = Instant::now();
            let lag = last_msg.elapsed().as_millis() as u64;
            if lag > cfg.max_stream_lag_ms && !lag_paused {
                tracing::warn!("stream lag {lag} ms: pausing new entries");
                brain.paused_until = u64::MAX;
                lag_paused = true;
            } else if lag <= cfg.max_stream_lag_ms && lag_paused && !killed {
                brain.paused_until = 0;
                lag_paused = false;
            }
            if !killed && cfg.kill_file.exists() {
                killed = true;
                tracing::error!("kill file present: selling everything, no new entries");
                brain.paused_until = u64::MAX;
                brain.panic_sell_all("kill_switch");
                journal.log("kill", serde_json::json!({}));
                telegram(&cfg, &http, "KILL SWITCH: exiting all positions".into()).await;
            }
            if !empty_atas.is_empty() && last_reclaim.elapsed() > Duration::from_secs(60) {
                last_reclaim = Instant::now();
                if let (Some(k), Some(w), Some(bh)) = (key.clone(), wallet, *blockhash.read().unwrap()) {
                    let batch: Vec<(Pk, Pk)> = empty_atas.drain(..empty_atas.len().min(8)).collect();
                    let rpc = rpc.clone();
                    tokio::spawn(async move {
                        let mut ixs = Vec::new();
                        for (mint, tp) in &batch {
                            let ata = Pk::ata(&w, mint, tp);
                            // only close accounts that are really empty
                            if let Ok(Some((d, _, _))) = rpc.account_data(&ata).await {
                                if d.get(64..72).map(|b| u64::from_le_bytes(b.try_into().unwrap())) == Some(0) {
                                    ixs.push(ucore::txbuild::close_account(&ata, &w, &w, tp));
                                }
                            }
                        }
                        if !ixs.is_empty() {
                            let (tx, sig) = build_tx(&w, &ixs, &bh, Some(&k));
                            match rpc.send(&tx).await {
                                Ok(_) => tracing::info!("rent reclaim: closed {} token accounts ({sig})", ixs.len()),
                                Err(e) => tracing::warn!("rent reclaim failed: {e}"),
                            }
                        }
                    });
                }
            }
            if killed && brain.real_open_count() == 0 && inflight.is_empty() {
                tracing::info!("killed and flat: exiting");
                break;
            }
        }
        // resolved launches (incl. shadow predictions) → journal, for live-vs-model checks
        for r in brain.rows.drain(..) {
            if r.real_variant >= 0 || r.gate_pass || r.ctx_pass {
                journal.log("launch", serde_json::to_value(&r)?);
            }
        }
        // closed real trades → journal
        for c in brain.closed_real.drain(..) {
            journal.log("closed", serde_json::to_value(&c)?);
            if !c.shadow {
                telegram(&cfg, &http, format!("CLOSED {} {} pnl {:+.4} SOL ({:+.1}%)", c.mint, c.exit_reason, c.pnl as f64 / 1e9, c.ret * 100.0)).await;
            }
        }
        // ---------------- execute intents
        let intents: Vec<Intent> = brain.intents.drain(..).collect();
        for it in intents {
            journal.log("intent", serde_json::to_value(&it)?);
            let (Some(sender), Some(key), Some(w)) = (&sender, &key, wallet) else { continue };
            let Some(bh) = *blockhash.read().unwrap() else {
                tracing::warn!("no blockhash yet; dropping intent");
                continue;
            };
            tip_rr += 1;
            let costs = &brain.cfg.costs;
            let (ixs, inf, sig_side) = match &it {
                Intent::Buy { pos, mint, lamports, min_tokens, .. } => {
                    let Some(tp) = brain.token_program(mint) else { continue };
                    let p = TxParams { cu_limit: costs.buy_cu_limit, cu_price: costs.buy_cu_price, tip_lamports: costs.buy_tip, tip_account: Some(tips[tip_rr % tips.len()]) };
                    (buy_ixs(&w, mint, &tp, *lamports, *min_tokens, &p, tip_rr), Inflight { pos: *pos, side: Side::Buy, sent_slot: cur_slot, tokens: 0, tip: costs.buy_tip }, Side::Buy)
                }
                Intent::Sell { pos, mint, tokens, min_lamports, urgent, .. } => {
                    let Some(tp) = brain.token_program(mint).or(Some(ucore::pump::TOKEN_2022)) else { continue };
                    let tip = if *urgent { costs.sell_tip * 2 } else { costs.sell_tip };
                    let p = TxParams { cu_limit: costs.sell_cu_limit, cu_price: costs.sell_cu_price * if *urgent { 2 } else { 1 }, tip_lamports: tip, tip_account: Some(tips[tip_rr % tips.len()]) };
                    (sell_ixs(&w, mint, &tp, *tokens, *min_lamports, false, &p, tip_rr), Inflight { pos: *pos, side: Side::Sell, sent_slot: cur_slot, tokens: *tokens, tip }, Side::Sell)
                }
            };
            let (tx, sig) = build_tx(&w, &ixs, &bh, Some(key));
            if tx.len() > MAX_TX_SIZE {
                tracing::error!("tx too large ({} bytes)", tx.len());
                continue;
            }
            let _ = inf.tokens;
            let sender = sender.clone();
            let sent_tx = sent_tx.clone();
            // register before sending so the stream can never beat us
            inflight.insert(sig, inf);
            let _ = sig_side;
            tokio::spawn(async move {
                let res = sender.send_all(&tx).await;
                for (name, el, r) in res {
                    match r {
                        Ok(_) => tracing::info!("sent {sig} via {name} in {} µs", el.as_micros()),
                        Err(e) => tracing::warn!("send via {name} failed: {e}"),
                    }
                }
                let _ = sent_tx; // channel kept for future resend logic
            });
        }
    }
    journal.log("stop", serde_json::json!({}));
    let _ = anyhow!("");
    Ok(())
}

/// Check our token balance for a position's mint and report it back to the main loop.
fn spawn_reconcile(rpc: &Rpc, brain: &Brain, pos: u64, wallet: Option<Pk>, tx: mpsc::UnboundedSender<(u64, u64)>) {
    let (Some(w), Some(p)) = (wallet, brain.position(pos)) else { return };
    let mint = p.mint;
    let tp = brain.token_program(&mint).unwrap_or(ucore::pump::TOKEN_2022);
    let rpc = rpc.clone();
    tokio::spawn(async move {
        let ata = Pk::ata(&w, &mint, &tp);
        let bal = match rpc.account_data(&ata).await {
            Ok(Some((d, _, _))) => d.get(64..72).map(|b| u64::from_le_bytes(b.try_into().unwrap())).unwrap_or(0),
            Ok(None) => 0,
            Err(_) => return,
        };
        let _ = tx.send((pos, bal));
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn example_config_parses() {
        let s = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/trader.example.toml")).unwrap();
        let c: super::TraderCfg = toml::from_str(&s).unwrap();
        assert_eq!(c.brain.risk.stake_lamports, 100_000_000);
        assert_eq!(c.brain.costs.buy_tip, 1_000_000);
        assert!(!c.brain.variants.is_empty());
    }
}
