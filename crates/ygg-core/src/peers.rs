//! Hardcoded public peer resolution.
//!
//! `peer.yggdrasil.openmesh.cloud` resolves to clearnet addresses. DNS results
//! are returned immediately so a VPN can be established without waiting. Every
//! resolved address is then ICMP-pinged (with a TCP connect fallback) and only
//! the lowest-RTT address is used as the `tls://<ip>:9003` peer. All resolved
//! IPs remain plain DNS servers on port 53.

use std::cmp::Ordering;
use std::mem::MaybeUninit;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::task::JoinSet;

use crate::Error;

pub const PEER_HOST: &str = "peer.yggdrasil.openmesh.cloud";
pub const PEER_PORT: u16 = 9003;
pub const DNS_PORT: u16 = 53;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);
const ECHO_PAYLOAD: &[u8; 16] = b"directdns-probe!";

/// Peers and DNS servers derived from one DNS lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerSet {
    /// The single lowest-RTT `tls://<ip>:9003` URI.
    pub uris: Vec<String>,
    /// All resolved clearnet IPs that also serve plain DNS.
    pub dns_servers: Vec<IpAddr>,
}

/// Resolves the hardcoded peer host without probing. This is fast enough to
/// run on a UI or VPN-service thread before the tunnel is established.
pub async fn resolve_addresses() -> Result<Vec<IpAddr>, Error> {
    let mut addresses = Vec::new();
    for socket in tokio::net::lookup_host((PEER_HOST, PEER_PORT))
        .await
        .map_err(|error| Error(format!("failed to resolve {PEER_HOST}: {error}")))?
    {
        let ip = socket.ip();
        if !addresses.contains(&ip) {
            addresses.push(ip);
        }
    }
    if addresses.is_empty() {
        return Err(Error(format!("{PEER_HOST} did not resolve to any address")));
    }
    Ok(addresses)
}

/// Non-async variant for JNI calls that cannot use a Tokio runtime.
pub fn resolve_addresses_blocking() -> Result<Vec<IpAddr>, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error(format!("cannot start resolver runtime: {error}")))?;
    runtime.block_on(resolve_addresses())
}

/// Probes every address concurrently and returns the lowest-RTT one.
/// Suitable for running in the background after the VPN is already up.
pub async fn select_fastest(ips: &[IpAddr]) -> Option<IpAddr> {
    if ips.is_empty() {
        return None;
    }

    let mut probes = JoinSet::new();
    for ip in ips.iter().copied() {
        probes.spawn_blocking(move || (ip, probe_rtt(ip)));
    }

    let mut results = Vec::new();
    while let Some(result) = probes.join_next().await {
        match result {
            Ok(measured) => results.push(measured),
            Err(error) => log::warn!("peer probe task failed: {error}"),
        }
    }
    results.sort_by(compare_probes);

    let selected = results.first().map(|(ip, _)| *ip)?;
    match results.first().and_then(|(_, rtt)| *rtt) {
        Some(rtt) => log::info!("selected Yggdrasil peer {selected} with RTT {rtt:?}"),
        None => log::warn!("all peer probes failed; using first resolved address {selected}"),
    }
    Some(selected)
}

/// Resolves peers and selects the fastest address. Used where no tunnel needs
/// to be established first (Linux startup and resolver tests).
pub async fn resolve_peers() -> Result<PeerSet, Error> {
    let dns_servers = resolve_addresses().await?;
    let selected = select_fastest(&dns_servers).await.unwrap_or(dns_servers[0]);
    Ok(PeerSet {
        uris: vec![tls_uri(selected)],
        dns_servers,
    })
}

/// Resolves peers from a plain (non-async) context, such as a JNI call.
pub fn resolve_peers_blocking() -> Result<PeerSet, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error(format!("cannot start resolver runtime: {error}")))?;
    runtime.block_on(resolve_peers())
}

pub(crate) fn tls_uri(ip: IpAddr) -> String {
    format!(
        "tls://{}",
        yggdrasil::core::uri::join_host_port(&ip.to_string(), PEER_PORT)
    )
}

fn compare_probes(a: &(IpAddr, Option<Duration>), b: &(IpAddr, Option<Duration>)) -> Ordering {
    match (a.1, b.1) {
        (Some(left), Some(right)) => left.cmp(&right).then_with(|| a.0.cmp(&b.0)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    }
}

fn probe_rtt(ip: IpAddr) -> Option<Duration> {
    icmp_rtt(ip).or_else(|| tcp_rtt(ip))
}

fn icmp_rtt(ip: IpAddr) -> Option<Duration> {
    let (domain, protocol, request_type, reply_type) = match ip {
        IpAddr::V4(_) => (Domain::IPV4, Protocol::ICMPV4, 8u8, 0u8),
        IpAddr::V6(_) => (Domain::IPV6, Protocol::ICMPV6, 128u8, 129u8),
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(protocol)).ok()?;
    socket.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    socket.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;

    let id = echo_id();
    let sequence = 1u16;
    let mut packet = [0u8; 8 + ECHO_PAYLOAD.len()];
    packet[0] = request_type;
    packet[4..6].copy_from_slice(&id.to_be_bytes());
    packet[6..8].copy_from_slice(&sequence.to_be_bytes());
    packet[8..].copy_from_slice(ECHO_PAYLOAD);
    if ip.is_ipv4() {
        let sum = checksum(&packet);
        packet[2..4].copy_from_slice(&sum.to_be_bytes());
    }

    let address = SockAddr::from(SocketAddr::new(ip, 0));
    let started = Instant::now();
    socket.send_to(&packet, &address).ok()?;

    let mut buffer = [const { MaybeUninit::<u8>::uninit() }; 256];
    loop {
        let (received, _) = socket.recv_from(&mut buffer).ok()?;
        if received < 8 {
            continue;
        }
        let reply = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), received) };
        if reply[0] != reply_type
            || reply[4..6] != id.to_be_bytes()[..]
            || reply[6..8] != sequence.to_be_bytes()[..]
        {
            continue;
        }
        return Some(started.elapsed());
    }
}

fn tcp_rtt(ip: IpAddr) -> Option<Duration> {
    let address = SocketAddr::new(ip, PEER_PORT);
    let started = Instant::now();
    let stream = std::net::TcpStream::connect_timeout(&address, PROBE_TIMEOUT).ok()?;
    let rtt = started.elapsed();
    drop(stream);
    Some(rtt)
}

fn echo_id() -> u16 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (std::process::id() as u16) ^ (nanos as u16)
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut pairs = bytes.chunks_exact(2);
    for pair in pairs.by_ref() {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let Some(last) = pairs.remainder().first() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
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
        assert_eq!(set.uris.len(), 1);
        assert!(!set.dns_servers.is_empty());
        assert!(set.uris[0].starts_with("tls://"));
    }
}
