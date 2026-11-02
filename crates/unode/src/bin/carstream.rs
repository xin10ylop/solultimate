//! Stream one Old Faithful epoch archive (CAR v1, https://files.old-faithful.net/<epoch>/epoch-<epoch>.car)
//! over parallel HTTP range requests and decode it into the event store, exactly like `backfill`
//! does from `getBlock` (same `TxView` → `decode_tx` path, same file format).
//!
//! Nothing but the decoded events touches the disk. Resumable at file-rotation boundaries
//! (`resume.txt` = "<byte offset> <next slot>").
//!
//! Node layout (yellowstone-faithful ledger schema, CBOR tuples, first field = kind):
//!   Transaction 0: [0, data DataFrame, metadata DataFrame, slot, index?]
//!   Entry       1: [1, num_hashes, hash, [tx links]]
//!   Block       2: [2, slot, shredding, [entry links], [parent_slot, blocktime, block_height?], rewards link]
//!   DataFrame   6: [6, hash?, index?, total?, data, [next links]?]
//! The archive is written bottom-up, so all transactions of a slot precede its Block node.
//! Transaction data = wire-format VersionedTransaction; metadata = zstd(protobuf TransactionStatusMeta).

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use futures::StreamExt;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use ucore::event::{BlockStat, Ev};
use ucore::pk::{Pk, Sig};
use ucore::pump;
use ucore::store::StoreWriter;
use ucore::tx::{decode_tx, DecodeCfg, DecodeState, Ix, TokBal, TxClass, TxView};
use prost::Message;
use yellowstone_grpc_proto::prelude::{InnerInstructions, TokenBalance, TransactionError};

/// The subset of `TransactionStatusMeta` the decoder needs (logs, rewards, return data skipped).
#[derive(Clone, PartialEq, prost::Message)]
struct SlimMeta {
    #[prost(message, optional, tag = "1")]
    err: Option<TransactionError>,
    #[prost(uint64, tag = "2")]
    fee: u64,
    #[prost(uint64, repeated, tag = "3")]
    pre_balances: Vec<u64>,
    #[prost(uint64, repeated, tag = "4")]
    post_balances: Vec<u64>,
    #[prost(message, repeated, tag = "5")]
    inner_instructions: Vec<InnerInstructions>,
    #[prost(message, repeated, tag = "7")]
    pre_token_balances: Vec<TokenBalance>,
    #[prost(message, repeated, tag = "8")]
    post_token_balances: Vec<TokenBalance>,
    #[prost(bytes = "vec", repeated, tag = "12")]
    loaded_writable_addresses: Vec<Vec<u8>>,
    #[prost(bytes = "vec", repeated, tag = "13")]
    loaded_readonly_addresses: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct LogsOnly {
    #[prost(string, repeated, tag = "6")]
    log_messages: Vec<String>,
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    epoch: u64,
    #[arg(long, default_value = "https://files.old-faithful.net")]
    base: String,
    #[arg(long)]
    out: PathBuf,
    /// Only write blocks with slot in [from_slot, to_slot); stops reading once past to_slot.
    #[arg(long, default_value_t = 0)]
    from_slot: u64,
    #[arg(long, default_value_t = u64::MAX)]
    to_slot: u64,
    #[arg(long, default_value_t = 20)]
    conns: usize,
    #[arg(long, default_value_t = 32)]
    chunk_mb: u64,
    #[arg(long, default_value_t = 9000)]
    rotate: u64,
    #[arg(long)]
    amm: bool,
    #[arg(long, default_value_t = 3)]
    workers: usize,
    /// Stop after this many bytes (testing).
    #[arg(long, default_value_t = 0)]
    max_bytes: u64,
    /// Zero the signatures of Fund / TokXfer events (unused by the brain; saves disk).
    #[arg(long)]
    slim: bool,
    /// zstd level of the written segments.
    #[arg(long, default_value_t = 9)]
    level: i32,
    /// Continue from <out>/resume.txt if present.
    #[arg(long)]
    resume: bool,
}

// ---------------------------------------------------------------- download

struct ChunkReader {
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    cur: Vec<u8>,
    pos: usize,
    consumed: u64,
}

impl ChunkReader {
    fn fill(&mut self) -> bool {
        while self.pos >= self.cur.len() {
            match self.rx.blocking_recv() {
                Some(c) => {
                    self.cur = c;
                    self.pos = 0;
                }
                None => return false,
            }
        }
        true
    }
    fn byte(&mut self) -> Option<u8> {
        if !self.fill() {
            return None;
        }
        let b = self.cur[self.pos];
        self.pos += 1;
        self.consumed += 1;
        Some(b)
    }
    fn uvarint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut s = 0;
        loop {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64) << s;
            if b & 0x80 == 0 {
                return Some(v);
            }
            s += 7;
            if s > 63 {
                return None;
            }
        }
    }
    fn read_into(&mut self, out: &mut Vec<u8>, mut n: usize) -> bool {
        out.clear();
        while n > 0 {
            if !self.fill() {
                return false;
            }
            let k = n.min(self.cur.len() - self.pos);
            out.extend_from_slice(&self.cur[self.pos..self.pos + k]);
            self.pos += k;
            self.consumed += k as u64;
            n -= k;
        }
        true
    }
}

