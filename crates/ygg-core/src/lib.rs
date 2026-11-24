//! Thin wrapper around `yggdrasil-mesh` for the DirectDNS apps.
//!
//! Responsibilities:
//! - generate and persist the node seed,
//! - resolve the hardcoded public peers, probe every address, and use the
//!   lowest-RTT one as the TLS peer while keeping all IPs as DNS servers,
//! - run the node with a split `200::/7` route on Linux or a supplied TUN fd on
//!   mobile,
//! - expose a small synchronous [`Session`] API to the UI and JNI layers.

pub mod key;
pub mod linux;
mod node;
pub mod peers;
pub mod session;
mod status;

use std::fmt;

pub use key::{
    Seed, address_from_seed, default_seed_path, display_address, generate_seed,
    load_or_create_seed, read_seed,
};
pub use peers::{
    PeerSet, resolve_addresses, resolve_addresses_blocking, resolve_peers, resolve_peers_blocking,
    select_fastest,
};
pub use session::Session;
pub use status::Status;
pub use yggdrasil::tun::InterfaceSource;

/// Error type for platform-facing operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Error(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Error(message.to_string())
    }
}

/// Starts a node with a platform-provided TUN source.
pub fn start_session(seed: Seed, interface: InterfaceSource) -> Result<Session, Error> {
    Session::start(seed, interface)
}
