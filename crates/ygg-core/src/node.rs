//! Async Yggdrasil node: core, TUN adapter, peer links and platform routing.

use std::net::IpAddr;
use std::sync::Arc;

use regex::Regex;
use yggdrasil::core::{Core, Options, Peer, default_peer_filter};
use yggdrasil::ipv6rwc::ReadWriteCloser;
use yggdrasil::multicast::{Multicast, MulticastInterface};
use yggdrasil::tun::{InterfaceSource, TunAdapter};

use crate::Error;
use crate::peers::{resolve_addresses, select_fastest, tls_uri};
use crate::status::Status;

const TUN_MTU: u64 = 65535;

/// A running node. Dropping it stops the TUN adapter and the core.
pub(crate) struct Node {
    core: Core,
    tun: Arc<TunAdapter>,
    #[cfg(target_os = "linux")]
    network: Option<crate::linux::NetworkGuard>,
    address: String,
    peer_uri: String,
    dns_servers: Vec<IpAddr>,
    multicast: Option<Multicast>,
}

impl Node {
    /// Starts the core and attaches the supplied TUN interface. Only DNS is
    /// resolved here; RTT probing happens after the tunnel is already up.
    pub(crate) async fn start(seed: [u8; 32], interface: InterfaceSource) -> Result<Self, Error> {
        let dns_servers = resolve_addresses().await?;
        let peer_uri = tls_uri(dns_servers[0]);

        let secret = yggdrasil::PrivateKey::from_seed(&seed);
        let options = Options {
            peers: vec![Peer {
                uri: peer_uri.clone(),
                source_interface: String::new(),
            }],
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
            peer_uri,
            dns_servers,
            multicast: None,
        })
    }

    /// Probes all resolved addresses in the background and switches to the
    /// lowest-RTT peer. The VPN is already running when this is called.
    pub(crate) fn spawn_peer_probe(&self) {
        let core = self.core.clone();
        let initial = self.peer_uri.clone();
        let dns_servers = self.dns_servers.clone();
        tokio::spawn(async move {
            let Some(best) = select_fastest(&dns_servers).await else {
                return;
            };
            let best_uri = tls_uri(best);
            if best_uri == initial {
                return;
            }
            if let Err(error) = core.remove_peer(&initial, "") {
                log::warn!("cannot remove initial peer {initial}: {error}");
                return;
            }
            if let Err(error) = core.add_peer(&best_uri, "") {
                log::warn!("cannot add lowest-RTT peer {best_uri}: {error}");
                let _ = core.add_peer(&initial, "");
                return;
            }
            log::info!("switched to lowest-RTT peer {best_uri}");
        });
    }

    /// Starts or stops local-network peer discovery. Local links are removed
    /// explicitly when stopping: `Multicast::stop()` closes listeners but does
    /// not close links that are already established.
    pub(crate) async fn set_multicast(&mut self, enabled: bool) -> Result<(), Error> {
        if enabled {
            if self.multicast.is_some() {
                return Ok(());
            }
            let config = vec![MulticastInterface {
                regex: Regex::new(".*").map_err(|error| {
                    Error(format!("invalid multicast interface filter: {error}"))
                })?,
                beacon: true,
                listen: true,
                port: 0,
                priority: 0,
                password: String::new(),
            }];
            let multicast = Multicast::new(self.core.clone(), config)
                .await
                .map_err(|error| Error(format!("cannot start multicast: {error}")))?;
            self.multicast = Some(multicast);
        } else {
            self.stop_multicast();
        }
        Ok(())
    }

    fn stop_multicast(&mut self) {
        let interfaces = self
            .multicast
            .as_ref()
            .map(|multicast| multicast.interfaces())
            .unwrap_or_default();
        if let Some(multicast) = self.multicast.take() {
            multicast.stop();
        }
        for peer in self.core.get_peers() {
            // The public peer is a clearnet address; multicast links are
            // link-local IPv6.
            if !peer.uri.contains("fe80:") {
                continue;
            }
            let zone = peer
                .uri
                .split('%')
                .nth(1)
                .and_then(|rest| rest.split(|c| c == ']' || c == ':').next())
                .filter(|zone| !zone.is_empty())
                .map(str::to_string);
            let mut candidates = Vec::new();
            candidates.extend(zone);
            candidates.extend(interfaces.iter().cloned());
            candidates.push(String::new());
            for sintf in candidates {
                if self.core.remove_peer(&peer.uri, &sintf).is_ok() {
                    break;
                }
            }
        }
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
        self.stop_multicast();
        self.core.stop();
        #[cfg(target_os = "linux")]
        {
            self.network.take();
        }
    }
}
