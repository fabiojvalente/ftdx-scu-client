//! # scu-client
//!
//! Live SCU-LAN10 session driver built on Tokio: performs the 7-phase
//! handshake, maintains 1 Hz keepalives, and fans decoded CAT / audio / scope
//! data out as [`Event`]s.
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use scu_client::{ConnectConfig, ScuClient};
//!
//! let mut client = ScuClient::connect(ConnectConfig::new(
//!     "192.168.1.100",
//!     "defaultuser",
//!     "defaultuser",
//! )).await?;
//! let handle = client.handle();
//! handle.send_cat("FA;");
//! while let Some(event) = client.recv().await {
//!     println!("{event:?}");
//! }
//! # Ok(())
//! # }
//! ```

mod config;
mod event;
mod session;
mod transport;

pub use config::ConnectConfig;
pub use event::Event;
pub use session::{ClientError, ScuClient, ScuHandle};
pub use transport::timeout;

pub use scu_audio::AudioFrame;
pub use scu_cat::{Mode, RadioModel};
pub use scu_scope::{Colormap, FrequencyAxis, ScopeLine};
