//! Live session: the 7-phase handshake plus steady-state channel tasks.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use scu_protocol::{auth::build_auth, channel, frame::parse_packet, msg, Packet};
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};

use crate::config::ConnectConfig;
use crate::event::Event;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(#[from] scu_protocol::ProtocolError),
    #[error("no authentication response (check host, credentials, and that no other client is connected)")]
    AuthTimeout,
}

#[inline]
fn next_seq(counter: &AtomicU8) -> u8 {
    counter.fetch_add(1, Ordering::Relaxed)
}

async fn bind_connect(host: &str, port: u16) -> std::io::Result<Arc<UdpSocket>> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).await?;
    sock.connect((host, port)).await?;
    Ok(Arc::new(sock))
}

async fn send_packet(
    sock: &UdpSocket,
    seq: u8,
    channel: u8,
    msg_type: u8,
    body: &[u8],
) -> std::io::Result<()> {
    let datagram = scu_protocol::frame_new(seq, channel, msg_type, body, None);
    sock.send(&datagram).await.map(|_| ())
}

struct Inner {
    ctrl: Arc<UdpSocket>,
    cat: Arc<UdpSocket>,
    audio: Arc<UdpSocket>,
    scope: Arc<UdpSocket>,

    session_id: u8,

    ctrl_seq: AtomicU8,
    cat_seq: AtomicU8,
    audio_seq: AtomicU8,
    scope_seq: AtomicU8,
    cat_cmd_seq: AtomicU8,

    event_tx: UnboundedSender<Event>,
}

/// A cloneable handle for issuing commands on a live session.
#[derive(Clone)]
pub struct ScuHandle(Arc<Inner>);

impl ScuHandle {
    pub fn session_id(&self) -> u8 {
        self.0.session_id
    }

    /// Send a keepalive on all four channels.
    pub async fn send_keepalives(&self) -> std::io::Result<()> {
        let inner = &self.0;
        let body = [0u8; 4];
        let seq = next_seq(&inner.ctrl_seq);
        send_packet(&inner.ctrl, seq, channel::CTRL, msg::KEEPALIVE, &body).await?;
        let seq = next_seq(&inner.cat_seq);
        send_packet(&inner.cat, seq, channel::CAT, msg::KEEPALIVE, &body).await?;
        let seq = next_seq(&inner.audio_seq);
        send_packet(&inner.audio, seq, channel::AUDIO, msg::KEEPALIVE, &body).await?;
        let seq = next_seq(&inner.scope_seq);
        send_packet(&inner.scope, seq, channel::SCOPE, msg::KEEPALIVE, &body).await?;
        Ok(())
    }

    /// Send a CAT command. It is transmitted twice (same internal cmd_seq) for
    /// UDP reliability, per the spec.
    pub async fn send_cat_now(&self, command: &str) -> std::io::Result<()> {
        let inner = &self.0;
        let cmd_seq = inner.cat_cmd_seq.fetch_add(1, Ordering::Relaxed);
        let mut body = Vec::with_capacity(8 + command.len());
        body.extend_from_slice(&[inner.session_id, 0, 0, 0, cmd_seq, 0, 0, 0]);
        body.extend_from_slice(command.as_bytes());

        for _ in 0..2 {
            let seq = next_seq(&inner.cat_seq);
            send_packet(&inner.cat, seq, channel::CAT, msg::DATA, &body).await?;
        }
        Ok(())
    }

    /// Fire-and-forget CAT command.
    ///
    /// Uses non-blocking sends so it works from any thread (e.g. the UI thread)
    /// without needing the session runtime to be alive.
    pub fn send_cat(&self, command: &str) {
        let inner = &self.0;
        let cmd_seq = inner.cat_cmd_seq.fetch_add(1, Ordering::Relaxed);
        let mut body = Vec::with_capacity(8 + command.len());
        body.extend_from_slice(&[inner.session_id, 0, 0, 0, cmd_seq, 0, 0, 0]);
        body.extend_from_slice(command.as_bytes());

        for _ in 0..2 {
            let seq = next_seq(&inner.cat_seq);
            let datagram = scu_protocol::frame_new(seq, channel::CAT, msg::DATA, &body, None);
            if let Err(e) = inner.cat.try_send(&datagram) {
                tracing::warn!(%e, "CAT send failed");
            }
        }
    }

