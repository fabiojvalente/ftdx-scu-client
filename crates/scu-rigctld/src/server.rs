//! Native rigctld-protocol TCP server.
//!
//! Speaks the text protocol used by Hamlib's `NET rigctl` backend (`f`, `F`,
//! `m`, `M`, `t`, `T`, … plus `\chk_vfo` and `\dump_state`) so WSJT-X, fldigi,
//! N1MM, Log4OM and friends can share the single SCU-LAN10 session. Getters are
//! served from the cached [`RadioState`]; setters are forwarded to the radio and
//! update the cache immediately.
//!
//! Modelled on the proven Yaesu Web Control `RigctldServer`, including its TX
//! safety watchdog: a client that keys the radio and disappears cannot leave it
//! transmitting.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use scu_client::ScuHandle;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::hamlib::{mode_from_hamlib, mode_to_hamlib};
use crate::link::RadioLink;
use crate::state::SharedState;

/// Default rigctld TCP port.
pub const DEFAULT_PORT: u16 = 4532;

/// How long a rigctld-initiated key-up may be held without a release.
const TX_SAFETY_TIMEOUT: Duration = Duration::from_secs(180);

/// Delay before re-asserting the frequency after a mode change, letting the
/// radio finish applying the mode's carrier offset (see [`set_mode`]).
const MODE_SETTLE_DELAY: Duration = Duration::from_millis(150);

#[derive(Debug, Error)]
pub enum RigctldError {
    #[error("failed to bind rigctld listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("failed to start rigctld thread: {0}")]
    Thread(#[source] std::io::Error),
}

/// Live server counters, for display in the UI.
#[derive(Debug, Clone, Default)]
pub struct RigctldStatus {
    pub port: u16,
    pub clients: usize,
    pub running: bool,
}

struct Watchdog {
    start: Instant,
    inner: Mutex<WatchdogInner>,
}

#[derive(Default)]
struct WatchdogInner {
    deadline_ms: Option<u64>,
    client: Option<String>,
}

impl Watchdog {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            inner: Mutex::new(WatchdogInner::default()),
        }
    }

    fn arm(&self, client: &str) {
        let now = self.start.elapsed().as_millis() as u64;
        let mut inner = self.inner.lock().unwrap();
        inner.deadline_ms = Some(now + TX_SAFETY_TIMEOUT.as_millis() as u64);
        inner.client = Some(client.to_string());
    }

    fn disarm(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.deadline_ms = None;
        inner.client = None;
    }

    /// Release the watchdog if `client` is the one currently holding PTT.
    fn release_if(&self, client: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.client.as_deref() == Some(client) && inner.deadline_ms.is_some() {
            inner.deadline_ms = None;
            inner.client = None;
            true
        } else {
            false
        }
    }

    fn expired(&self) -> bool {
        let now = self.start.elapsed().as_millis() as u64;
        let mut inner = self.inner.lock().unwrap();
        if inner.deadline_ms.is_some_and(|deadline| now >= deadline) {
            inner.deadline_ms = None;
            inner.client = None;
            true
        } else {
            false
        }
    }
}

struct Context {
    link: Arc<dyn RadioLink>,
    state: SharedState,
    external_ptt: Arc<AtomicBool>,
    watchdog: Arc<Watchdog>,
    clients: Arc<AtomicUsize>,
    /// Whether clients may change the radio's VFO selection and split state.
    /// When `false`, `V` is ignored and `S`/`I`/`X` only update a soft state.
    split_control: Arc<AtomicBool>,
}

/// A running rigctld server on its own thread/runtime.
pub struct Rigctld {
    port: u16,
    running: Arc<AtomicBool>,
    notify: Arc<Notify>,
    clients: Arc<AtomicUsize>,
    external_ptt: Arc<AtomicBool>,
    split_control: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Rigctld {
    /// Bind `port` (0 picks a free one) and start serving against the session.
    pub fn start(handle: ScuHandle, state: SharedState, port: u16) -> Result<Self, RigctldError> {
        Self::start_with_link(Arc::new(handle), state, port)
    }

    /// Like [`Rigctld::start`] but over an arbitrary [`RadioLink`] (for tests).
    pub fn start_with_link(
        link: Arc<dyn RadioLink>,
        state: SharedState,
        port: u16,
    ) -> Result<Self, RigctldError> {
        let listener =
            std::net::TcpListener::bind(("0.0.0.0", port)).map_err(RigctldError::Bind)?;
        listener.set_nonblocking(true).map_err(RigctldError::Bind)?;
        let port = listener.local_addr().map_err(RigctldError::Bind)?.port();

        let running = Arc::new(AtomicBool::new(true));
        let notify = Arc::new(Notify::new());
        let clients = Arc::new(AtomicUsize::new(0));
        let external_ptt = Arc::new(AtomicBool::new(false));
        let split_control = Arc::new(AtomicBool::new(false));

        let thread = {
            let running = Arc::clone(&running);
            let notify = Arc::clone(&notify);
            let clients = Arc::clone(&clients);
            let external_ptt = Arc::clone(&external_ptt);
            let split_control = Arc::clone(&split_control);
            std::thread::Builder::new()
                .name("scu-rigctld".into())
                .spawn(move || {
                    let runtime = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            tracing::error!(%error, "failed to build rigctld runtime");
                            running.store(false, Ordering::SeqCst);
                            return;
                        }
                    };
                    runtime.block_on(run_server(
                        listener,
                        link,
                        state,
                        external_ptt,
                        split_control,
                        clients,
                        notify,
                        running,
                    ));
                })
                .map_err(RigctldError::Thread)?
        };

