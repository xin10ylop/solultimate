//! Historical block backfill over JSON-RPC `getBlock` → event store.
//!
//! Used for the pilot dataset and for filling recorder gaps. Blocks are fetched in parallel
//! (with 429 backoff) and decoded strictly in slot order.

use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use ucore::event::Ev;
use ucore::rpcjson::{decode_block, parse_block_response, RpcBlock};
use ucore::store::StoreWriter;
use ucore::tx::{DecodeCfg, DecodeState};

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc: String,
    /// First slot (inclusive). If omitted, uses finalized - last.
    #[arg(long)]
    start: Option<u64>,
    /// End slot (exclusive). Defaults to start + last.
    #[arg(long)]
    end: Option<u64>,
    /// Number of slots to fetch when start/end are not both given.
    #[arg(long, default_value_t = 9000)]
    last: u64,
    #[arg(long, default_value = "data/pilot")]
    out: String,
    #[arg(long, default_value_t = 16)]
    concurrency: usize,
    /// Rotate segment files every N slots.
    #[arg(long, default_value_t = 9000)]
    rotate: u64,
}

async fn rpc_slot(client: &reqwest::Client, url: &str) -> Result<u64> {
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"finalized"}]});
    let v: serde_json::Value = client.post(url).json(&body).send().await?.json().await?;
    v["result"].as_u64().ok_or_else(|| anyhow::anyhow!("bad getSlot: {v}"))
}

async fn fetch_block(client: &reqwest::Client, url: &str, slot: u64, throttled: &AtomicU64) -> Option<RpcBlock> {
    let body = serde_json::json!({
        "jsonrpc":"2.0","id":slot,"method":"getBlock",
        "params":[slot,{"encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"full","rewards":false,"commitment":"finalized"}]
    });
    let mut backoff = 500u64;
    for _attempt in 0..12 {
        let res = client.post(url).json(&body).send().await;
        match res {
            Ok(r) => {
                let status = r.status();
                if status.as_u16() == 429 {
                    throttled.fetch_add(1, Ordering::Relaxed);
                } else if let Ok(bytes) = r.bytes().await {
                    match parse_block_response(&bytes) {
                        Ok(b) => return b,
                        Err(e) => {
                            let s = e.to_string();
                            if s.contains("429") || s.contains("Too many") {
                                throttled.fetch_add(1, Ordering::Relaxed);
                            } else if !s.contains("not available") {
                                tracing::debug!("slot {slot}: {s}");
                            }
                        }
                    }
                }
            }
            Err(e) => tracing::debug!("slot {slot}: http {e}"),
        }
        tokio::time::sleep(Duration::from_millis(backoff)).await;
        backoff = (backoff * 2).min(8000);
    }
    tracing::warn!("slot {slot}: giving up");
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?)).init();
    let a = Args::parse();
    let client = reqwest::Client::builder().http1_only().gzip(true).timeout(Duration::from_secs(60)).pool_max_idle_per_host(64).build()?;
    let (start, end) = match (a.start, a.end) {
        (Some(s), Some(e)) => (s, e),
        (Some(s), None) => (s, s + a.last),
        _ => {
            let fin = rpc_slot(&client, &a.rpc).await?;
            (fin - a.last, fin)
        }
    };
    tracing::info!("backfill slots [{start}, {end}) → {}", a.out);
    let (tx, mut rx) = mpsc::channel::<(u64, Option<RpcBlock>)>(a.concurrency * 4);
    let sem = Arc::new(Semaphore::new(a.concurrency));
    let throttled = Arc::new(AtomicU64::new(0));
    {
        let client = client.clone();
        let url = a.rpc.clone();
        let sem = sem.clone();
        let throttled = throttled.clone();
        tokio::spawn(async move {
            for slot in start..end {
                let permit = sem.clone().acquire_owned().await.unwrap();
                let client = client.clone();
                let url = url.clone();
                let tx = tx.clone();
                let throttled = throttled.clone();
                tokio::spawn(async move {
                    let b = fetch_block(&client, &url, slot, &throttled).await;
                    let _ = tx.send((slot, b)).await;
                    drop(permit);
                });
            }
        });
    }

    let mut writer = StoreWriter::new(&a.out, a.rotate)?;
    let cfg = DecodeCfg::default();
    let mut st = DecodeState::default();
    let mut pending: BTreeMap<u64, Option<RpcBlock>> = BTreeMap::new();
    let mut next = start;
    let t0 = Instant::now();
    let (mut n_blocks, mut n_create, mut n_trade, mut n_fund, mut n_xfer, mut n_pump_failed) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut non_pump_suffix = 0u64;
    while next < end {
        let Some((slot, b)) = rx.recv().await else { break };
        pending.insert(slot, b);
        while let Some(b) = pending.remove(&next) {
            if let Some(b) = b {
                let evs = decode_block(next, &b, &cfg, &mut st);
                for e in &evs {
                    match e {
                        Ev::Create(c) => {
                            n_create += 1;
                            if !c.mint.to_b58().ends_with("pump") {
                                non_pump_suffix += 1;
                            }
                        }
                        Ev::Trade(_) => n_trade += 1,
                        Ev::Fund(_) => n_fund += 1,
                        Ev::TokXfer(_) => n_xfer += 1,
                        Ev::Block(s) => n_pump_failed += s.pump_failed as u64,
                        _ => {}
                    }
                }
                writer.write_block(next, &evs)?;
                n_blocks += 1;
            }
            next += 1;
            if (next - start) % 1000 == 0 {
                let el = t0.elapsed().as_secs_f64();
                tracing::info!(
                    "slot {next} ({:.1}%) {:.1} slots/s blocks={n_blocks} creates={n_create} trades={n_trade} funds={n_fund} tokxfers={n_xfer} pump_failed_tx={n_pump_failed} throttled={} non_pump_suffix_mints={non_pump_suffix}",
                    100.0 * (next - start) as f64 / (end - start) as f64,
                    (next - start) as f64 / el,
                    throttled.load(Ordering::Relaxed)
                );
                writer.flush()?;
            }
        }
    }
    writer.close()?;
    tracing::info!("done: blocks={n_blocks} creates={n_create} trades={n_trade} funds={n_fund} tokxfers={n_xfer} in {:.0}s", t0.elapsed().as_secs_f64());
    Ok(())
}
