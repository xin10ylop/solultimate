//! Transaction senders: Nozomi (Temporal) and plain RPC, fanned out concurrently.

use anyhow::{anyhow, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointCfg {
    pub name: String,
    /// Full URL incl. API key, e.g. "http://fra2.nozomi.temporal.xyz/?c=YOUR_KEY"
    pub url: String,
    /// Optional keep-alive URL (GET) to keep the TCP/TLS connection warm.
    #[serde(default)]
    pub ping_url: Option<String>,
}

#[derive(Clone)]
pub struct Sender {
    eps: Vec<(EndpointCfg, reqwest::Client)>,
}

impl Sender {
    pub fn new(eps: &[EndpointCfg]) -> Result<Sender> {
        let mut v = Vec::new();
        for e in eps {
            let c = reqwest::Client::builder()
                .http1_only()
                .tcp_nodelay(true)
                .pool_idle_timeout(Duration::from_secs(300))
                .timeout(Duration::from_secs(5))
                .build()?;
            v.push((e.clone(), c));
        }
        if v.is_empty() {
            return Err(anyhow!("no send endpoints configured"));
        }
        Ok(Sender { eps: v })
    }

    /// Send to every endpoint concurrently; returns (endpoint, elapsed, result) per endpoint.
    pub async fn send_all(&self, tx: &[u8]) -> Vec<(String, Duration, Result<String>)> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(tx);
        let body = json!({"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[b64, {"encoding":"base64","skipPreflight":true,"maxRetries":0}]});
        let futs = self.eps.iter().map(|(e, c)| {
            let body = body.clone();
            async move {
                let t0 = Instant::now();
                let r = async {
                    let v: serde_json::Value = c.post(&e.url).json(&body).send().await?.json().await?;
                    if let Some(err) = v.get("error") {
                        return Err(anyhow!("{err}"));
                    }
                    Ok(v["result"].as_str().unwrap_or("").to_string())
                }
                .await;
                (e.name.clone(), t0.elapsed(), r)
            }
        });
        futures::future::join_all(futs).await
    }

    pub fn spawn_keepalive(&self) {
        for (e, c) in &self.eps {
            if let Some(p) = e.ping_url.clone() {
                let c = c.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = c.get(&p).send().await;
                        tokio::time::sleep(Duration::from_secs(25)).await;
                    }
                });
            }
        }
    }
}