async fn fetch_range(client: &reqwest::Client, url: &str, a: u64, b: u64, retries: &AtomicU64) -> Vec<u8> {
    let mut backoff = 500u64;
    loop {
        let r = client.get(url).header("Range", format!("bytes={a}-{b}")).send().await;
        match r {
            Ok(r) if r.status().as_u16() == 206 => match r.bytes().await {
                Ok(by) if by.len() as u64 == b - a + 1 => return by.to_vec(),
                Ok(by) => tracing::warn!("range {a}-{b}: short body {}", by.len()),
                Err(e) => tracing::warn!("range {a}-{b}: body {e}"),
            },
            Ok(r) => tracing::warn!("range {a}-{b}: status {}", r.status()),
            Err(e) => tracing::warn!("range {a}-{b}: {e}"),
        }
        retries.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(backoff)).await;
        backoff = (backoff * 2).min(15_000);
    }
}

fn start_download(url: String, start: u64, end: u64, chunk: u64, conns: usize, retries: Arc<AtomicU64>) -> ChunkReader {
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async move {
            let client = reqwest::Client::builder()
                .http1_only()
                .timeout(Duration::from_secs(300))
                .pool_max_idle_per_host(conns * 2)
                .build()
                .unwrap();
            let mut ranges = Vec::new();
            let mut a = start;
            while a < end {
                let b = (a + chunk).min(end) - 1;
                ranges.push((a, b));
                a = b + 1;
            }
            let mut s = futures::stream::iter(ranges)
                .map(|(a, b)| {
                    let client = client.clone();
                    let url = url.clone();
                    let retries = retries.clone();
                    async move { fetch_range(&client, &url, a, b, &retries).await }
                })
                .buffered(conns);
            while let Some(c) = s.next().await {
                if tx.send(c).await.is_err() {
                    break;
                }
            }
        });
    });
    ChunkReader { rx, cur: Vec::new(), pos: 0, consumed: 0 }
}

// ---------------------------------------------------------------- CBOR (just enough for the ledger nodes)