        Ok(Self {
            port,
            running,
            notify,
            clients,
            external_ptt,
            split_control,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Flag set while an external client owns PTT, so the app can gate TX audio.
    pub fn external_ptt(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.external_ptt)
    }

    /// Allow (`true`) or forbid (`false`, the default) clients to change the
    /// radio's receive-VFO selection and split state.
    ///
    /// Forbidden, `V` is ignored and `S`/`I`/`X` only update a soft state that
    /// is reported back to the client; nothing is sent to the radio. That is
    /// enough for WSJT-X with *Split Operation: None* or *Fake It*, and it means
    /// a client cannot flip the radio into split by accident. Enable it for
    /// *Split Operation: Rig*.
    pub fn set_split_control(&self, allow: bool) {
        self.split_control.store(allow, Ordering::Relaxed);
    }

    pub fn status(&self) -> RigctldStatus {
        RigctldStatus {
            port: self.port,
            clients: self.clients.load(Ordering::Relaxed),
            running: self.running.load(Ordering::Relaxed),
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.notify.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Rigctld {
    fn drop(&mut self) {
        self.stop();
    }
}

// Each argument is a distinct piece of shared server state handed to the thread.
#[allow(clippy::too_many_arguments)]
async fn run_server(
    listener: std::net::TcpListener,
    link: Arc<dyn RadioLink>,
    state: SharedState,
    external_ptt: Arc<AtomicBool>,
    split_control: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    notify: Arc<Notify>,
    running: Arc<AtomicBool>,
) {
    let listener = match TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%error, "failed to adopt rigctld listener");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let watchdog = Arc::new(Watchdog::new());
    let ctx = Arc::new(Context {
        link,
        state,
        external_ptt,
        watchdog: Arc::clone(&watchdog),
        clients: Arc::clone(&clients),
        split_control,
    });
    tracing::info!(
        port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0),
        "rigctld server listening"
    );

    // TX safety watchdog.
    {
        let ctx = Arc::clone(&ctx);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if ctx.watchdog.expired() {
                    tracing::warn!("PTT held via rigctld with no release; forcing TX0;");
                    ctx.external_ptt.store(false, Ordering::Relaxed);
                    if let Ok(mut state) = ctx.state.lock() {
                        state.ptt = false;
                    }
                    ctx.link.send("TX0;");
                }
            }
        });
    }

    loop {
        tokio::select! {
            _ = notify.notified() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _peer)) => {
                    clients.fetch_add(1, Ordering::Relaxed);
                    let ctx = Arc::clone(&ctx);
                    tokio::spawn(async move { handle_client(stream, ctx).await });
                }
                Err(error) => tracing::debug!(%error, "rigctld accept failed"),
            },
        }
    }

    running.store(false, Ordering::SeqCst);
    tracing::info!("rigctld server stopped");
}

async fn handle_client(stream: TcpStream, ctx: Arc<Context>) {
    let peer = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unknown".into());
    let client_id = format!("rigctld-{peer}");
    tracing::debug!(%client_id, "rigctld client connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                tracing::debug!(%client_id, %error, "rigctld read error");
                break;
            }
        }
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        tracing::debug!(%client_id, %command, "rigctld command");

        let (response, close) = process_command(command, &ctx, &client_id);
        if !response.is_empty() {
            let mut out = response;
            out.push('\n');
            if write_half.write_all(out.as_bytes()).await.is_err() {
                break;
            }
        }
        if close {
            break;
        }
    }

    if ctx.watchdog.release_if(&client_id) {
        tracing::warn!(%client_id, "client disconnected holding PTT; forced TX0;");
        ctx.external_ptt.store(false, Ordering::Relaxed);
        if let Ok(mut state) = ctx.state.lock() {
            state.ptt = false;
        }
        ctx.link.send("TX0;");
    }
    ctx.clients.fetch_sub(1, Ordering::Relaxed);
    tracing::debug!(%client_id, "rigctld client disconnected");
}

