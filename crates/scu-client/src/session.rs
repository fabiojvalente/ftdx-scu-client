//! Live session: the 7-phase handshake plus steady-state channel tasks.
//!
//! The concurrency is executor-agnostic (`futures` only) so the same code runs
//! on the native Tokio runtime and under `wasm_bindgen_futures::spawn_local` in
//! the browser. Only the transport (`crate::transport`) and timers differ.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::stream::{FuturesUnordered, StreamExt};
use scu_protocol::{auth::build_auth, channel, frame::parse_packet, msg, Packet};
use thiserror::Error;

use crate::config::ConnectConfig;
use crate::event::Event;
use crate::transport::{self, BoxFut, Channels, SharedChannel};

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(#[from] scu_protocol::ProtocolError),
    #[error("no authentication response (check host, credentials, and that no other client is connected)")]
    AuthTimeout,
    #[error("bridge URL not configured (required by the browser build)")]
    BridgeUrl,
    #[error("bridge error: {0}")]
    Bridge(String),
}

#[inline]
fn next_seq(counter: &AtomicU8) -> u8 {
    counter.fetch_add(1, Ordering::Relaxed)
}

struct Inner {
    ctrl: SharedChannel,
    cat: SharedChannel,
    audio: SharedChannel,
    scope: SharedChannel,

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
        keepalive(&inner.ctrl, &inner.ctrl_seq, channel::CTRL, &body).await?;
        keepalive(&inner.cat, &inner.cat_seq, channel::CAT, &body).await?;
        keepalive(&inner.audio, &inner.audio_seq, channel::AUDIO, &body).await?;
        keepalive(&inner.scope, &inner.scope_seq, channel::SCOPE, &body).await?;
        Ok(())
    }

    /// Send a CAT command. It is transmitted twice (same internal cmd_seq) for
    /// UDP reliability, per the spec.
    pub async fn send_cat_now(&self, command: &str) -> std::io::Result<()> {
        let inner = &self.0;
        let body = cat_body(inner, command);
        for _ in 0..2 {
            let seq = next_seq(&inner.cat_seq);
            let datagram = scu_protocol::frame_new(seq, channel::CAT, msg::DATA, &body, None);
            inner.cat.send(&datagram).await?;
        }
        Ok(())
    }

    /// Fire-and-forget CAT command.
    ///
    /// Uses non-blocking sends so it works from any thread (e.g. the UI thread)
    /// without needing the session runtime to be alive.
    pub fn send_cat(&self, command: &str) {
        let inner = &self.0;
        let body = cat_body(inner, command);
        for _ in 0..2 {
            let seq = next_seq(&inner.cat_seq);
            let datagram = scu_protocol::frame_new(seq, channel::CAT, msg::DATA, &body, None);
            if let Err(e) = inner.cat.send_now(&datagram) {
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
        if let Err(e) = inner.audio.send_now(&datagram) {
            tracing::debug!(%e, "TX audio send failed");
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.0.event_tx.unbounded_send(event);
    }
}

fn cat_body(inner: &Inner, command: &str) -> Vec<u8> {
    let cmd_seq = inner.cat_cmd_seq.fetch_add(1, Ordering::Relaxed);
    let mut body = Vec::with_capacity(8 + command.len());
    body.extend_from_slice(&[inner.session_id, 0, 0, 0, cmd_seq, 0, 0, 0]);
    body.extend_from_slice(command.as_bytes());
    body
}

async fn keepalive(
    chan: &SharedChannel,
    seq_cell: &AtomicU8,
    logical: u8,
    body: &[u8],
) -> std::io::Result<()> {
    let seq = next_seq(seq_cell);
    let datagram = scu_protocol::frame_new(seq, logical, msg::KEEPALIVE, body, None);
    chan.send(&datagram).await
}

/// A connected SCU-LAN10 session.
pub struct ScuClient {
    handle: ScuHandle,
    events: UnboundedReceiver<Event>,
    runners: FuturesUnordered<BoxFut<'static, ()>>,
}

impl ScuClient {
    /// Perform the full 7-phase handshake and return a live session.
    pub async fn connect(config: ConnectConfig) -> Result<Self, ClientError> {
        let (event_tx, events) = mpsc::unbounded();

        let Channels {
            ctrl,
            cat,
            audio,
            scope,
        } = transport::open(&config).await?;

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
            event_tx: event_tx.clone(),
        });
        let handle = ScuHandle(Arc::clone(&inner));

        // Phase 4: continue to channel setup (sent on CTRL, channel field CAT).
        let continuation = [session_id, 0, 0, 0, 0x51, 0xC3, 0x00, 0x96, 0, 0];
        let seq = next_seq(&inner.ctrl_seq);
        ctrl.send(&scu_protocol::frame_new(
            seq,
            channel::CAT,
            msg::SETUP,
            &continuation,
            None,
        ))
        .await?;
        transport::sleep(Duration::from_millis(180)).await;

        // Phase 5: channel setup.
        let init_body = [session_id, 0, 0, 0];
        let cat_setup = [session_id, 0, 0, 0, 0x52, 0xC3];
        let audio_setup = [session_id, 0, 0, 0, 0x53, 0xC3];

        let seq = next_seq(&inner.cat_seq);
        cat.send(&scu_protocol::frame_new(
            seq,
            channel::CAT,
            msg::PORT_INIT,
            &init_body,
            None,
        ))
        .await?;
        transport::sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.ctrl_seq);
        ctrl.send(&scu_protocol::frame_new(
            seq,
            channel::AUDIO,
            msg::SETUP,
            &cat_setup,
            None,
        ))
        .await?;
        transport::sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.audio_seq);
        audio
            .send(&scu_protocol::frame_new(
                seq,
                channel::AUDIO,
                msg::PORT_INIT,
                &init_body,
                None,
            ))
            .await?;
        transport::sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.ctrl_seq);
        ctrl.send(&scu_protocol::frame_new(
            seq,
            channel::SCOPE,
            msg::SETUP,
            &audio_setup,
            None,
        ))
        .await?;
        transport::sleep(Duration::from_millis(180)).await;

        let seq = next_seq(&inner.scope_seq);
        scope
            .send(&scu_protocol::frame_new(
                seq,
                channel::SCOPE,
                msg::PORT_INIT,
                &init_body,
                None,
            ))
            .await?;
        transport::sleep(Duration::from_millis(180)).await;

        // Spawn receive loops plus the keepalive ticker.
        let runners: FuturesUnordered<BoxFut<'static, ()>> = FuturesUnordered::new();
        runners.push(recv_runner(
            Arc::clone(&ctrl),
            event_tx.clone(),
            "ctrl",
            no_event,
        ));
        runners.push(recv_runner(
            Arc::clone(&cat),
            event_tx.clone(),
            "cat",
            cat_event,
        ));
        runners.push(recv_runner(
            Arc::clone(&audio),
            event_tx.clone(),
            "audio",
            audio_event,
        ));
        runners.push(recv_runner(
            Arc::clone(&scope),
            event_tx.clone(),
            "scope",
            scope_event,
        ));
        runners.push(keepalive_runner(handle.clone(), event_tx.clone()));

        // Phase 6: initial keepalives on all channels.
        handle.send_keepalives().await?;

        // Phase 7: liveness check.
        handle.send_cat_now(&scu_cat::read_frequency()).await?;

        handle.emit(Event::Connected { session_id });

        Ok(Self {
            handle,
            events,
            runners,
        })
    }

    pub fn handle(&self) -> ScuHandle {
        self.handle.clone()
    }

    /// Non-blocking poll for the next queued event.
    pub fn try_recv(&mut self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Await the next event, driving the session's receive loops.
    pub async fn recv(&mut self) -> Option<Event> {
        loop {
            if let Ok(event) = self.events.try_recv() {
                return Some(event);
            }
            if self.runners.is_empty() {
                return None;
            }
            let events = &mut self.events;
            let runners = &mut self.runners;
            futures::select! {
                maybe = events.next() => return maybe,
                _ = runners.select_next_some() => {}
            }
        }
    }
}