#[derive(Debug)]
enum C<'a> {
    U(u64),
    N,
    B(&'a [u8]),
    T,
    A(Vec<C<'a>>),
    M,
    Tag(u64, Box<C<'a>>),
    Null,
    Other,
}

fn cbor<'a>(b: &'a [u8], p: &mut usize) -> Result<C<'a>> {
    let h = *b.get(*p).context("cbor eof")?;
    *p += 1;
    let major = h >> 5;
    let info = h & 0x1f;
    let mut arg = |p: &mut usize| -> Result<Option<u64>> {
        Ok(Some(match info {
            0..=23 => info as u64,
            24 => {
                let v = *b.get(*p).context("eof")? as u64;
                *p += 1;
                v
            }
            25 => {
                let v = u16::from_be_bytes(b.get(*p..*p + 2).context("eof")?.try_into()?) as u64;
                *p += 2;
                v
            }
            26 => {
                let v = u32::from_be_bytes(b.get(*p..*p + 4).context("eof")?.try_into()?) as u64;
                *p += 4;
                v
            }
            27 => {
                let v = u64::from_be_bytes(b.get(*p..*p + 8).context("eof")?.try_into()?);
                *p += 8;
                v
            }
            31 => return Ok(None),
            _ => bail!("cbor info {info}"),
        }))
    };
    Ok(match major {
        0 => C::U(arg(p)?.context("indef uint")?),
        1 => {
            arg(p)?;
            C::N
        }
        2 | 3 => {
            let n = arg(p)?.context("indefinite string unsupported")? as usize;
            let s = b.get(*p..*p + n).context("string eof")?;
            *p += n;
            if major == 2 {
                C::B(s)
            } else {
                C::T
            }
        }
        4 => match arg(p)? {
            Some(n) => {
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push(cbor(b, p)?);
                }
                C::A(v)
            }
            None => {
                let mut v = Vec::new();
                while *b.get(*p).context("eof")? != 0xff {
                    v.push(cbor(b, p)?);
                }
                *p += 1;
                C::A(v)
            }
        },
        5 => {
            let n = arg(p)?.context("indefinite map unsupported")?;
            for _ in 0..n * 2 {
                cbor(b, p)?;
            }
            C::M
        }
        6 => {
            let t = arg(p)?.context("tag")?;
            C::Tag(t, Box::new(cbor(b, p)?))
        }
        _ => match info {
            22 | 23 => C::Null,
            20 | 21 => C::Other,
            25 => {
                *p += 2;
                C::Other
            }
            26 => {
                *p += 4;
                C::Other
            }
            27 => {
                *p += 8;
                C::Other
            }
            _ => C::Other,
        },
    })
}

fn u(c: Option<&C>) -> Option<u64> {
    match c {
        Some(C::U(v)) => Some(*v),
        _ => None,
    }
}

/// CID bytes of a link (tag 42, identity multibase prefix 0x00).
fn link<'a>(c: &C<'a>) -> Option<&'a [u8]> {
    match c {
        C::Tag(42, b) => match b.as_ref() {
            C::B(x) if !x.is_empty() => Some(&x[1..]),
            _ => None,
        },
        _ => None,
    }
}

fn uv(b: &[u8], p: &mut usize) -> Result<u64> {
    let mut v = 0u64;
    let mut s = 0;
    loop {
        let x = *b.get(*p).context("varint eof")?;
        *p += 1;
        v |= ((x & 0x7f) as u64) << s;
        if x & 0x80 == 0 {
            return Ok(v);
        }
        s += 7;
        if s > 63 {
            bail!("varint");
        }
    }
}

fn cid_len(b: &[u8]) -> Result<usize> {
    if b.len() >= 2 && b[0] == 0x12 && b[1] == 0x20 {
        return Ok(34);
    }
    let mut p = 0;
    uv(b, &mut p)?; // version
    uv(b, &mut p)?; // codec
    uv(b, &mut p)?; // multihash code
    let l = uv(b, &mut p)? as usize;
    Ok(p + l)
}

/// One data frame: (index, data, next links).
struct Frame {
    index: u64,
    data: Vec<u8>,
    next: Vec<Vec<u8>>,
}

fn frame(c: &C) -> Option<Frame> {
    let C::A(a) = c else { return None };
    let data = match a.get(4) {
        Some(C::B(x)) => x.to_vec(),
        _ => return None,
    };
    let next = match a.get(5) {
        Some(C::A(v)) => v.iter().filter_map(|l| link(l).map(|x| x.to_vec())).collect(),
        _ => Vec::new(),
    };
    Some(Frame { index: u(a.get(2)).unwrap_or(0), data, next })
}

/// Reassemble a possibly split frame from the frames seen so far. None if a piece is missing.
fn assemble(first: Frame, frames: &FxHashMap<Vec<u8>, Frame>) -> Option<Vec<u8>> {
    if first.next.is_empty() {
        return Some(first.data);
    }
    let mut parts: Vec<(u64, &[u8])> = vec![(first.index, &first.data[..])];
    let mut seen: Vec<&[u8]> = Vec::new();
    let mut stack: Vec<&[u8]> = first.next.iter().map(|v| v.as_slice()).collect();
    while let Some(cid) = stack.pop() {
        if seen.contains(&cid) {
            continue;
        }
        seen.push(cid);
        let f = frames.get(cid)?;
        parts.push((f.index, &f.data));
        stack.extend(f.next.iter().map(|v| v.as_slice()));
    }
    parts.sort_by_key(|x| x.0);
    let mut out = Vec::with_capacity(parts.iter().map(|x| x.1.len()).sum());
    for (_, d) in parts {
        out.extend_from_slice(d);
    }
    Some(out)
}