/// Parse and answer one rigctld command. Returns `(response, close_connection)`.
fn process_command(command: &str, ctx: &Context, client_id: &str) -> (String, bool) {
    let command = command.strip_prefix('\\').unwrap_or(command);
    let parts: Vec<&str> = command.split_whitespace().collect();
    let Some(&cmd) = parts.first() else {
        return ("RPRT 0".to_string(), false);
    };
    let arg = |index: usize| parts.get(index).copied();

    // Single-character commands are case-sensitive: lower = get, upper = set.
    if cmd.len() == 1 {
        let response = match cmd {
            "f" => get_frequency(ctx),
            "F" => arg(1).map_or_else(rprt_fail, |value| set_frequency(ctx, value)),
            "m" => get_mode(ctx),
            "M" => arg(1).map_or_else(rprt_fail, |mode| set_mode(ctx, mode)),
            "v" => get_vfo(ctx),
            "V" => set_vfo(ctx, arg(1).unwrap_or_default(), client_id),
            "t" => get_ptt(ctx),
            "T" => arg(1).map_or_else(rprt_fail, |value| set_ptt(ctx, value, client_id)),
            "s" => get_split_vfo(ctx),
            "S" => arg(1).map_or_else(rprt_fail, |value| set_split(ctx, value, arg(2), client_id)),
            "i" => get_split_frequency(ctx),
            "I" => arg(1).map_or_else(rprt_fail, |value| {
                set_split_frequency(ctx, value, client_id)
            }),
            "x" => get_split_mode(ctx),
            "X" => arg(1).map_or_else(rprt_fail, |mode| set_split_mode(ctx, mode, client_id)),
            "j" => get_rit(ctx),
            "J" => arg(1).map_or_else(rprt_fail, |value| set_rit(ctx, value)),
            "z" => get_xit(ctx),
            "Z" => arg(1).map_or_else(rprt_fail, |value| set_xit(ctx, value)),
            "l" => arg(1).map_or_else(rprt_fail, |level| get_level(ctx, level)),
            "L" => arg(2).map_or_else(rprt_fail, |value| {
                set_level(ctx, arg(1).unwrap_or_default(), value)
            }),
            "u" => arg(1).map_or_else(rprt_fail, |func| get_func(ctx, func)),
            "U" => arg(2).map_or_else(rprt_fail, |value| {
                set_func(ctx, arg(1).unwrap_or_default(), value)
            }),
            "r" => "None".to_string(),
            "R" => "RPRT 0".to_string(),
            "N" => "0".to_string(),
            "n" => "RPRT 0".to_string(),
            "_" => get_info(ctx),
            "1" => dump_state(),
            "q" | "Q" => return ("RPRT 0".to_string(), true),
            _ => rprt_fail(),
        };
        return (response, false);
    }

    // Long-form commands are case-insensitive.
    let response = match cmd.to_ascii_lowercase().as_str() {
        "get_freq" => get_frequency(ctx),
        "set_freq" => arg(1).map_or_else(rprt_fail, |value| set_frequency(ctx, value)),
        "get_mode" => get_mode(ctx),
        "set_mode" => arg(1).map_or_else(rprt_fail, |mode| set_mode(ctx, mode)),
        "get_vfo" => get_vfo(ctx),
        "set_vfo" => set_vfo(ctx, arg(1).unwrap_or_default(), client_id),
        "get_ptt" => get_ptt(ctx),
        "set_ptt" => arg(1).map_or_else(rprt_fail, |value| set_ptt(ctx, value, client_id)),
        "get_split_vfo" => get_split_vfo(ctx),
        "set_split_vfo" => {
            arg(1).map_or_else(rprt_fail, |value| set_split(ctx, value, arg(2), client_id))
        }
        "get_split_freq" => get_split_frequency(ctx),
        "set_split_freq" => arg(1).map_or_else(rprt_fail, |value| {
            set_split_frequency(ctx, value, client_id)
        }),
        "get_split_mode" => get_split_mode(ctx),
        "set_split_mode" => {
            arg(1).map_or_else(rprt_fail, |mode| set_split_mode(ctx, mode, client_id))
        }
        "get_rit" => get_rit(ctx),
        "set_rit" => arg(1).map_or_else(rprt_fail, |value| set_rit(ctx, value)),
        "get_xit" => get_xit(ctx),
        "set_xit" => arg(1).map_or_else(rprt_fail, |value| set_xit(ctx, value)),
        "get_level" => arg(1).map_or_else(rprt_fail, |level| get_level(ctx, level)),
        "set_level" => arg(2).map_or_else(rprt_fail, |value| {
            set_level(ctx, arg(1).unwrap_or_default(), value)
        }),
        "get_func" => arg(1).map_or_else(rprt_fail, |func| get_func(ctx, func)),
        "set_func" => arg(2).map_or_else(rprt_fail, |value| {
            set_func(ctx, arg(1).unwrap_or_default(), value)
        }),
        "get_info" => get_info(ctx),
        "set_band" => arg(1).map_or_else(rprt_fail, |band| set_band(ctx, band)),
        "get_powerstat" => get_powerstat(ctx),
        "set_powerstat" => arg(1).map_or_else(rprt_fail, |value| set_powerstat(ctx, value)),
        "get_dcd" => "0".to_string(),
        "chk_vfo" => "0".to_string(),
        "dump_state" | "dump_caps" => dump_state(),
        "get_parm" => "RPRT -1".to_string(),
        "set_parm" => "RPRT 0".to_string(),
        "get_conf" => "".to_string(),
        "set_conf" => "RPRT 0".to_string(),
        "get_ant" => "RPRT -1".to_string(),
        "set_ant" => "RPRT 0".to_string(),
        "quit" => return ("RPRT 0".to_string(), true),
        _ => rprt_fail(),
    };
    (response, false)
}

fn rprt_fail() -> String {
    "RPRT -1".to_string()
}

fn get_frequency(ctx: &Context) -> String {
    let hz = ctx.state.lock().unwrap().frequency();
    if hz == 0 {
        rprt_fail()
    } else {
        hz.to_string()
    }
}

fn set_frequency(ctx: &Context, value: &str) -> String {
    let Ok(hz) = value.parse::<f64>() else {
        return rprt_fail();
    };
    let hz = hz.round().max(0.0) as u64;
    if hz == 0 || hz > 999_999_990 {
        return rprt_fail();
    }
    // Read the target VFO and update the cache under one lock so a concurrent
    // VFO change cannot split the "which VFO" decision from the cache write.
    let (sub, command) = {
        let mut state = ctx.state.lock().unwrap();
        let sub = state.sub_vfo;
        state.set_frequency(hz);
        (sub, frequency_command(sub, hz))
    };
    tracing::info!(hz, sub, %command, "rigctld set_freq");
    ctx.link.send(&command);
    "RPRT 0".to_string()
}

/// CAT command that sets `hz` on VFO-A (`sub == false`) or VFO-B.
fn frequency_command(sub: bool, hz: u64) -> String {
    if sub {
        scu_cat::set_frequency_b(hz)
    } else {
        scu_cat::set_frequency(hz)
    }
}

fn get_mode(ctx: &Context) -> String {
    let mode = ctx
        .state
        .lock()
        .unwrap()
        .mode()
        .unwrap_or(scu_cat::Mode::Usb);
    format!("{}\n0", mode_to_hamlib(mode))
}

