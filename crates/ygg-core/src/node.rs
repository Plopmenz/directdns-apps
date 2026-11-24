//! Async Yggdrasil node: core, TUN adapter, peer links and platform routing.

use std::net::IpAddr;
use std::sync::Arc;

use yggdrasil::core::{Core, Options, Peer, default_peer_filter};
use yggdrasil::ipv6rwc::ReadWriteCloser;
use yggdrasil::tun::{InterfaceSource, TunAdapter};

use crate::Error;
use crate::peers::{PeerSet, resolve_peers};
use crate::status::Status;

const TUN_MTU: u64 = 65535;

/// A running node. Dropping it stops the TUN adapter and the core.
pub(crate) struct Node {
    core: Core,
    tun: Arc<TunAdapter>,
    #[cfg(target_os = "linux")]
    network: Option<crate::linux::NetworkGuard>,
    address: String,
    dns_servers: Vec<IpAddr>,
}

impl Node {
    /// Resolves peers, starts the core and attaches the supplied TUN interface.
    pub(crate) async fn start(seed: [u8; 32], interface: InterfaceSource) -> Result<Self, Error> {
        let PeerSet { uris, dns_servers } = resolve_peers().await?;

        let secret = yggdrasil::PrivateKey::from_seed(&seed);
        let options = Options {
            peers: uris
                .into_iter()
                .map(|uri| Peer {
                    uri,
                    source_interface: String::new(),
                })
                .collect(),
            peer_filter: Some(default_peer_filter()),
            ..Options::default()
        };

        let core = Core::new(secret, options)
            .await
            .map_err(|error| Error(format!("failed to start Yggdrasil core: {error}")))?;
        let address = core.address().to_string();
        let rwc = ReadWriteCloser::new(core.clone());
        let tun = TunAdapter::new(rwc, interface, TUN_MTU)
            .await
            .map_err(|error| Error(format!("failed to start TUN adapter: {error}")))?;

        #[cfg(target_os = "linux")]
        let network = if tun.is_enabled() {
            match crate::linux::NetworkGuard::setup(tun.name(), &dns_servers).await {
                Ok(guard) => Some(guard),
                Err(error) => {
                    tun.stop();
                    core.stop();
                    return Err(error);
                }
            }
        } else {
            None
        };

        Ok(Self {
            core,
            tun,
            #[cfg(target_os = "linux")]
            network,
            address,
            dns_servers,
        })
    }

    /// Returns a snapshot of the current connection state.
    pub(crate) fn status(&self) -> Status {
        let peers = self.core.get_peers();
        let peers_up = peers.iter().filter(|peer| peer.up).count();
        let running = !self.core.is_stopped();
        let message = if !running {
            "Stopped"
        } else if peers_up > 0 {
            "Connected"
        } else {
            "Connecting"
        };

        Status {
            running,
            message: message.into(),
            address: self.address.clone(),
            peers_up,
            peers_total: peers.len(),
            dns_servers: self.dns_servers.iter().map(|ip| ip.to_string()).collect(),
        }
    }

    /// Stops packet forwarding and clears platform routing state.
    pub(crate) fn stop(&mut self) {
        self.tun.stop();
        self.core.stop();
        #[cfg(target_os = "linux")]
        {
            self.network.take();
        }
    }
}
