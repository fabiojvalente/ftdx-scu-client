//! `scu-bridge` — WebSocket ⇄ UDP relay for the browser client.
//!
//! A browser cannot open the four raw UDP sockets the SCU-LAN10 protocol needs,
//! so this small native process relays them. The browser opens one WebSocket and
//! sends a JSON hello naming the radio (`host`, `base_port`); the bridge binds
//! four UDP sockets to `base_port .. base_port + 3` and then shuttles opaque
//! frames both ways. Every binary frame is tagged with its physical channel:
//!
//! ```text
//! 0 = ctrl (base_port + 0)   2 = audio (base_port + 2)
//! 1 = cat  (base_port + 1)   3 = scope (base_port + 3)
//! ```
//!
//! The bridge does not understand the protocol, credentials, or handshake — it
//! is a dumb byte forwarder.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio_tungstenite::tungstenite::Message;

const CHANNELS: usize = 4;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let listen = parse_args()?;
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    tracing::info!(%listen, "scu-bridge listening");

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                tracing::warn!(%error, "accept failed");
                continue;
            }
        };
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, peer).await {
                tracing::warn!(%peer, %error, "connection ended");
            }
        });
    }
}

fn parse_args() -> Result<SocketAddr> {
    let mut args = std::env::args().skip(1);
    let mut listen: SocketAddr = "127.0.0.1:9000".parse().unwrap();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" | "-l" => {
                listen = args
                    .next()
                    .ok_or_else(|| anyhow!("--listen needs a value"))?
                    .parse()
                    .context("--listen must be an address, e.g. 0.0.0.0:9000")?;
            }
            "--help" | "-h" => {
                println!(
                    "scu-bridge — WebSocket ⇄ UDP relay for the browser client

USAGE:
    scu-bridge [--listen <ADDR>]

OPTIONS:
    -l, --listen <ADDR>   Address to serve WebSocket clients on (default 127.0.0.1:9000)
    -h, --help            Show this help"
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown argument: {other}")),
        }
    }
    Ok(listen)
}

async fn handle_connection(stream: tokio::net::TcpStream, peer: SocketAddr) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .context("WebSocket handshake")?;
    let (mut sink, mut inbound) = ws.split();

    // First message must be the JSON hello naming the radio.
    let hello = loop {
        match inbound.next().await {
            Some(Ok(Message::Text(text))) => break text,
            Some(Ok(Message::Binary(_))) => return Err(anyhow!("expected JSON hello first")),
            Some(Ok(_)) => continue,
            Some(Err(error)) => return Err(error.into()),
            None => return Err(anyhow!("closed before hello")),
        }
    };
    let (host, base_port) = parse_hello(hello.as_str())?;
    tracing::info!(%peer, %host, base_port, "bridging");

    let mut sockets: Vec<Arc<UdpSocket>> = Vec::with_capacity(CHANNELS);
    for offset in 0..CHANNELS as u16 {
        let socket = UdpSocket::bind(("0.0.0.0", 0))
            .await
            .context("binding local UDP socket")?;
        socket
            .connect((host.as_str(), base_port + offset))
            .await
            .with_context(|| format!("connecting UDP to {host}:{}", base_port + offset))?;
        sockets.push(Arc::new(socket));
    }

    // One writer funnels every UDP reader into the WebSocket sink.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(256);
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });

    let mut readers = Vec::with_capacity(CHANNELS);
    for (tag, socket) in sockets.iter().cloned().enumerate() {
        let tx = tx.clone();
        readers.push(tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                match socket.recv(&mut buf).await {
                    Ok(n) => {
                        let mut frame = Vec::with_capacity(n + 1);
                        frame.push(tag as u8);
                        frame.extend_from_slice(&buf[..n]);
                        if tx.send(Message::binary(frame)).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(tag, %error, "UDP recv failed");
                        break;
                    }
                }
            }
        }));
    }
    drop(tx);

    // WebSocket → UDP.
    while let Some(message) = inbound.next().await {
        match message {
            Ok(Message::Binary(bytes)) => {
                if let Some((&tag, payload)) = bytes.split_first() {
                    if let Some(socket) = sockets.get(tag as usize) {
                        let _ = socket.send(payload).await;
                    }
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    for reader in readers {
        reader.abort();
    }
    writer.abort();
    tracing::info!(%peer, "bridge closed");
    Ok(())
}

fn parse_hello(text: &str) -> Result<(String, u16)> {
    let value: serde_json::Value = serde_json::from_str(text).context("hello must be JSON")?;
    let host = value
        .get("host")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("hello missing \"host\""))?
        .to_string();
    let base_port = value
        .get("base_port")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("hello missing \"base_port\""))? as u16;
    Ok((host, base_port))
}
