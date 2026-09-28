//! Connection configuration.

use scu_protocol::DEFAULT_BASE_PORT;
use serde::{Deserialize, Serialize};

/// Where and how to reach a SCU-LAN10.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectConfig {
    pub host: String,
    pub base_port: u16,
    pub username: String,
    pub password: String,
    /// WebSocket URL of a `scu-bridge` relay. Required by the browser build;
    /// ignored by the native build, which speaks UDP directly.
    #[serde(default)]
    pub bridge_url: Option<String>,
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            host: "192.168.1.100".into(),
            base_port: DEFAULT_BASE_PORT,
            username: "defaultuser".into(),
            password: "defaultuser".into(),
            bridge_url: None,
        }
    }
}

impl ConnectConfig {
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            base_port: DEFAULT_BASE_PORT,
            username: username.into(),
            password: password.into(),
            bridge_url: None,
        }
    }

    pub fn ctrl_port(&self) -> u16 {
        self.base_port
    }
    pub fn cat_port(&self) -> u16 {
        self.base_port.wrapping_add(1)
    }
    pub fn audio_port(&self) -> u16 {
        self.base_port.wrapping_add(2)
    }
    pub fn scope_port(&self) -> u16 {
        self.base_port.wrapping_add(3)
    }
}
