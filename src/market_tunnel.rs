use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;
use russh::client;
use russh::keys::key::PublicKey;
use russh::{Channel, Disconnect};
use serde::Deserialize;
use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::config::Config;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketLeaseResponse {
    connection_id: String,
    ssh_username: String,
    ssh_password: String,
    ssh_addr: String,
    tunnel_url: String,
    subdomain: String,
    #[serde(default)]
    ssh_host_fingerprint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RouterErrorEnvelope {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<RouterErrorBody>,
}

#[derive(Debug, Deserialize)]
struct RouterErrorBody {
    message: String,
}

pub fn spawn(config: Config) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if !config.market_tunnel_enabled {
            tracing::info!("market tunnel disabled");
            return;
        }

        let local_addr = market_local_forward_addr(&config.market_http_addr);
        let client = reqwest::Client::new();
        let mut delay = Duration::from_secs(1);

        loop {
            match run_once(&client, &config, &local_addr).await {
                Ok(()) => {
                    delay = Duration::from_secs(1);
                    tracing::warn!("market tunnel ended; reconnecting");
                }
                Err(err) => {
                    tracing::warn!(error = %err, retry_in_secs = delay.as_secs(), "market tunnel failed");
                }
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(60));
        }
    })
}

async fn run_once(
    client: &reqwest::Client,
    config: &Config,
    local_addr: &str,
) -> anyhow::Result<()> {
    let lease = issue_market_lease(client, config).await?;
    tracing::info!(
        subdomain = %lease.subdomain,
        tunnel_url = %lease.tunnel_url,
        local_addr = %local_addr,
        "issued market tunnel lease"
    );
    connect_and_forward(&lease, local_addr).await
}

async fn issue_market_lease(
    client: &reqwest::Client,
    config: &Config,
) -> anyhow::Result<MarketLeaseResponse> {
    let access_token = crate::router_account::access_token(config).await?;
    let url = format!(
        "{}/v1/markets/tunnel/lease",
        config.router_api_base_url.trim_end_matches('/')
    );
    let response = client
        .post(&url)
        .bearer_auth(access_token)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if response.status().is_success() {
        return Ok(response.json().await?);
    }

    let status = response.status();
    let body = response
        .json::<RouterErrorEnvelope>()
        .await
        .map(|body| {
            body.message
                .or_else(|| body.error.map(|error| error.message))
                .unwrap_or_else(|| status.to_string())
        })
        .unwrap_or_else(|_| status.to_string());
    anyhow::bail!("router market lease request failed: {body}");
}

async fn connect_and_forward(lease: &MarketLeaseResponse, local_addr: &str) -> anyhow::Result<()> {
    let ssh_config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    });
    let (fwd_tx, fwd_rx) = mpsc::unbounded_channel();
    let handler = MarketTunnelHandler {
        fwd_tx,
        expected_fingerprint: lease.ssh_host_fingerprint.clone(),
        ssh_addr: lease.ssh_addr.clone(),
    };
    let mut handle = client::connect(ssh_config, &lease.ssh_addr, handler).await?;
    let auth_ok = handle
        .authenticate_password(&lease.ssh_username, &lease.ssh_password)
        .await?;
    if !auth_ok {
        anyhow::bail!("router ssh authentication failed");
    }

    let remote_port = request_forward(&mut handle).await?;
    tracing::info!(
        connection_id = %lease.connection_id,
        remote_port,
        "market ssh reverse tunnel connected"
    );
    let result = accept_loop(fwd_rx, local_addr).await;
    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
    result
}

async fn request_forward(handle: &mut client::Handle<MarketTunnelHandler>) -> anyhow::Result<u16> {
    for _ in 0..10 {
        let port: u16 = rand::thread_rng().gen_range(20000..30000);
        match handle.tcpip_forward("0.0.0.0", port as u32).await {
            Ok(bound_port) => {
                return Ok(if bound_port == 0 {
                    port
                } else {
                    bound_port as u16
                });
            }
            Err(err) => {
                tracing::debug!(port, error = %err, "remote forward port rejected");
            }
        }
    }
    anyhow::bail!("all remote forward port attempts failed")
}

async fn accept_loop(
    mut fwd_rx: mpsc::UnboundedReceiver<Channel<client::Msg>>,
    local_addr: &str,
) -> anyhow::Result<()> {
    while let Some(channel) = fwd_rx.recv().await {
        let local_addr = local_addr.to_string();
        tokio::spawn(async move {
            let stream = channel.into_stream();
            if let Err(err) = forward_tcp(stream, &local_addr).await {
                tracing::debug!(error = %err, "market tunnel tcp forward failed");
            }
        });
    }
    Ok(())
}

async fn forward_tcp<S>(mut remote: S, local_addr: &str) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut local = TcpStream::connect(local_addr).await?;
    io::copy_bidirectional(&mut remote, &mut local).await?;
    Ok(())
}

fn market_local_forward_addr(listen_addr: &str) -> String {
    match listen_addr.parse::<std::net::SocketAddr>() {
        Ok(addr) => {
            let port = addr.port();
            if addr.ip().is_unspecified() {
                format!("127.0.0.1:{port}")
            } else {
                format!("{}:{port}", addr.ip())
            }
        }
        Err(_) => listen_addr.to_string(),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

struct MarketTunnelHandler {
    fwd_tx: mpsc::UnboundedSender<Channel<client::Msg>>,
    expected_fingerprint: Option<String>,
    ssh_addr: String,
}

#[async_trait]
impl client::Handler for MarketTunnelHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let actual = format!("SHA256:{}", server_public_key.fingerprint());
        match &self.expected_fingerprint {
            Some(expected) if constant_time_eq(expected.as_bytes(), actual.as_bytes()) => {
                tracing::info!(ssh_addr = %self.ssh_addr, fingerprint = %actual, "router ssh host key verified");
                Ok(true)
            }
            Some(expected) => {
                tracing::error!(
                    ssh_addr = %self.ssh_addr,
                    expected = %expected,
                    actual = %actual,
                    "router ssh host key mismatch"
                );
                Ok(false)
            }
            None => {
                tracing::warn!(
                    ssh_addr = %self.ssh_addr,
                    actual = %actual,
                    "router did not return ssh host fingerprint; accepting key"
                );
                Ok(true)
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.fwd_tx.send(channel);
        Ok(())
    }
}
