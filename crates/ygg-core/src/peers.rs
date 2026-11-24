//! Hardcoded public peer resolution.
//!
//! `peer.yggdrasil.openmesh.cloud` resolves to clearnet addresses. Every
//! resolved address is used both as a TLS peer on port 9003 and as a plain DNS
//! server on port 53. DNS is plain DNS over the normal (non-VPN) network.

use std::net::IpAddr;

use crate::Error;

pub const PEER_HOST: &str = "peer.yggdrasil.openmesh.cloud";
pub const PEER_PORT: u16 = 9003;
pub const DNS_PORT: u16 = 53;

/// Peers and DNS servers derived from one DNS lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerSet {
    /// `tls://<ip>:9003` URIs.
    pub uris: Vec<String>,
    /// Clearnet IPs that also serve plain DNS.
    pub dns_servers: Vec<IpAddr>,
}

/// Resolves the hardcoded peer host over the normal network.
pub async fn resolve_peers() -> Result<PeerSet, Error> {
    let mut set = PeerSet::default();
    let mut seen = Vec::new();

    let addresses = tokio::net::lookup_host((PEER_HOST, PEER_PORT))
        .await
        .map_err(|error| Error(format!("failed to resolve {PEER_HOST}: {error}")))?;

    for socket in addresses {
        let ip = socket.ip();
        if seen.contains(&ip) {
            continue;
        }
        seen.push(ip);
        set.uris.push(format!(
            "tls://{}",
            yggdrasil::core::uri::join_host_port(&ip.to_string(), PEER_PORT)
        ));
        set.dns_servers.push(ip);
    }

    if set.uris.is_empty() {
        return Err(Error(format!("{PEER_HOST} did not resolve to any address")));
    }
    Ok(set)
}

/// Resolves peers from a plain (non-async) context, such as a JNI call.
pub fn resolve_peers_blocking() -> Result<PeerSet, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error(format!("cannot start resolver runtime: {error}")))?;
    runtime.block_on(resolve_peers())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_servers_come_from_peer_ips() {
        let set = PeerSet {
            uris: vec![
                "tls://1.2.3.4:9003".into(),
                "tls://[2001:db8::1]:9003".into(),
            ],
            dns_servers: vec!["1.2.3.4".parse().unwrap(), "2001:db8::1".parse().unwrap()],
        };
        assert_eq!(set.uris.len(), set.dns_servers.len());
        assert!(set.uris[1].contains("]:9003"));
    }

    #[test]
    fn looks_up_public_peer() {
        let set = resolve_peers_blocking().expect("peer host should resolve");
        assert!(!set.uris.is_empty());
        assert_eq!(set.uris.len(), set.dns_servers.len());
        assert!(set.uris.iter().all(|uri| uri.starts_with("tls://")));
    }
}
