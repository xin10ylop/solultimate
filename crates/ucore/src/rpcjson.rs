//! Convert JSON-RPC `getBlock` (encoding=json, transactionDetails=full) into `TxView`s.

use crate::event::{BlockStat, Ev};
use crate::pk::{Pk, Sig};
use crate::pump;
use crate::tx::{decode_tx, DecodeCfg, DecodeState, Ix, TokBal, TxClass, TxView};
use anyhow::{anyhow, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct RpcResp<T> {
    pub result: Option<T>,
    pub error: Option<RpcErr>,
}
#[derive(Deserialize, Debug)]
pub struct RpcErr {
    pub code: i64,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlock {
    pub block_time: Option<i64>,
    pub parent_slot: u64,
    pub transactions: Vec<RpcTx>,
}
#[derive(Deserialize)]
pub struct RpcTx {
    pub transaction: RpcTxInner,
    pub meta: Option<RpcMeta>,
}
#[derive(Deserialize)]
pub struct RpcTxInner {
    pub signatures: Vec<String>,
    pub message: RpcMsg,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcMsg {
    pub account_keys: Vec<String>,
    pub instructions: Vec<RpcIx>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcIx {
    pub program_id_index: u16,
    pub accounts: Vec<u16>,
    pub data: String,
}
#[derive(Deserialize)]
pub struct RpcInner {
    pub index: u16,
    pub instructions: Vec<RpcIx>,
}
#[derive(Deserialize)]
pub struct RpcLoaded {
    #[serde(default)]
    pub writable: Vec<String>,
    #[serde(default)]
    pub readonly: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcUiAmt {
    pub amount: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTokBal {
    pub account_index: u16,
    pub mint: String,
    pub owner: Option<String>,
    pub ui_token_amount: RpcUiAmt,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcMeta {
    pub err: Option<serde_json::Value>,
    #[serde(default)]
    pub fee: u64,
    #[serde(default)]
    pub pre_balances: Vec<u64>,
    #[serde(default)]
    pub post_balances: Vec<u64>,
    pub inner_instructions: Option<Vec<RpcInner>>,
    pub loaded_addresses: Option<RpcLoaded>,
    pub pre_token_balances: Option<Vec<RpcTokBal>>,
    pub post_token_balances: Option<Vec<RpcTokBal>>,
    pub log_messages: Option<Vec<String>>,
}

fn needs_data(p: &Pk) -> bool {
    *p == pump::PUMP_PROGRAM || *p == pump::SYSTEM_PROGRAM || *p == pump::COMPUTE_BUDGET || *p == crate::amm::PUMP_AMM
}

fn conv_ix(ix: &RpcIx, keys: &[Pk]) -> Ix {
    let prog = keys.get(ix.program_id_index as usize).copied().unwrap_or_default();
    let data = if needs_data(&prog) {
        bs58::decode(&ix.data).into_vec().unwrap_or_default()
    } else {
        Vec::new()
    };
    Ix { program: ix.program_id_index, accounts: ix.accounts.clone(), data }
}

fn conv_tok(v: &Option<Vec<RpcTokBal>>, pump_mints_only: bool) -> Vec<TokBal> {
    let mut out = Vec::new();
    if let Some(v) = v {
        for b in v {
            if pump_mints_only && !b.mint.ends_with("pump") {
                continue;
            }
            let (Ok(mint), Some(owner)) = (Pk::from_b58(&b.mint), b.owner.as_deref()) else { continue };
            let Ok(owner) = Pk::from_b58(owner) else { continue };
            out.push(TokBal {
                account_index: b.account_index,
                mint,
                owner,
                amount: b.ui_token_amount.amount.parse().unwrap_or(0),
            });
        }
    }
    out
}

/// Convert one RPC transaction. Returns None for vote / malformed transactions.
pub fn to_txview(slot: u64, idx: u32, block_time: i64, t: &RpcTx) -> Option<TxView> {
    let meta = t.meta.as_ref()?;
    let mut keys: Vec<Pk> = Vec::with_capacity(t.transaction.message.account_keys.len() + 8);
    for k in &t.transaction.message.account_keys {
        keys.push(Pk::from_b58(k).ok()?);
    }
    if let Some(l) = &meta.loaded_addresses {
        for k in l.writable.iter().chain(l.readonly.iter()) {
            keys.push(Pk::from_b58(k).ok()?);
        }
    }
    if keys.iter().any(|k| *k == pump::VOTE_PROGRAM) {
        return None;
    }
    let ixs = t.transaction.message.instructions.iter().map(|i| conv_ix(i, &keys)).collect();
    let inner = meta
        .inner_instructions
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|ii| (ii.index, ii.instructions.iter().map(|i| conv_ix(i, &keys)).collect()))
                .collect()
        })
        .unwrap_or_default();
    let pump_touched = keys.contains(&pump::PUMP_PROGRAM);
    Some(TxView {
        slot,
        tx_idx: idx,
        block_time,
        recv_us: 0,
        sig: Sig::from_b58(t.transaction.signatures.first()?).ok()?,
        keys,
        ixs,
        inner,
        logs: if pump_touched { meta.log_messages.clone().unwrap_or_default() } else { Vec::new() },
        failed: meta.err.is_some(),
        fee: meta.fee,
        pre_bal: meta.pre_balances.clone(),
        post_bal: meta.post_balances.clone(),
        pre_tok: conv_tok(&meta.pre_token_balances, false),
        post_tok: conv_tok(&meta.post_token_balances, false),
    })
}

/// Decode a full block into events (+ a trailing BlockStat).
pub fn decode_block(slot: u64, b: &RpcBlock, cfg: &DecodeCfg, st: &mut DecodeState) -> Vec<Ev> {
    let bt = b.block_time.unwrap_or(0);
    let mut out = Vec::new();
    let mut stat = BlockStat {
        slot,
        parent_slot: b.parent_slot,
        block_time: bt,
        n_tx: b.transactions.len() as u32,
        ..Default::default()
    };
    for (i, t) in b.transactions.iter().enumerate() {
        let Some(tv) = to_txview(slot, i as u32, bt, t) else { continue };
        match decode_tx(&tv, cfg, st, &mut out) {
            TxClass::PumpOk => stat.pump_ok += 1,
            TxClass::PumpFailed => stat.pump_failed += 1,
            TxClass::Other => {}
        }
    }
    out.push(Ev::Block(stat));
    out
}

/// Parse a raw getBlock response body. Ok(None) = skipped / missing slot.
pub fn parse_block_response(body: &[u8]) -> Result<Option<RpcBlock>> {
    let r: RpcResp<RpcBlock> = serde_json::from_slice(body)?;
    if let Some(e) = r.error {
        // -32007 / -32009: slot skipped or missing in long-term storage
        if e.code == -32007 || e.code == -32009 || e.code == -32004 {
            return Ok(None);
        }
        return Err(anyhow!("rpc error {}: {}", e.code, e.message));
    }
    Ok(r.result)
}