// ---------------------------------------------------------------- transactions

struct RawTx {
    idx: u32,
    data: Vec<u8>,
    meta: Vec<u8>,
}

struct SlotBatch {
    slot: u64,
    parent: u64,
    block_time: i64,
    n_tx: u32,
    /// Byte offset where this slot's records start (resume point).
    start_off: u64,
    txs: Vec<RawTx>,
}

fn shortvec(b: &[u8], p: &mut usize) -> Result<usize> {
    let mut v = 0usize;
    let mut s = 0;
    loop {
        let x = *b.get(*p).context("shortvec eof")?;
        *p += 1;
        v |= ((x & 0x7f) as usize) << s;
        if x & 0x80 == 0 {
            return Ok(v);
        }
        s += 7;
        if s > 21 {
            bail!("shortvec");
        }
    }
}

fn needs_data(p: &Pk) -> bool {
    *p == pump::PUMP_PROGRAM || *p == pump::SYSTEM_PROGRAM || *p == pump::COMPUTE_BUDGET || *p == ucore::amm::PUMP_AMM
}

#[derive(Default)]
struct Stats {
    txs: u64,
    votes: u64,
    meta_err: u64,
    wire_err: u64,
    alt_tx: u64,
    alt_loaded: u64,
}

struct Conv {
    pk_cache: FxHashMap<String, Pk>,
    zd: zstd::bulk::Decompressor<'static>,
    buf: Vec<u8>,
}

fn keys_have_pump(keys: &[Pk], m: &Result<SlimMeta, prost::DecodeError>) -> bool {
    if keys.contains(&pump::PUMP_PROGRAM) {
        return true;
    }
    match m {
        Ok(m) => m.loaded_writable_addresses.iter().chain(m.loaded_readonly_addresses.iter()).any(|k| k[..] == pump::PUMP_PROGRAM.0[..]),
        Err(_) => false,
    }
}

impl Conv {
    fn pk_s(&mut self, s: &str) -> Option<Pk> {
        if let Some(p) = self.pk_cache.get(s) {
            return Some(*p);
        }
        let p = Pk::from_b58(s).ok()?;
        if self.pk_cache.len() > 2_000_000 {
            self.pk_cache.clear();
        }
        self.pk_cache.insert(s.to_string(), p);
        Some(p)
    }

    fn tok(&mut self, v: &[TokenBalance]) -> Vec<TokBal> {
        let mut out = Vec::with_capacity(v.len());
        for b in v {
            let (Some(mint), Some(owner)) = (self.pk_s(&b.mint), if b.owner.is_empty() { None } else { self.pk_s(&b.owner) }) else { continue };
            let amount = b.ui_token_amount.as_ref().and_then(|a| a.amount.parse().ok()).unwrap_or(0);
            out.push(TokBal { account_index: b.account_index as u16, mint, owner, amount });
        }
        out
    }