fn set_mode(ctx: &Context, value: &str) -> String {
    let Some(mode) = mode_from_hamlib(value) else {
        return "RPRT -1 // E_MODE: Unsupported mode for this rig.".to_string();
    };
    // Hamlib's `set_mode` targets the current (active) VFO. The FTDX10's `MD`
    // P1 is relative to the active VFO, so the active VFO is always `MD0`.
    let command = scu_cat::set_mode_vfo(false, mode);
    tracing::info!(?mode, %command, "rigctld set_mode");
    ctx.link.send(&command);

    // Re-assert the tuned frequency immediately (so the mode→frequency order is
    // right even if the radio applies the mode synchronously), then once more
    // after the mode has settled. On the FTDX10 (and the other Yaesu rigs) `MD`
    // can shift the dial by the mode's carrier offset (e.g. the CW/RTTY pitch),
    // and the radio applies that offset a moment after acknowledging the mode.
    // Clients such as WaveLogGate apply a QSY as `F <hz>` followed by
    // `M <mode> -1`, so the shift lands after the frequency and leaves the rig
    // off-channel; Wavelog then never confirms the spot and the DX Waterfall
    // click is lost.
    let (sub, hz, seq) = {
        let mut state = ctx.state.lock().unwrap();
        state.set_mode(mode);
        (state.sub_vfo, state.frequency(), state.freq_seq())
    };
    if hz > 0 {
        ctx.link.send(&frequency_command(sub, hz));
    }

    // The deferred re-send uses the frequency captured now, and is dropped if
    // the client has since changed the frequency or the VFO: re-reading the
    // cache later could resurrect a stale value and detune RX from TX.
    let link = Arc::clone(&ctx.link);
    let state = Arc::clone(&ctx.state);
    std::thread::spawn(move || {
        std::thread::sleep(MODE_SETTLE_DELAY);
        let superseded = {
            let state = state.lock().unwrap();
            state.freq_seq() != seq || state.sub_vfo != sub
        };
        if hz > 0 && !superseded {
            link.send(&frequency_command(sub, hz));
        }
    });
    "RPRT 0".to_string()
}

fn get_vfo(ctx: &Context) -> String {
    if ctx.state.lock().unwrap().sub_vfo {
        "VFOB".to_string()
    } else {
        "VFOA".to_string()
    }
}

fn split_control_enabled(ctx: &Context) -> bool {
    ctx.split_control.load(Ordering::Relaxed)
}

/// `set_vfo`. Selecting the operating VFO moves the radio's RX *and* TX, so it
/// is only forwarded when split control is enabled; otherwise it is ignored
/// (the client keeps working on whichever VFO the radio is already using).
fn set_vfo(ctx: &Context, vfo: &str, client_id: &str) -> String {
    let sub = vfo.to_ascii_uppercase().contains('B') || vfo.eq_ignore_ascii_case("Sub");
    if !split_control_enabled(ctx) {
        tracing::info!(%client_id, vfo, "rigctld set_vfo ignored (split control disabled)");
        return "RPRT 0".to_string();
    }
    tracing::info!(%client_id, vfo, sub, "rigctld set_vfo -> VS");
    ctx.link.send(scu_cat::select_vfo(sub));
    ctx.state.lock().unwrap().set_sub_vfo(sub);
    "RPRT 0".to_string()
}

