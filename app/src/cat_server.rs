//! Native-only "Radio Server": runs the in-process rigctld-protocol server
//! (see `scu-rigctld`) so Hamlib clients such as WSJT-X can share the session.
//!
//! There is no external `rigctld` binary or serial PTY: getters are served from
//! a cached [`RadioState`] fed by the CAT stream, setters go straight to the
//! radio, and multiple clients are accepted concurrently.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use scu_client::ScuHandle;
use scu_rigctld::{RadioState, Rigctld, DEFAULT_PORT};

/// Snapshot of the server state for display.
#[derive(Debug, Clone, Default)]
pub struct CatServerStatus {
    pub port: u16,
    pub clients: usize,
    pub running: bool,
    pub error: Option<String>,
}

/// Owns the live rigctld server and the shared radio state cache.
pub struct CatServerState {
    server: Option<Rigctld>,
    state: Arc<Mutex<RadioState>>,
    port: u16,
    error: Option<String>,
    split_control: bool,
}

impl CatServerState {
    pub fn new() -> Self {
        Self {
            server: None,
            state: Arc::new(Mutex::new(RadioState::default())),
            port: DEFAULT_PORT,
            error: None,
            split_control: false,
        }
    }

    pub fn running(&self) -> bool {
        self.server.is_some()
    }

    /// Start (or restart) the server against `handle`.
    pub fn start(&mut self, handle: ScuHandle, port: u16) {
        self.stop();
        self.port = port;
        self.error = None;
        self.state = Arc::new(Mutex::new(RadioState::default()));
        match Rigctld::start(handle, Arc::clone(&self.state), port) {
            Ok(server) => {
                server.set_split_control(self.split_control);
                self.port = server.port();
                self.server = Some(server);
            }
            Err(error) => self.error = Some(format!("rigctld server failed: {error}")),
        }
    }

    /// Whether rigctld clients may change the radio's VFO selection and split
    /// state. Applied immediately, and re-applied whenever the server restarts.
    pub fn set_split_control(&mut self, allow: bool) {
        self.split_control = allow;
        if let Some(server) = &self.server {
            server.set_split_control(allow);
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
    }

    /// Update the cached state from a CAT response frame.
    pub fn apply(&self, frame: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.apply(frame);
        }
    }

    /// Flag set while an external client owns PTT.
    pub fn external_ptt_active(&self) -> bool {
        self.server
            .as_ref()
            .is_some_and(|server| server.external_ptt().load(Ordering::Relaxed))
    }

    pub fn status(&self) -> CatServerStatus {
        match &self.server {
            Some(server) => {
                let status = server.status();
                CatServerStatus {
                    port: status.port,
                    clients: status.clients,
                    running: status.running,
                    error: self.error.clone(),
                }
            }
            None => CatServerStatus {
                port: self.port,
                error: self.error.clone(),
                ..Default::default()
            },
        }
    }
}

impl Default for CatServerState {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CatServerState {
    fn drop(&mut self) {
        self.stop();
    }
}