fn recv_runner(
    chan: SharedChannel,
    tx: UnboundedSender<Event>,
    label: &'static str,
    on_packet: fn(&Packet) -> Option<Event>,
) -> BoxFut<'static, ()> {
    Box::pin(async move {
        let mut buf = vec![0u8; 65535];
        loop {
            match chan.recv(&mut buf).await {
                Ok(n) => match parse_packet(&buf[..n]) {
                    Ok(pkt) => {
                        if let Some(event) = on_packet(&pkt) {
                            if tx.unbounded_send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => tracing::debug!(channel = label, %e, "dropping malformed packet"),
                },
                Err(e) => {
                    let _ = tx.unbounded_send(Event::Disconnected(format!("{label}: {e}")));
                    break;
                }
            }
        }
    })
}

fn keepalive_runner(handle: ScuHandle, tx: UnboundedSender<Event>) -> BoxFut<'static, ()> {
    Box::pin(async move {
        loop {
            transport::sleep(Duration::from_secs(1)).await;
            if handle.send_keepalives().await.is_err() {
                let _ = tx.unbounded_send(Event::Disconnected("keepalive send failed".into()));
                break;
            }
        }
    })
}

fn no_event(_pkt: &Packet) -> Option<Event> {
    None
}

fn cat_event(pkt: &Packet) -> Option<Event> {
    if pkt.msg_type() != msg::DATA_RESP || pkt.body.len() < 4 {
        return None;
    }
    let text = String::from_utf8_lossy(&pkt.body[4..]).into_owned();
    if let Some(id) = scu_cat::parse_id(&text) {
        return Some(Event::Radio(scu_cat::RadioModel::from_id(id)));
    }
    Some(Event::Cat(text))
}

fn audio_event(pkt: &Packet) -> Option<Event> {
    if pkt.msg_type() != msg::DATA_RESP {
        return None;
    }
    scu_audio::decode(&pkt.body)
        .ok()
        .map(|f| Event::Audio(Box::new(f)))
}

fn scope_event(pkt: &Packet) -> Option<Event> {
    if pkt.msg_type() != msg::DATA_RESP {
        return None;
    }
    Some(Event::Scope(pkt.body.clone()))
}

async fn await_auth(ctrl: &SharedChannel) -> Result<u8, ClientError> {
    let mut buf = [0u8; 4096];
    let attempt = async {
        loop {
            let n = ctrl.recv(&mut buf).await?;
            if let Ok(pkt) = parse_packet(&buf[..n]) {
                if pkt.msg_type() == msg::AUTH_RESP && !pkt.body.is_empty() {
                    return Ok::<u8, ClientError>(pkt.body[0]);
                }
            }
        }
    };
    transport::timeout(Duration::from_secs(5), attempt)
        .await
        .unwrap_or(Err(ClientError::AuthTimeout))
}
