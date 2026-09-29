//! # scu-rigctld
//!
//! A native, in-process implementation of the Hamlib **rigctld** TCP protocol
//! that exposes a live SCU-LAN10 session to third-party software (WSJT-X,
//! fldigi, N1MM, Log4OM, …).
//!
//! Unlike the external-Hamlib approach it needs no serial PTY, no `rigctld`
//! binary and no GPL component. Getters are answered from a cached
//! [`RadioState`] fed by the CAT response stream; setters are forwarded to the
//! radio and update the cache immediately. Multiple clients share the one
//! session, and a TX safety watchdog releases PTT if a client disappears while
//! transmitting.
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::{Arc, Mutex};
//! use scu_client::{ConnectConfig, Event, ScuClient};
//! use scu_rigctld::{RadioState, Rigctld, DEFAULT_PORT};
//!
//! let mut client = ScuClient::connect(ConnectConfig::new("192.168.1.100", "user", "pass")).await?;
//! let state = Arc::new(Mutex::new(RadioState::default()));
//! let _server = Rigctld::start(client.handle(), Arc::clone(&state), DEFAULT_PORT)?;
//! while let Some(event) = client.recv().await {
//!     if let Event::Cat(frame) = event {
//!         state.lock().unwrap().apply(&frame);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

mod hamlib;
mod link;
mod server;
mod state;

pub use hamlib::{mode_from_hamlib, mode_to_hamlib};
pub use link::RadioLink;
pub use server::{Rigctld, RigctldError, RigctldStatus, DEFAULT_PORT};
pub use state::{RadioState, SharedState};
