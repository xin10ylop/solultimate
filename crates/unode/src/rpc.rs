//! Small JSON-RPC client (HTTP/1 only: some proxies mishandle multiplexed HTTP/2).

use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;
use ucore::pk::Pk;

#[derive(Clone)]
pub struct Rpc {
    pub url: String,
    pub http: reqwest::Client,
}

impl Rpc {
    pub fn new(url: &str) -> Result<Rpc> {
        let http = reqwest::Client::builder()
            .http1_only()
            .gzip(true)
            .timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(32)
            .tcp_nodelay(true)
            .build()?;
        Ok(Rpc { url: url.to_string(), http })
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let mut wait = 400u64;
        for attempt in 0..6 {
            let v: Value = self.http.post(&self.url).json(&body).send().await?.json().await?;
            if let Some(e) = v.get("error") {
                if e["code"].as_i64() == Some(429) && attempt < 5 {
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                    wait *= 2;
                    continue;
                }
                return Err(anyhow!("{method}: {e}"));
            }
            return Ok(v["result"].clone());
        }
        Err(anyhow!("{method}: rate limited"))
    }

    pub async fn latest_blockhash(&self) -> Result<([u8; 32], u64)> {
        let r = self.call("getLatestBlockhash", json!([{"commitment":"processed"}])).await?;
        let h = r["value"]["blockhash"].as_str().ok_or_else(|| anyhow!("no blockhash"))?;
        let b = bs58::decode(h).into_vec()?;
        let mut a = [0u8; 32];
        a.copy_from_slice(&b);
        Ok((a, r["value"]["lastValidBlockHeight"].as_u64().unwrap_or(0)))
    }

    pub async fn balance(&self, pk: &Pk) -> Result<u64> {
        let r = self.call("getBalance", json!([pk.to_b58(), {"commitment":"processed"}])).await?;
        r["value"].as_u64().ok_or_else(|| anyhow!("bad balance"))
    }

    pub async fn account_data(&self, pk: &Pk) -> Result<Option<(Vec<u8>, Pk, u64)>> {
        let r = self.call("getAccountInfo", json!([pk.to_b58(), {"encoding":"base64","commitment":"processed"}])).await?;
        if r["value"].is_null() {
            return Ok(None);
        }
        let d = r["value"]["data"][0].as_str().unwrap_or("");
        let data = base64::engine::general_purpose::STANDARD.decode(d)?;
        let owner = Pk::from_b58(r["value"]["owner"].as_str().unwrap_or("11111111111111111111111111111111"))?;
        Ok(Some((data, owner, r["value"]["lamports"].as_u64().unwrap_or(0))))
    }

    pub async fn simulate(&self, tx: &[u8]) -> Result<Value> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tx);
        self.call(
            "simulateTransaction",
            json!([b64, {"encoding":"base64","sigVerify":false,"replaceRecentBlockhash":true,"commitment":"processed"}]),
        )
        .await
    }

    pub async fn send(&self, tx: &[u8]) -> Result<String> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tx);
        let r = self
            .call("sendTransaction", json!([b64, {"encoding":"base64","skipPreflight":true,"maxRetries":0}]))
            .await?;
        Ok(r.as_str().unwrap_or("").to_string())
    }

    /// (slot, err) for each signature; None = not found yet.
    pub async fn signature_statuses(&self, sigs: &[String]) -> Result<Vec<Option<(u64, Option<Value>)>>> {
        let r = self.call("getSignatureStatuses", json!([sigs, {"searchTransactionHistory": false}])).await?;
        let mut out = Vec::new();
        for v in r["value"].as_array().cloned().unwrap_or_default() {
            if v.is_null() {
                out.push(None);
            } else {
                let err = if v["err"].is_null() { None } else { Some(v["err"].clone()) };
                out.push(Some((v["slot"].as_u64().unwrap_or(0), err)));
            }
        }
        Ok(out)
    }

    pub async fn largest_token_holder(&self, mint: &Pk) -> Result<Option<(Pk, u64)>> {
        let r = self.call("getTokenLargestAccounts", json!([mint.to_b58(), {"commitment":"processed"}])).await?;
        for v in r["value"].as_array().cloned().unwrap_or_default() {
            let acct = Pk::from_b58(v["address"].as_str().unwrap_or(""))?;
            let amt: u64 = v["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
            // token account owner = bytes 32..64
            if let Some((data, _, _)) = self.account_data(&acct).await? {
                if data.len() >= 64 {
                    let owner = Pk::from_slice(&data[32..64]).unwrap();
                    if owner.is_on_curve() && amt > 0 {
                        return Ok(Some((owner, amt)));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Decode the pump BondingCurve account prefix.
pub fn decode_bonding_curve(data: &[u8]) -> Option<ucore::curve::Curve> {
    let mut r = ucore::pump::Rd::new(data.get(8..)?);
    let vtok = r.u64()?;
    let vsol = r.u64()?;
    let rtok = r.u64()?;
    let rsol = r.u64()?;
    Some(ucore::curve::Curve { vsol, vtok, rsol, rtok })
}