fn get_ptt(ctx: &Context) -> String {
    let on = ctx.external_ptt.load(Ordering::Relaxed) || ctx.state.lock().unwrap().ptt;
    if on {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

fn set_ptt(ctx: &Context, value: &str, client_id: &str) -> String {
    let keyed = value.trim() != "0";
    ctx.link.send(if keyed { "TX1;" } else { "TX0;" });
    ctx.external_ptt.store(keyed, Ordering::Relaxed);
    ctx.state.lock().unwrap().ptt = keyed;
    if keyed {
        ctx.watchdog.arm(client_id);
    } else {
        ctx.watchdog.disarm();
    }
    "RPRT 0".to_string()
}

fn vfo_name(sub: bool) -> &'static str {
    if sub {
        "VFOB"
    } else {
        "VFOA"
    }
}

/// `get_split_vfo`: split state plus the VFO that transmits. With split on the
/// transmitter is the VFO *not* used for receive, so it is VFO-B while
/// receiving on A and VFO-A while receiving on B.
/// Always reports the radio's *real* split state, whether or not clients may
/// change it. Reporting a soft "off" while the radio was actually split would
/// hide the very condition that makes TX land on the other VFO.
fn get_split_vfo(ctx: &Context) -> String {
    let state = ctx.state.lock().unwrap();
    format!("{}\n{}", state.split as u8, vfo_name(state.tx_sub()))
}

/// Strictly parse a Hamlib split flag (`0` or `1`).
fn parse_split_flag(value: &str) -> Option<bool> {
    match value.trim() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// `set_split_vfo <split> [<tx_vfo>]`. The FTDX10's `ST` always transmits on
/// the VFO opposite the receive VFO, so a requested TX VFO that equals the
/// receive VFO cannot be honoured; it is logged and treated as "the other VFO".
///
/// With split control disabled, enabling split is blocked but *disabling* it is
/// forwarded: clearing split can only move the radio toward simplex, and it
/// self-heals a split the app never asked for.
fn set_split(ctx: &Context, value: &str, tx_vfo: Option<&str>, client_id: &str) -> String {
    let Some(on) = parse_split_flag(value) else {
        tracing::warn!(%client_id, value, "rigctld set_split_vfo rejected: split must be 0 or 1");
        return rprt_fail();
    };
    if !split_control_enabled(ctx) {
        if on {
            tracing::info!(%client_id, "rigctld set_split_vfo ON ignored (split control disabled)");
            return "RPRT 0".to_string();
        }
        if ctx.state.lock().unwrap().split {
            tracing::info!(%client_id, "rigctld set_split_vfo OFF -> ST0 (clearing split)");
            ctx.link.send(scu_cat::set_split(false));
            ctx.state.lock().unwrap().set_split_on(false);
        }
        return "RPRT 0".to_string();
    }
    let rx_sub = ctx.state.lock().unwrap().sub_vfo;
    if let Some(tx) = tx_vfo {
        let tx_sub = tx.to_ascii_uppercase().contains('B') || tx.eq_ignore_ascii_case("Sub");
        if on && tx_sub == rx_sub {
            tracing::warn!(
                %client_id,
                tx,
                rx = vfo_name(rx_sub),
                "rigctld set_split_vfo asked to transmit on the receive VFO; using the other VFO"
            );
        }
    }
    tracing::info!(%client_id, on, "rigctld set_split_vfo -> ST");
    ctx.link.send(scu_cat::set_split(on));
    ctx.state.lock().unwrap().set_split_on(on);
    "RPRT 0".to_string()
}

/// Frequency of the split transmit VFO (the one not selected for receive).
fn get_split_frequency(ctx: &Context) -> String {
    let hz = ctx.state.lock().unwrap().other_frequency();
    if hz == 0 {
        rprt_fail()
    } else {
        hz.to_string()
    }
}

fn set_split_frequency(ctx: &Context, value: &str, client_id: &str) -> String {
    let Ok(hz) = value.parse::<f64>() else {
        return rprt_fail();
    };
    let hz = hz.round().max(0.0) as u64;
    if hz == 0 || hz > 999_999_990 {
        return rprt_fail();
    }
    if !split_control_enabled(ctx) {
        // Only meaningful in split, and client-initiated split is blocked.
        tracing::info!(%client_id, hz, "rigctld set_split_freq ignored (split control disabled)");
        return "RPRT 0".to_string();
    }
    // Target the VFO opposite the receive VFO: VFO-B normally, VFO-A when
    // receiving on B. Always writing VFO-B would retune the receive VFO.
    let (tx_sub, command) = {
        let mut state = ctx.state.lock().unwrap();
        let tx_sub = !state.sub_vfo;
        state.set_other_frequency(hz);
        (tx_sub, frequency_command(tx_sub, hz))
    };
    tracing::info!(%client_id, hz, tx_sub, %command, "rigctld set_split_freq");
    ctx.link.send(&command);
    "RPRT 0".to_string()
}

fn get_split_mode(ctx: &Context) -> String {
    let mode = {
        let state = ctx.state.lock().unwrap();
        state
            .other_mode()
            .or(state.mode())
            .unwrap_or(scu_cat::Mode::Usb)
    };
    format!("{}\n0", mode_to_hamlib(mode))
}

fn set_split_mode(ctx: &Context, value: &str, client_id: &str) -> String {
    let Some(mode) = mode_from_hamlib(value) else {
        return rprt_fail();
    };
    if !split_control_enabled(ctx) {
        tracing::info!(%client_id, ?mode, "rigctld set_split_mode ignored (split control disabled)");
        return "RPRT 0".to_string();
    }
    // `MD1` addresses the inactive VFO, i.e. the split transmit VFO.
    tracing::info!(%client_id, ?mode, "rigctld set_split_mode -> MD1");
    ctx.link.send(&scu_cat::set_mode_vfo(true, mode));
    ctx.state.lock().unwrap().set_other_mode(mode);
    "RPRT 0".to_string()
}

fn get_rit(ctx: &Context) -> String {
    ctx.state.lock().unwrap().rit_hz.to_string()
}

fn set_rit(ctx: &Context, value: &str) -> String {
    let Ok(hz) = value.parse::<i32>() else {
        return rprt_fail();
    };
    let hz = scu_cat::clamp_clarifier_hz(hz);
    let sub = ctx.state.lock().unwrap().sub_vfo;
    ctx.link.send(&scu_cat::set_clarifier_offset(sub, hz));
    let mut state = ctx.state.lock().unwrap();
    // One physical clarifier offset is shared by RIT and XIT.
    state.rit_hz = hz;
    state.xit_hz = hz;
    "RPRT 0".to_string()
}

fn get_xit(ctx: &Context) -> String {
    ctx.state.lock().unwrap().xit_hz.to_string()
}

fn set_xit(ctx: &Context, value: &str) -> String {
    let Ok(hz) = value.parse::<i32>() else {
        return rprt_fail();
    };
    let hz = scu_cat::clamp_clarifier_hz(hz);
    let sub = ctx.state.lock().unwrap().sub_vfo;
    ctx.link.send(&scu_cat::set_clarifier_offset(sub, hz));
    let mut state = ctx.state.lock().unwrap();
    state.rit_hz = hz;
    state.xit_hz = hz;
    "RPRT 0".to_string()
}

fn get_level(ctx: &Context, level: &str) -> String {
    let state = ctx.state.lock().unwrap();
    match level.to_ascii_uppercase().as_str() {
        "STRENGTH" => {
            let dbm = (state.smeter as f64 / 255.0) * 60.0 - 60.0;
            format!("{dbm:.0}")
        }
        "RFPOWER" | "RFPOWER_METER_WATTS" => format!("{:.6}", state.power_w as f64 / 100.0),
        "RF" => format!("{:.6}", state.rf_gain as f64 / 255.0),
        "SQL" => format!("{:.6}", state.squelch as f64 / 100.0),
        "MICGAIN" => format!("{:.6}", state.mic_gain as f64 / 100.0),
        _ => "0".to_string(),
    }
}

fn set_level(ctx: &Context, level: &str, value: &str) -> String {
    let Ok(raw) = value.parse::<f64>() else {
        return rprt_fail();
    };
    let upper = level.to_ascii_uppercase();
    match upper.as_str() {
        "RFPOWER" => {
            let watts = (raw.clamp(0.0, 1.0) * scu_cat::POWER_MAX_W as f64).round() as u16;
            let command = scu_cat::set_power(watts);
            ctx.link.send(&command);
            ctx.state.lock().unwrap().power_w = watts;
        }
        "RF" => {
            let gain = (raw.clamp(0.0, 1.0) * scu_cat::RF_GAIN_MAX as f64).round() as u8;
            ctx.link.send(&scu_cat::set_rf_gain(gain));
            ctx.state.lock().unwrap().rf_gain = gain;
        }
        "SQL" => {
            let value = (raw.clamp(0.0, 1.0) * scu_cat::SQUELCH_MAX as f64).round() as u8;
            ctx.link.send(&scu_cat::set_squelch(value));
            ctx.state.lock().unwrap().squelch = value;
        }
        "MICGAIN" => {
            let value = (raw.clamp(0.0, 1.0) * scu_cat::MIC_GAIN_MAX as f64).round() as u8;
            ctx.link.send(&scu_cat::set_mic_gain(value));
            ctx.state.lock().unwrap().mic_gain = value;
        }
        "NB" => {
            let on = raw != 0.0;
            ctx.link.send(&scu_cat::set_noise_blanker(on));
            ctx.state.lock().unwrap().nb = on;
        }
        "NR" => {
            let on = raw != 0.0;
            ctx.link.send(&scu_cat::set_noise_reduction(on));
            ctx.state.lock().unwrap().nr = on;
        }
        _ => return "RPRT 0".to_string(),
    }
    "RPRT 0".to_string()
}

fn get_func(ctx: &Context, func: &str) -> String {
    let state = ctx.state.lock().unwrap();
    let on = match func.to_ascii_uppercase().as_str() {
        "TUNER" => state.atu_on,
        "RIT" => state.rit_on,
        "XIT" => state.xit_on,
        "NB" => state.nb,
        "NR" => state.nr,
        "ANF" | "BC" => state.auto_notch,
        _ => return rprt_fail(),
    };
    if on {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

fn set_func(ctx: &Context, func: &str, value: &str) -> String {
    let on = value.trim() != "0";
    match func.to_ascii_uppercase().as_str() {
        "TUNER" => {
            ctx.link.send(&scu_cat::set_atu(on));
            ctx.state.lock().unwrap().atu_on = on;
        }
        "RIT" => {
            ctx.link.send(scu_cat::set_rit(on));
            ctx.state.lock().unwrap().rit_on = on;
        }
        "XIT" => {
            ctx.link.send(scu_cat::set_xit(on));
            ctx.state.lock().unwrap().xit_on = on;
        }
        "NB" => {
            ctx.link.send(&scu_cat::set_noise_blanker(on));
            ctx.state.lock().unwrap().nb = on;
        }
        "NR" => {
            ctx.link.send(&scu_cat::set_noise_reduction(on));
            ctx.state.lock().unwrap().nr = on;
        }
        "ANF" | "BC" => {
            ctx.link.send(&scu_cat::set_auto_notch(on));
            ctx.state.lock().unwrap().auto_notch = on;
        }
        _ => return rprt_fail(),
    }
    "RPRT 0".to_string()
}

fn set_band(ctx: &Context, band: &str) -> String {
    if let Some(code) = band_code(band) {
        ctx.link.send(&scu_cat::set_band(code));
        return "RPRT 0".to_string();
    }
    // Fall back to treating the argument as a frequency in Hz.
    set_frequency(ctx, band)
}

fn band_code(band: &str) -> Option<u8> {
    Some(match band.to_ascii_lowercase().as_str() {
        "160m" => 0,
        "80m" => 1,
        "60m" => 2,
        "40m" => 3,
        "30m" => 4,
        "20m" => 5,
        "17m" => 6,
        "15m" => 7,
        "12m" => 8,
        "10m" => 9,
        "6m" => 10,
        "4m" => 11,
        _ => return None,
    })
}

fn get_powerstat(ctx: &Context) -> String {
    if ctx.state.lock().unwrap().power_on {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

fn set_powerstat(ctx: &Context, value: &str) -> String {
    let on = matches!(value.trim(), "1" | "4");
    ctx.link.send(scu_cat::set_radio_power(on));
    ctx.state.lock().unwrap().power_on = on;
    "RPRT 0".to_string()
}

fn get_info(ctx: &Context) -> String {
    let state = ctx.state.lock().unwrap();
    let (name, id) = match &state.radio {
        Some(model) => (model.name(), model.id()),
        None => ("FTDX10".to_string(), 761),
    };
    format!("Yaesu;{name};1.0.0;000000;{id}")
}

/// Minimal `\dump_state` block, matching the response the reference client
/// ships (known to satisfy WSJT-X and Log4OM).
fn dump_state() -> String {
    [
        "0",
        "2",
        "1800000.000000 30000000.000000 0x1ff -1 -1 0x10000003 0x3",
        "50000000.000000 54000000.000000 0x1ff -1 -1 0x10000003 0x3",
        "0 0 0 0 0 0 0",
        "1800000.000000 30000000.000000 0x1ff 5 200 0x10000003 0x3",
        "50000000.000000 54000000.000000 0x1ff 5 100 0x10000003 0x3",
        "0 0 0 0 0 0 0",
        "0 0",
        "0 0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0",
        "0x00000003",
        "0x00000003",
        "0x000fffff",
        "0x000fffff",
        "0",
        "0",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RadioState;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockLink {
        sent: Mutex<Vec<String>>,
    }

    impl RadioLink for MockLink {
        fn send(&self, command: &str) {
            self.sent.lock().unwrap().push(command.to_string());
        }
    }

    struct Harness {
        ctx: Context,
        link: Arc<MockLink>,
    }

    fn harness() -> Harness {
        let link = Arc::new(MockLink::default());
        let ctx = Context {
            link: Arc::clone(&link) as Arc<dyn RadioLink>,
            state: Arc::new(Mutex::new(RadioState::default())),
            external_ptt: Arc::new(AtomicBool::new(false)),
            watchdog: Arc::new(Watchdog::new()),
            clients: Arc::new(AtomicUsize::new(0)),
            split_control: Arc::new(AtomicBool::new(false)),
        };
        Harness { ctx, link }
    }

    /// A harness where clients may change the radio's VFO and split state.
    fn harness_with_split_control() -> Harness {
        let h = harness();
        h.ctx.split_control.store(true, Ordering::Relaxed);
        h
    }

    fn run(h: &Harness, command: &str) -> (String, bool) {
        process_command(command, &h.ctx, "test-client")
    }

    #[test]
    fn frequency_set_then_get_uses_cache() {
        let h = harness();
        assert_eq!(run(&h, "F 14074000").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["FA014074000;"]);
        assert_eq!(run(&h, "f").0, "14074000");
        assert_eq!(run(&h, "\\set_freq 7074000").0, "RPRT 0");
        assert_eq!(run(&h, "\\get_freq").0, "7074000");
    }

    #[test]
    fn mode_round_trip() {
        let h = harness();
        assert_eq!(run(&h, "M USB 2400").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["MD02;"]);
        assert_eq!(run(&h, "m").0, "USB\n0");
        assert_eq!(run(&h, "M PKTUSB 0").0, "RPRT 0");
        assert_eq!(run(&h, "m").0, "PKTUSB\n0");
        assert!(run(&h, "M BOGUS 0").0.starts_with("RPRT -1"));
    }

    #[test]
    fn mode_change_re_asserts_tuned_frequency() {
        let h = harness();
        // WaveLogGate applies a Wavelog QSY as `F <hz>` followed by `M <mode> -1`.
        assert_eq!(run(&h, "F 14062000").0, "RPRT 0");
        assert_eq!(run(&h, "M CW -1").0, "RPRT 0");
        // The mode is sent, then the frequency is re-applied so the rig does not
        // settle on the mode's carrier offset (the FTDX10 shifts on mode change).
        assert_eq!(
            h.link.sent.lock().unwrap().as_slice(),
            ["FA014062000;", "MD03;", "FA014062000;"]
        );
    }

    #[test]
    fn ptt_keys_and_releases() {
        let h = harness();
        assert_eq!(run(&h, "T 3").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["TX1;"]);
        assert!(h.ctx.external_ptt.load(Ordering::Relaxed));
        assert_eq!(run(&h, "t").0, "1");
        assert_eq!(run(&h, "T 0").0, "RPRT 0");
        assert!(!h.ctx.external_ptt.load(Ordering::Relaxed));
        assert_eq!(run(&h, "t").0, "0");
    }

    #[test]
    fn split_and_clarifier() {
        let h = harness_with_split_control();
        assert_eq!(run(&h, "S 1 VFOB").0, "RPRT 0");
        assert_eq!(run(&h, "s").0, "1\nVFOB");
        assert_eq!(run(&h, "I 7074000").0, "RPRT 0");
        assert_eq!(run(&h, "i").0, "7074000");
        assert_eq!(run(&h, "J 100").0, "RPRT 0");
        assert_eq!(run(&h, "j").0, "100");
        assert_eq!(run(&h, "Z -50").0, "RPRT 0");
        assert_eq!(run(&h, "z").0, "-50");
    }

    #[test]
    fn split_targets_the_vfo_opposite_receive() {
        let h = harness_with_split_control();
        // Receiving on A: split TX is VFO-B.
        assert_eq!(run(&h, "S 1 VFOB").0, "RPRT 0");
        assert_eq!(run(&h, "s").0, "1\nVFOB");
        assert_eq!(run(&h, "I 14075000").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().last().unwrap(), "FB014075000;");

        // Receiving on B: split TX is VFO-A, and VFO-B (receive) is untouched.
        h.link.sent.lock().unwrap().clear();
        assert_eq!(run(&h, "F 7074000").0, "RPRT 0");
        assert_eq!(run(&h, "V VFOB").0, "RPRT 0");
        assert_eq!(run(&h, "F 7074000").0, "RPRT 0");
        h.link.sent.lock().unwrap().clear();
        assert_eq!(run(&h, "s").0, "1\nVFOA");
        assert_eq!(run(&h, "I 7076000").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["FA007076000;"]);
        assert_eq!(run(&h, "i").0, "7076000");
        assert_eq!(run(&h, "f").0, "7074000");
    }

    #[test]
    fn split_is_blocked_but_reads_stay_truthful() {
        let h = harness();
        assert_eq!(run(&h, "F 14074000").0, "RPRT 0");
        h.link.sent.lock().unwrap().clear();

        // A client asking for split / another VFO is answered but never reaches
        // the radio...
        assert_eq!(run(&h, "S 1 VFOB").0, "RPRT 0");
        assert_eq!(run(&h, "I 14075000").0, "RPRT 0");
        assert_eq!(run(&h, "X PKTUSB 0").0, "RPRT 0");
        assert_eq!(run(&h, "V VFOB").0, "RPRT 0");
        assert!(h.link.sent.lock().unwrap().is_empty());
        {
            let state = h.ctx.state.lock().unwrap();
            assert!(!state.sub_vfo && !state.split);
            assert_eq!(state.freq_b, 0);
        }

        // ...and the getters report the radio's real (simplex) state, not a
        // spoofed one.
        assert_eq!(run(&h, "v").0, "VFOA");
        assert_eq!(run(&h, "s").0, "0\nVFOA");

        // Plain tuning still reaches the radio.
        assert_eq!(run(&h, "F 14076000").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["FA014076000;"]);
    }

    #[test]
    fn a_stuck_split_is_visible_and_a_client_may_clear_it() {
        let h = harness();
        // The radio is split even though nothing in this server asked for it.
        h.ctx.state.lock().unwrap().apply("ST1;");
        h.ctx.state.lock().unwrap().apply("FA007074000;");
        h.ctx.state.lock().unwrap().apply("FB014074000;");

        // The client must be able to see it...
        assert_eq!(run(&h, "s").0, "1\nVFOB");
        // ...and clearing it is forwarded so a stale split can be undone.
        h.link.sent.lock().unwrap().clear();
        assert_eq!(run(&h, "S 0 VFOA").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["ST0;"]);
        assert!(!h.ctx.state.lock().unwrap().split);
        assert_eq!(run(&h, "s").0, "0\nVFOA");
    }

    #[test]
    fn split_flag_is_parsed_strictly() {
        let h = harness_with_split_control();
        for bad in [
            "S 2 VFOB",
            "S VFOA 1 VFOB",
            "S on",
            "set_split_vfo yes VFOB",
        ] {
            assert_eq!(run(&h, bad).0, "RPRT -1", "{bad}");
        }
        assert!(h.link.sent.lock().unwrap().is_empty());
        assert!(!h.ctx.state.lock().unwrap().split);
        assert_eq!(run(&h, "S 1 VFOB").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().as_slice(), ["ST1;"]);
    }

    #[test]
    fn split_mode_targets_the_other_vfo() {
        let h = harness_with_split_control();
        run(&h, "S 1 VFOB");
        run(&h, "X PKTUSB 0");
        assert_eq!(h.link.sent.lock().unwrap().last().unwrap(), "MD1C;");
        assert_eq!(
            h.ctx.state.lock().unwrap().mode_b,
            Some(scu_cat::Mode::DataU)
        );
        assert_eq!(h.ctx.state.lock().unwrap().mode_a, None);
        run(&h, "V VFOB");
        run(&h, "X USB 0");
        assert_eq!(h.ctx.state.lock().unwrap().mode_a, Some(scu_cat::Mode::Usb));
    }

    #[test]
    fn stale_radio_replies_do_not_overwrite_a_fresh_set() {
        let h = harness_with_split_control();
        run(&h, "F 14074000");
        // A poll reply that left the radio before our `FA` was applied.
        h.ctx.state.lock().unwrap().apply("FA007074000;");
        assert_eq!(run(&h, "f").0, "14074000");
        // Same for a client-selected VFO and split state.
        run(&h, "V VFOB");
        run(&h, "S 1 VFOA");
        h.ctx.state.lock().unwrap().apply("VS0;");
        h.ctx.state.lock().unwrap().apply("ST0;");
        assert_eq!(run(&h, "v").0, "VFOB");
        assert_eq!(run(&h, "s").0, "1\nVFOA");
    }

    #[test]
    fn deferred_mode_resend_is_dropped_when_superseded() {
        let h = harness();
        run(&h, "F 14062000");
        run(&h, "M CW -1");
        // A newer set_freq lands inside the settle window.
        run(&h, "F 14074000");
        std::thread::sleep(MODE_SETTLE_DELAY + Duration::from_millis(100));
        let sent = h.link.sent.lock().unwrap().clone();
        assert!(!sent[3..].iter().any(|c| c == "FA014062000;"), "{sent:?}");
    }

    #[test]
    fn funcs_and_levels() {
        let h = harness();
        assert_eq!(run(&h, "U TUNER 1").0, "RPRT 0");
        assert_eq!(run(&h, "u TUNER").0, "1");
        assert_eq!(run(&h, "L RFPOWER 0.5").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().last().unwrap(), "PC050;");
        assert!(run(&h, "u NOPE").0.starts_with("RPRT -1"));
    }

    #[test]
    fn housekeeping_commands() {
        let h = harness();
        assert_eq!(run(&h, "\\chk_vfo").0, "0");
        assert!(run(&h, "\\dump_state").0.starts_with("0\n2\n"));
        assert!(run(&h, "get_info").0.starts_with("Yaesu;"));
        assert_eq!(run(&h, "bogus").0, "RPRT -1");
        assert_eq!(run(&h, "q"), ("RPRT 0".to_string(), true));
    }

    #[test]
    fn band_selection() {
        let h = harness();
        assert_eq!(run(&h, "set_band 40m").0, "RPRT 0");
        assert_eq!(h.link.sent.lock().unwrap().last().unwrap(), "BS03;");
    }

    #[test]
    fn answers_over_tcp() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;

        fn read(reader: &mut BufReader<TcpStream>) -> String {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line.trim_end().to_string()
        }

        let link = Arc::new(MockLink::default());
        let state = Arc::new(Mutex::new(RadioState::default()));
        let server =
            Rigctld::start_with_link(Arc::clone(&link) as Arc<dyn RadioLink>, state, 0).unwrap();

        let stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);

        for (command, expected) in [
            ("F 14074000", "RPRT 0"),
            ("f", "14074000"),
            ("M USB 2400", "RPRT 0"),
            ("T 1", "RPRT 0"),
            ("t", "1"),
            ("T 0", "RPRT 0"),
            ("t", "0"),
        ] {
            writer.write_all(format!("{command}\n").as_bytes()).unwrap();
            writer.flush().unwrap();
            assert_eq!(read(&mut reader), expected, "for {command}");
        }
        // `get_mode` returns mode then passband.
        writer.write_all(b"m\n").unwrap();
        writer.flush().unwrap();
        assert_eq!(read(&mut reader), "USB");
        assert_eq!(read(&mut reader), "0");
    }
}