    /// Same semantics as `rpcjson::to_txview`: None for votes / malformed.
    fn convert(&mut self, slot: u64, t: &RawTx, st: &mut Stats) -> Option<TxView> {
        st.txs += 1;
        let b = &t.data;
        let mut p = 0usize;
        let parsed = (|| -> Result<(Sig, Vec<Pk>, Vec<(u8, Vec<u16>, Vec<u8>)>, usize)> {
            if b.first() == Some(&0x81) {
                // v1 layout: version, header(3), config mask u32, blockhash, n_ix u8, n_keys u8, keys,
                // config words, ix headers (prog u8, n_acc u8, data_len u16), ix payloads, signatures
                let nsig = *b.get(1).context("v1 header")? as usize;
                let mask = u32::from_le_bytes(b.get(4..8).context("v1 mask")?.try_into()?);
                p = 8 + 32;
                let ni = *b.get(p).context("v1 n_ix")? as usize;
                let nk = *b.get(p + 1).context("v1 n_keys")? as usize;
                p += 2;
                let mut keys = Vec::with_capacity(nk + 4);
                for _ in 0..nk {
                    keys.push(Pk::from_slice(b.get(p..p + 32).context("key")?).context("key")?);
                    p += 32;
                }
                let cfg_words = mask.count_ones() as usize;
                p += 4 * cfg_words;
                let mut hdr = Vec::with_capacity(ni);
                for _ in 0..ni {
                    let h = b.get(p..p + 4).context("v1 ix header")?;
                    hdr.push((h[0], h[1] as usize, u16::from_le_bytes([h[2], h[3]]) as usize));
                    p += 4;
                }
                let mut ixs = Vec::with_capacity(ni);
                for (prog, na, nd) in hdr {
                    let acc: Vec<u16> = b.get(p..p + na).context("v1 ix accounts")?.iter().map(|x| *x as u16).collect();
                    p += na;
                    let data = b.get(p..p + nd).context("v1 ix data")?.to_vec();
                    p += nd;
                    ixs.push((prog, acc, data));
                }
                if nsig == 0 {
                    bail!("no signatures");
                }
                let sig = Sig::from_slice(b.get(p..p + 64).context("v1 sig")?).context("sig")?;
                return Ok((sig, keys, ixs, 0));
            }
            let ns = shortvec(b, &mut p)?;
            if ns == 0 {
                bail!("no signatures");
            }
            let sig = Sig::from_slice(b.get(p..p + 64).context("sig")?).context("sig")?;
            p += 64 * ns;
            let v0 = *b.get(p).context("msg")? & 0x80 != 0;
            if v0 {
                p += 1;
            }
            p += 3;
            let nk = shortvec(b, &mut p)?;
            let mut keys = Vec::with_capacity(nk + 8);
            for _ in 0..nk {
                keys.push(Pk::from_slice(b.get(p..p + 32).context("key")?).context("key")?);
                p += 32;
            }
            p += 32;
            let ni = shortvec(b, &mut p)?;
            let mut ixs = Vec::with_capacity(ni);
            for _ in 0..ni {
                let prog = *b.get(p).context("ix")?;
                p += 1;
                let na = shortvec(b, &mut p)?;
                let acc: Vec<u16> = b.get(p..p + na).context("ix accounts")?.iter().map(|x| *x as u16).collect();
                p += na;
                let nd = shortvec(b, &mut p)?;
                let data = b.get(p..p + nd).context("ix data")?.to_vec();
                p += nd;
                ixs.push((prog, acc, data));
            }
            let nl = if v0 { shortvec(b, &mut p)? } else { 0 };
            Ok((sig, keys, ixs, nl))
        })();
        let Ok((sig, mut keys, raw_ixs, n_lookups)) = parsed.map_err(|e| {
            if st.wire_err < 3 && std::env::var("CAR_DEBUG").is_ok() {
                eprintln!("wire err {e}: len {} head {:02x?}", b.len(), &b[..b.len().min(200)]);
            }
        }) else {
            st.wire_err += 1;
            return None;
        };
        if keys.iter().any(|k| *k == pump::VOTE_PROGRAM) {
            st.votes += 1;
            return None;
        }
        // reusable context + buffer (a fresh zstd context per transaction dominated the profile)
        let mut mb = std::mem::take(&mut self.buf);
        mb.clear();
        let mut ok = false;
        for cap in [1usize << 16, 1 << 20, 1 << 24, 1 << 27] {
            if mb.capacity() < cap {
                mb.reserve(cap - mb.len());
            }
            mb.clear();
            if self.zd.decompress_to_buffer(&t.meta[..], &mut mb).is_ok() {
                ok = true;
                break;
            }
        }
        if !ok {
            // not compressed (or corrupt): try the raw bytes
            mb.clear();
            mb.extend_from_slice(&t.meta);
        }
        let meta_bytes = mb;
        let decoded = SlimMeta::decode(&meta_bytes[..]);
        let logs = if decoded.as_ref().map(|m| m.err.is_none()).unwrap_or(false) && keys_have_pump(&keys, &decoded) {
            LogsOnly::decode(&meta_bytes[..]).map(|l| l.log_messages).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.buf = meta_bytes;
        let Ok(meta) = decoded else {
            st.meta_err += 1;
            return None;
        };
        if n_lookups > 0 {
            st.alt_tx += 1;
            if !meta.loaded_writable_addresses.is_empty() || !meta.loaded_readonly_addresses.is_empty() {
                st.alt_loaded += 1;
            }
        }
        for k in meta.loaded_writable_addresses.iter().chain(meta.loaded_readonly_addresses.iter()) {
            keys.push(Pk::from_slice(k).unwrap_or_default());
        }
        if keys.iter().any(|k| *k == pump::VOTE_PROGRAM) {
            st.votes += 1;
            return None;
        }
        let mk_ix = |prog: u16, acc: Vec<u16>, data: Vec<u8>, keys: &[Pk]| -> Ix {
            let pk = keys.get(prog as usize).copied().unwrap_or_default();
            Ix { program: prog, accounts: acc, data: if needs_data(&pk) { data } else { Vec::new() } }
        };
        let ixs: Vec<Ix> = raw_ixs.into_iter().map(|(pr, acc, d)| mk_ix(pr as u16, acc, d, &keys)).collect();
        let inner: Vec<(u16, Vec<Ix>)> = meta
            .inner_instructions
            .iter()
            .map(|ii| {
                (
                    ii.index as u16,
                    ii.instructions
                        .iter()
                        .map(|i| mk_ix(i.program_id_index as u16, i.accounts.iter().map(|a| *a as u16).collect(), i.data.clone(), &keys))
                        .collect(),
                )
            })
            .collect();
        let pump_touched = keys.contains(&pump::PUMP_PROGRAM);
        let pre_tok = self.tok(&meta.pre_token_balances);
        let post_tok = self.tok(&meta.post_token_balances);
        Some(TxView {
            slot,
            tx_idx: t.idx,
            block_time: 0,
            recv_us: 0,
            sig,
            keys,
            ixs,
            inner,
            logs: if pump_touched { logs } else { Vec::new() },
            failed: meta.err.is_some(),
            fee: meta.fee,
            pre_bal: meta.pre_balances,
            post_bal: meta.post_balances,
            pre_tok,
            post_tok,
        })
    }
}

// ---------------------------------------------------------------- main

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?)).init();
    let a = Args::parse();
    let url = format!("{}/{}/epoch-{}.car", a.base, a.epoch, a.epoch);
    fs::create_dir_all(&a.out)?;

    // total size
    let total: u64 = {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let c = reqwest::Client::builder().http1_only().build()?;
            let r = c.head(&url).send().await?;
            let len = r.headers().get("content-length").context("no content-length")?.to_str()?.parse::<u64>()?;
            anyhow::Ok(len)
        })?
    };
    let mut start_off = 0u64;
    let mut from_slot = a.from_slot;
    let resume_path = a.out.join("resume.txt");
    if a.resume {
        if let Ok(s) = fs::read_to_string(&resume_path) {
            let v: Vec<u64> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 2 {
                start_off = v[0];
                from_slot = from_slot.max(v[1]);
                // drop partial segments at or after the resume slot
                for f in ucore::store::segments(&a.out)? {
                    let name = f.file_name().unwrap().to_string_lossy().to_string();
                    let s0: u64 = name.trim_start_matches("ev_").split(|c| c == '.' || c == '_').next().unwrap_or("0").parse().unwrap_or(0);
                    if s0 >= v[1] {
                        tracing::info!("removing partial segment {name}");
                        fs::remove_file(&f)?;
                    }
                }
            }
        }
    }
    let end = if a.max_bytes > 0 { (start_off + a.max_bytes).min(total) } else { total };
    tracing::info!("epoch {} archive {:.1} GB, reading bytes [{start_off}, {end}) → {}", a.epoch, total as f64 / 1e9, a.out.display());
    let retries = Arc::new(AtomicU64::new(0));
    let mut rd = start_download(url, start_off, end, a.chunk_mb << 20, a.conns, retries.clone());
    rd.consumed = start_off;

    // parser thread → workers → (reorder) → decoder/writer (this thread)
    let (btx, brx) = mpsc::sync_channel::<(u64, SlotBatch)>(256);
    let brx = Arc::new(Mutex::new(brx));
    let (rtx, rrx) = mpsc::sync_channel::<(u64, SlotBatch, Vec<TxView>, Stats)>(256);
    let to_slot = a.to_slot;
    let parser = std::thread::spawn(move || -> Result<u64> {
        if rd.consumed == 0 {
            let hl = rd.uvarint().context("car header")? as usize;
            let mut h = Vec::new();
            if !rd.read_into(&mut h, hl) {
                bail!("car header eof");
            }
        }
        let mut buf = Vec::with_capacity(1 << 20);
        let mut frames: FxHashMap<Vec<u8>, Frame> = FxHashMap::default();
        // per-slot pending transactions: (idx, data frame, meta frame)
        let mut pend: BTreeMap<u64, Vec<(Option<u64>, Frame, Frame)>> = BTreeMap::new();
        let mut seq = 0u64;
        let mut slot_start = rd.consumed;
        let mut n_nodes = 0u64;
        loop {
            let Some(len) = rd.uvarint() else { break };
            if !rd.read_into(&mut buf, len as usize) {
                break;
            }
            n_nodes += 1;
            let cl = cid_len(&buf)?;
            let node = &buf[cl..];
            let mut p = 0;
            let Ok(c) = cbor(node, &mut p) else {
                tracing::warn!("bad cbor node at {}", rd.consumed);
                continue;
            };
            let C::A(arr) = &c else { continue };
            match u(arr.first()) {
                Some(0) => {
                    let (Some(d), Some(m), Some(slot)) = (arr.get(1).and_then(frame), arr.get(2).and_then(frame), u(arr.get(3))) else {
                        tracing::warn!("bad tx node");
                        continue;
                    };
                    pend.entry(slot).or_default().push((u(arr.get(4)), d, m));
                }
                Some(6) => {
                    if let Some(f) = frame(&c) {
                        frames.insert(buf[..cl].to_vec(), f);
                    }
                }
                Some(2) => {
                    let Some(slot) = u(arr.get(1)) else { continue };
                    let (parent, bt) = match arr.get(4) {
                        Some(C::A(m)) => (u(m.first()).unwrap_or(0), u(m.get(1)).unwrap_or(0) as i64),
                        _ => (0, 0),
                    };
                    let list = pend.remove(&slot).unwrap_or_default();
                    // anything older than this slot that never got its block is dropped
                    let stale: Vec<u64> = pend.range(..slot).map(|(k, _)| *k).collect();
                    for k in stale {
                        tracing::warn!("slot {k}: transactions without a block node, dropped");
                        pend.remove(&k);
                    }
                    let n_tx = list.len() as u32;
                    let mut txs = Vec::with_capacity(list.len());
                    for (i, (idx, d, m)) in list.into_iter().enumerate() {
                        let (Some(data), Some(meta)) = (assemble(d, &frames), assemble(m, &frames)) else {
                            tracing::warn!("slot {slot}: missing data frames");
                            continue;
                        };
                        txs.push(RawTx { idx: idx.unwrap_or(i as u64) as u32, data, meta });
                    }
                    if frames.len() > 50_000 {
                        frames.clear();
                    }
                    if slot >= to_slot {
                        break;
                    }
                    let b = SlotBatch { slot, parent, block_time: bt, n_tx, start_off: slot_start, txs };
                    if btx.send((seq, b)).is_err() {
                        break;
                    }
                    seq += 1;
                    slot_start = rd.consumed;
                }
                _ => {}
            }
        }
        tracing::info!("parser done: nodes {n_nodes}, bytes {}", rd.consumed);
        Ok(rd.consumed)
    });
    let mut workers = Vec::new();
    for _ in 0..a.workers {
        let brx = brx.clone();
        let rtx = rtx.clone();
        workers.push(std::thread::spawn(move || {
            let mut conv = Conv { pk_cache: FxHashMap::default(), zd: zstd::bulk::Decompressor::new().unwrap(), buf: Vec::new() };
            loop {
                let msg = { brx.lock().unwrap().recv() };
                let Ok((seq, mut b)) = msg else { break };
                let mut st = Stats::default();
                let mut out = Vec::with_capacity(b.txs.len() / 2);
                let mut txs = std::mem::take(&mut b.txs);
                txs.sort_by_key(|t| t.idx);
                for t in &txs {
                    if let Some(mut tv) = conv.convert(b.slot, t, &mut st) {
                        tv.block_time = b.block_time;
                        out.push(tv);
                    }
                }
                if rtx.send((seq, b, out, st)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(rtx);

    let cfg = DecodeCfg { decode_amm: a.amm, ..DecodeCfg::default() };
    let mut dst = DecodeState::default();
    let mut writer: Option<(u64, StoreWriter)> = None;
    let mut reorder: BTreeMap<u64, (SlotBatch, Vec<TxView>, Stats)> = BTreeMap::new();
    let mut next_seq = 0u64;
    let t0 = Instant::now();
    let mut last_log = Instant::now();
    let (mut n_blocks, mut n_ev, mut n_trade, mut n_create) = (0u64, 0u64, 0u64, 0u64);
    let mut tot = Stats::default();
    let mut last_slot = 0u64;
    let mut last_off = start_off;
    for (seq, b, txs, st) in rrx {
        reorder.insert(seq, (b, txs, st));
        while let Some((b, txs, st)) = reorder.remove(&next_seq) {
            next_seq += 1;
            tot.txs += st.txs;
            tot.votes += st.votes;
            tot.meta_err += st.meta_err;
            tot.wire_err += st.wire_err;
            tot.alt_tx += st.alt_tx;
            tot.alt_loaded += st.alt_loaded;
            last_off = b.start_off;
            if b.slot < from_slot {
                continue;
            }
            // rotate: close the current segment, checkpoint, open a new one
            let rotate = match &writer {
                None => true,
                Some((s0, _)) => b.slot >= s0 + a.rotate,
            };
            if rotate {
                if let Some((_, mut w)) = writer.take() {
                    w.close()?;
                    fs::write(&resume_path, format!("{} {}\n", b.start_off, b.slot))?;
                }
                writer = Some((b.slot, StoreWriter::with_level(&a.out, 1 << 40, a.level)?));
            }
            let mut evs = Vec::new();
            let mut stat = BlockStat { slot: b.slot, parent_slot: b.parent, block_time: b.block_time, n_tx: b.n_tx, ..Default::default() };
            for tv in &txs {
                match decode_tx(tv, &cfg, &mut dst, &mut evs) {
                    TxClass::PumpOk => stat.pump_ok += 1,
                    TxClass::PumpFailed => stat.pump_failed += 1,
                    TxClass::Other => {}
                }
            }
            for e in evs.iter_mut() {
                match e {
                    Ev::Trade(_) => n_trade += 1,
                    Ev::Create(_) => n_create += 1,
                    // the brain never reads signatures; funding/transfer signatures are ~half the bytes
                    Ev::Fund(f) if a.slim => f.m.sig = Sig::default(),
                    Ev::TokXfer(x) if a.slim => x.m.sig = Sig::default(),
                    _ => {}
                }
            }
            evs.push(Ev::Block(stat));
            n_ev += evs.len() as u64;
            writer.as_mut().unwrap().1.write_block(b.slot, &evs)?;
            n_blocks += 1;
            last_slot = b.slot;
        }
        if last_log.elapsed().as_secs() >= 30 {
            last_log = Instant::now();
            let el = t0.elapsed().as_secs_f64();
            let done = last_off.saturating_sub(start_off) as f64;
            tracing::info!(
                "slot {last_slot} {:.1}% {:.0} MB/s blocks={n_blocks} txs={} votes={} meta_err={} wire_err={} creates={n_create} trades={n_trade} events={n_ev} retries={}",
                100.0 * last_off as f64 / total as f64,
                done / el / 1e6,
                tot.txs,
                tot.votes,
                tot.meta_err,
                tot.wire_err,
                retries.load(Ordering::Relaxed)
            );
        }
    }
    if let Some((_, mut w)) = writer.take() {
        w.close()?;
    }
    let consumed = parser.join().map_err(|_| anyhow::anyhow!("parser panicked"))??;
    for w in workers {
        let _ = w.join();
    }
    let complete = consumed >= total || last_slot + 1 >= a.to_slot;
    fs::write(a.out.join(if complete { "DONE" } else { "PARTIAL" }), format!("{last_slot} {consumed}\n"))?;
    tracing::info!(
        "done: epoch {} blocks={n_blocks} last_slot={last_slot} txs={} votes={} meta_err={} wire_err={} alt_tx={} alt_loaded={} creates={n_create} trades={n_trade} in {:.0}s",
        a.epoch,
        tot.txs,
        tot.votes,
        tot.meta_err,
        tot.wire_err,
        tot.alt_tx,
        tot.alt_loaded,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
