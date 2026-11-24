//! Connection status shared with the UI and platform bridges.

use serde::Serialize;

/// Snapshot of the node state.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub running: bool,
    pub message: String,
    pub address: String,
    pub peers_up: usize,
    pub peers_total: usize,
    pub dns_servers: Vec<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            running: false,
            message: "Off".into(),
            address: String::new(),
            peers_up: 0,
            peers_total: 0,
            dns_servers: Vec::new(),
        }
    }
}

impl Status {
    /// JSON form used by the JNI bridge.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}