    /// Key or unkey the transmitter via the CAT `TX` command.
    pub fn set_transmit(&self, on: bool) {
        self.send_cat(&scu_cat::transmit(on));
    }

    /// Send an encoded TX audio body (the 664-byte RX-shaped body from
    /// [`scu_audio::encode_tx`]) on the AUDIO channel.
    ///
    /// The `[session_id, 0, 0, 0]` prefix that every client -> server payload
    /// carries is added here. Fire-and-forget so it is safe to call from the
    /// real-time capture thread.
    pub fn send_tx_audio(&self, audio: &[u8]) {
        let inner = &self.0;
        let mut body = Vec::with_capacity(scu_protocol::AUDIO_TX_PREFIX_LEN + audio.len());
        body.extend_from_slice(&[inner.session_id, 0, 0, 0]);
        body.extend_from_slice(audio);

        let seq = next_seq(&inner.audio_seq);
        let datagram = scu_protocol::frame_new(seq, channel::AUDIO, msg::DATA, &body, None);
        if let Err(e) = inner.audio.try_send(&datagram) {
            tracing::debug!(%e, "TX audio send failed");
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.0.event_tx.send(event);
    }
}

/// A connected SCU-LAN10 session.
pub struct ScuClient {
    handle: ScuHandle,
    events: UnboundedReceiver<Event>,
    tasks: Vec<JoinHandle<()>>,
}

impl ScuClient {
    /// Perform the full 7-phase handshake and return a live session.
    pub async fn connect(config: ConnectConfig) -> Result<Self, ClientError> {
        let (event_tx, events) = mpsc::unbounded_channel();

        let ctrl = bind_connect(&config.host, config.ctrl_port()).await?;
        let cat = bind_connect(&config.host, config.cat_port()).await?;
        let audio = bind_connect(&config.host, config.audio_port()).await?;
        let scope = bind_connect(&config.host, config.scope_port()).await?;

        // Phase 1: five hardcoded init keepalives.
        for packet in scu_protocol::init::init_packets() {
            ctrl.send(&packet).await?;
        }

        // Phase 2: authentication.
        ctrl.send(&build_auth(&config.username, &config.password))
            .await?;

        // Phase 3: auth response -> session id.
        let session_id = await_auth(&ctrl).await?;
        tracing::info!(session_id, "authenticated");

        let inner = Arc::new(Inner {
            ctrl: Arc::clone(&ctrl),
            cat: Arc::clone(&cat),
            audio: Arc::clone(&audio),
            scope: Arc::clone(&scope),
            session_id,
            ctrl_seq: AtomicU8::new(7),
            cat_seq: AtomicU8::new(1),
            audio_seq: AtomicU8::new(1),
            scope_seq: AtomicU8::new(1),
            cat_cmd_seq: AtomicU8::new(1),
            event_tx,
        });
        let handle = ScuHandle(Arc::clone(&inner));

        // Phase 4: continue to channel setup (sent on CTRL, channel field CAT).
        let continuation = [session_id, 0, 0, 0, 0x51, 0xC3, 0x00, 0x96, 0, 0];
        let seq = next_seq(&inner.ctrl_seq);
        send_packet(&ctrl, seq, channel::CAT, msg::SETUP, &continuation).await?;
        sleep(Duration::from_millis(180)).await;

        // Phase 5: channel setup.
        let init_body = [session_id, 0, 0, 0];
        let cat_setup = [session_id, 0, 0, 0, 0x52, 0xC3];
        let audio_setup = [session_id, 0, 0, 0, 0x53, 0xC3];

        let seq = next_seq(&inner.cat_seq);
        send_packet(&cat, seq, channel::CAT, msg::PORT_INIT, &init_body).await?;
        sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.ctrl_seq);
        send_packet(&ctrl, seq, channel::AUDIO, msg::SETUP, &cat_setup).await?;
        sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.audio_seq);
        send_packet(&audio, seq, channel::AUDIO, msg::PORT_INIT, &init_body).await?;
        sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.ctrl_seq);
        send_packet(&ctrl, seq, channel::SCOPE, msg::SETUP, &audio_setup).await?;
        sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.scope_seq);
        send_packet(&scope, seq, channel::SCOPE, msg::PORT_INIT, &init_body).await?;
        sleep(Duration::from_millis(180)).await;

        // Spawn receive loops.
        let mut tasks = Vec::new();
        tasks.push(spawn_recv(
            Arc::clone(&ctrl),
            inner.event_tx.clone(),
            "ctrl",
            |_pkt| None,
        ));
        tasks.push(spawn_recv(
            Arc::clone(&inner.cat),
            inner.event_tx.clone(),
            "cat",
            |pkt| {
                if pkt.msg_type() != msg::DATA_RESP || pkt.body.len() < 4 {
                    return None;
                }
                let text = String::from_utf8_lossy(&pkt.body[4..]).into_owned();
                if let Some(id) = scu_cat::parse_id(&text) {
                    return Some(Event::Radio(scu_cat::RadioModel::from_id(id)));
                }
                Some(Event::Cat(text))
            },
        ));
        tasks.push(spawn_recv(
            Arc::clone(&inner.audio),
            inner.event_tx.clone(),
            "audio",
            |pkt| {
                if pkt.msg_type() != msg::DATA_RESP {
                    return None;
                }
                scu_audio::decode(&pkt.body)
                    .ok()
                    .map(|f| Event::Audio(Box::new(f)))
            },
        ));
        tasks.push(spawn_recv(
            Arc::clone(&inner.scope),
            inner.event_tx.clone(),
            "scope",
            |pkt| {
                if pkt.msg_type() != msg::DATA_RESP {
                    return None;
                }
                Some(Event::Scope(pkt.body.clone()))
            },
        ));

        // Phase 6: initial keepalives on all channels.
        handle.send_keepalives().await?;

        // Steady state: 1 Hz keepalives.
        {
            let ka_handle = handle.clone();
            tasks.push(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(1));
                ticker.tick().await; // consume immediate tick
                loop {
                    ticker.tick().await;
                    if ka_handle.send_keepalives().await.is_err() {
                        ka_handle.emit(Event::Disconnected("keepalive send failed".into()));
                        break;
                    }
                }
            }));
        }

        // Phase 7: liveness check.
        handle.send_cat_now(&scu_cat::read_frequency()).await?;

        handle.emit(Event::Connected { session_id });

        Ok(Self {
            handle,
            events,
            tasks,
        })
    }

    pub fn handle(&self) -> ScuHandle {
        self.handle.clone()
    }

    /// Non-blocking poll for the next event.
    pub fn try_recv(&mut self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Await the next event.
    pub async fn recv(&mut self) -> Option<Event> {
        self.events.recv().await
    }
}

