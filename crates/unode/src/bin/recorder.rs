//! 24/7 recorder: Yellowstone gRPC (confirmed) → ordered, decoded events → event store.
//! Run this for the 30-day data phase. Firehose mode also captures fresh-wallet fundings
//! (needed for operator lineage); without it only pump transactions are stored.

use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use ucore::event::{BlockStat, Ev};
use ucore::store::StoreWriter;
use ucore::tx::{decode_tx, DecodeCfg, DecodeState, TxClass, TxView};
use unode::grpc::{run_source, GrpcCfg, SourceMsg};

#[derive(Parser)]
struct Args {
    #[arg(long, env = "GRPC_ENDPOINT")]
    endpoint: String,
    #[arg(long, env = "GRPC_X_TOKEN")]
    x_token: Option<String>,
    #[arg(long, default_value = "data/live")]
    out: String,
    /// Stream every non-vote transaction (≈40–80 Mbit/s) to capture funding lineage.
    #[arg(long)]
    firehose: bool,
    /// Extra wallets to always record (comma separated): exchange hot wallets, treasuries.
    #[arg(long, default_value = "")]
    watch: String,
    #[arg(long, default_value_t = 9000)]
    rotate: u64,
    /// Also stream and decode PumpSwap (graduated coins, boost buy-and-burn).
    #[arg(long)]
    amm: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?)).init();
    let a = Args::parse();
    let watch: Vec<String> = a.watch.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
    let mut dcfg = DecodeCfg { decode_amm: a.amm, ..DecodeCfg::default() };
    for w in &watch {
        dcfg.watch.insert(w.parse()?);
    }
    let gcfg = GrpcCfg {
        endpoint: a.endpoint.clone(),
        x_token: a.x_token.clone(),
        commitment: "confirmed".into(),
        firehose: a.firehose,
        include_failed: true,
        extra_accounts: if a.amm { watch.iter().cloned().chain([ucore::amm::PUMP_AMM.to_b58()]).collect() } else { watch.clone() },
    };
    let (tx, mut rx) = mpsc::channel::<SourceMsg>(200_000);
    tokio::spawn(run_source(gcfg, tx));
    let mut writer = StoreWriter::new(&a.out, a.rotate)?;
    let mut st = DecodeState::default();
    let mut buf: BTreeMap<u64, Vec<TxView>> = BTreeMap::new();
    let (mut n_blocks, mut n_ev, mut n_tx) = (0u64, 0u64, 0u64);
    let mut last_log = Instant::now();
    let mut last_flush = Instant::now();
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => { tracing::info!("shutting down"); break; }
            m = rx.recv() => {
                let Some(m) = m else { break };
                match m {
                    SourceMsg::Tx(tv) => { n_tx += 1; buf.entry(tv.slot).or_default().push(*tv); }
                    SourceMsg::BlockMeta { slot, parent, block_time, tx_count } => {
                        // flush this slot and anything older that never got its meta
                        let ready: Vec<u64> = buf.range(..=slot).map(|(s, _)| *s).collect();
                        for s in ready {
                            let mut txs = buf.remove(&s).unwrap_or_default();
                            txs.sort_by_key(|t| t.tx_idx);
                            let mut evs = Vec::new();
                            let mut stat = BlockStat { slot: s, parent_slot: parent, block_time, n_tx: tx_count as u32, ..Default::default() };
                            for t in txs.iter_mut() {
                                t.block_time = block_time;
                                match decode_tx(t, &dcfg, &mut st, &mut evs) {
                                    TxClass::PumpOk => stat.pump_ok += 1,
                                    TxClass::PumpFailed => stat.pump_failed += 1,
                                    TxClass::Other => {}
                                }
                            }
                            if s != slot { stat.parent_slot = 0; }
                            evs.push(Ev::Block(stat));
                            n_ev += evs.len() as u64;
                            writer.write_block(s, &evs)?;
                            n_blocks += 1;
                        }
                    }
                    SourceMsg::Disconnected(why) => tracing::warn!("source disconnected: {why}"),
                }
            }
        }
        if last_flush.elapsed() > Duration::from_secs(2) {
            writer.flush()?;
            last_flush = Instant::now();
        }
        if last_log.elapsed() > Duration::from_secs(30) {
            tracing::info!("blocks {n_blocks} txs {n_tx} events {n_ev} buffered_slots {} last_slot {}", buf.len(), writer.last_slot());
            last_log = Instant::now();
        }
    }
    writer.close()?;
    Ok(())
}
