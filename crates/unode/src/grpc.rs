//! Yellowstone gRPC source → `TxView` / block boundaries, with automatic reconnect.

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use ucore::pk::{Pk, Sig};
use ucore::tx::{Ix, TokBal, TxView};
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterBlocksMeta,
    SubscribeRequestFilterTransactions, SubscribeRequestPing, SubscribeUpdateTransaction,
};

#[derive(Clone, Debug)]
pub struct GrpcCfg {
    pub endpoint: String,
    pub x_token: Option<String>,
    /// "processed" for trading, "confirmed" for recording.
    pub commitment: String,
    /// Subscribe to every non-vote transaction (needed for fresh-wallet funding lineage).
    pub firehose: bool,
    /// Include failed transactions touching pump (competition stats) and our own wallet.
    pub include_failed: bool,
    /// Extra accounts whose transactions must always be streamed (our wallet, watched funders).
    pub extra_accounts: Vec<String>,
}

pub enum SourceMsg {
    Tx(Box<TxView>),
    BlockMeta { slot: u64, parent: u64, block_time: i64, tx_count: u64 },
    Disconnected(String),
}

pub fn now_us() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

fn pk(b: &[u8]) -> Pk {
    Pk::from_slice(b).unwrap_or_default()
}

pub fn convert_tx(slot: u64, u: &SubscribeUpdateTransaction, recv_us: u64) -> Option<TxView> {
    let info = u.transaction.as_ref()?;
    if info.is_vote {
        return None;
    }
    let tx = info.transaction.as_ref()?;
    let msg = tx.message.as_ref()?;
    let meta = info.meta.as_ref()?;
    let mut keys: Vec<Pk> = msg.account_keys.iter().map(|k| pk(k)).collect();
    keys.extend(meta.loaded_writable_addresses.iter().map(|k| pk(k)));
    keys.extend(meta.loaded_readonly_addresses.iter().map(|k| pk(k)));
    let ixs = msg
        .instructions
        .iter()
        .map(|i| Ix { program: i.program_id_index as u16, accounts: i.accounts.iter().map(|a| *a as u16).collect(), data: i.data.clone() })
        .collect();
    let inner = meta
        .inner_instructions
        .iter()
        .map(|ii| {
            (
                ii.index as u16,
                ii.instructions
                    .iter()
                    .map(|i| Ix { program: i.program_id_index as u16, accounts: i.accounts.iter().map(|a| *a as u16).collect(), data: i.data.clone() })
                    .collect(),
            )
        })
        .collect();
    let tok = |v: &Vec<yellowstone_grpc_proto::prelude::TokenBalance>| -> Vec<TokBal> {
        v.iter()
            .filter_map(|b| {
                Some(TokBal {
                    account_index: b.account_index as u16,
                    mint: Pk::from_b58(&b.mint).ok()?,
                    owner: Pk::from_b58(&b.owner).ok()?,
                    amount: b.ui_token_amount.as_ref()?.amount.parse().ok()?,
                })
            })
            .collect()
    };
    Some(TxView {
        slot,
        tx_idx: info.index as u32,
        block_time: 0,
        recv_us,
        sig: Sig::from_slice(&info.signature).unwrap_or_default(),
        keys,
        ixs,
        inner,
        logs: if meta.inner_instructions.is_empty() { meta.log_messages.clone() } else { Vec::new() },
        failed: meta.err.is_some(),
        fee: meta.fee,
        pre_bal: meta.pre_balances.clone(),
        post_bal: meta.post_balances.clone(),
        pre_tok: tok(&meta.pre_token_balances),
        post_tok: tok(&meta.post_token_balances),
    })
}

fn request(cfg: &GrpcCfg) -> SubscribeRequest {
    let commitment = match cfg.commitment.as_str() {
        "confirmed" => CommitmentLevel::Confirmed,
        "finalized" => CommitmentLevel::Finalized,
        _ => CommitmentLevel::Processed,
    };
    let mut txs = HashMap::new();
    if cfg.firehose {
        txs.insert(
            "all".to_string(),
            SubscribeRequestFilterTransactions { vote: Some(false), failed: Some(false), ..Default::default() },
        );
    }
    let mut pump_accounts = vec![ucore::pump::PUMP_PROGRAM.to_b58()];
    pump_accounts.extend(cfg.extra_accounts.iter().cloned());
    txs.insert(
        "pump".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: if cfg.include_failed { None } else { Some(false) },
            account_include: pump_accounts,
            ..Default::default()
        },
    );
    let mut bm = HashMap::new();
    bm.insert("blocks".to_string(), SubscribeRequestFilterBlocksMeta {});
    SubscribeRequest { transactions: txs, blocks_meta: bm, commitment: Some(commitment as i32), ..Default::default() }
}

/// Run forever: connect, subscribe, forward messages; reconnect with backoff.
pub async fn run_source(cfg: GrpcCfg, out: mpsc::Sender<SourceMsg>) -> Result<()> {
    let mut backoff = 250u64;
    loop {
        let res: Result<()> = async {
            let mut client = GeyserGrpcClient::build_from_shared(cfg.endpoint.clone())?
                .x_token(cfg.x_token.clone())?
                .connect_timeout(Duration::from_secs(5))
                .tcp_nodelay(true)
                .http2_adaptive_window(true)
                .max_decoding_message_size(64 * 1024 * 1024)
                .connect()
                .await?;
            let (mut sink, mut stream) = client.subscribe_with_request(Some(request(&cfg))).await?;
            tracing::info!("gRPC subscribed ({}, firehose={})", cfg.commitment, cfg.firehose);
            while let Some(msg) = stream.next().await {
                let msg = msg?;
                let recv = now_us();
                match msg.update_oneof {
                    Some(UpdateOneof::Transaction(t)) => {
                        if let Some(tv) = convert_tx(t.slot, &t, recv) {
                            if out.send(SourceMsg::Tx(Box::new(tv))).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Some(UpdateOneof::BlockMeta(b)) => {
                        let bt = b.block_time.map(|x| x.timestamp).unwrap_or(0);
                        let _ = out
                            .send(SourceMsg::BlockMeta { slot: b.slot, parent: b.parent_slot, block_time: bt, tx_count: b.executed_transaction_count })
                            .await;
                    }
                    Some(UpdateOneof::Ping(_)) => {
                        let _ = sink.send(SubscribeRequest { ping: Some(SubscribeRequestPing { id: 1 }), ..Default::default() }).await;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        .await;
        let why = match &res {
            Ok(()) => "stream ended".to_string(),
            Err(e) => e.to_string(),
        };
        tracing::warn!("gRPC disconnected: {why}; reconnecting in {backoff} ms");
        let _ = out.send(SourceMsg::Disconnected(why)).await;
        tokio::time::sleep(Duration::from_millis(backoff)).await;
        backoff = (backoff * 2).min(5_000);
    }
}