impl Drop for ScuClient {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn await_auth(ctrl: &UdpSocket) -> Result<u8, ClientError> {
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ClientError::AuthTimeout);
        }
        match timeout(remaining, ctrl.recv(&mut buf)).await {
            Err(_) => return Err(ClientError::AuthTimeout),
            Ok(Err(e)) => return Err(ClientError::Io(e)),
            Ok(Ok(n)) => {
                if let Ok(pkt) = parse_packet(&buf[..n]) {
                    if pkt.msg_type() == msg::AUTH_RESP && !pkt.body.is_empty() {
                        return Ok(pkt.body[0]);
                    }
                }
            }
        }
    }
}

fn spawn_recv<F>(
    sock: Arc<UdpSocket>,
    tx: UnboundedSender<Event>,
    label: &'static str,
    on_packet: F,
) -> JoinHandle<()>
where
    F: Fn(&Packet) -> Option<Event> + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        loop {
            match sock.recv(&mut buf).await {
                Ok(n) => match parse_packet(&buf[..n]) {
                    Ok(pkt) => {
                        if let Some(event) = on_packet(&pkt) {
                            let _ = tx.send(event);
                        }
                    }
                    Err(e) => tracing::debug!(channel = label, %e, "dropping malformed packet"),
                },
                Err(e) => {
                    let _ = tx.send(Event::Disconnected(format!("{label}: {e}")));
                    break;
                }
            }
        }
    })
}
