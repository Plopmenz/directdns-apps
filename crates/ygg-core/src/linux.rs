//! Linux split-tunnel routing and DNS configuration.
//!
//! Only `200::/7` is routed through the TUN interface. DNS is set on that same
//! interface for all domains (`~.`), so every lookup goes to the resolved peer
//! IPs over the normal network path.

use std::net::IpAddr;
use std::process::Command;

use crate::Error;

pub const YGGDRASIL_ROUTE: &str = "200::/7";

/// Reverts routing and DNS changes when dropped.
pub(crate) struct NetworkGuard {
    ifname: String,
}

impl NetworkGuard {
    /// Applies the split route and global DNS override, returning a guard that
    /// restores the previous configuration on drop.
    pub(crate) async fn setup(ifname: &str, dns_servers: &[IpAddr]) -> Result<Self, Error> {
        if dns_servers.is_empty() {
            return Err(Error("no DNS servers to configure".into()));
        }
        if ifname.is_empty() {
            return Err(Error("TUN interface has no name".into()));
        }

        if let Err(error) = Self::apply(ifname, dns_servers).await {
            Self::revert(ifname);
            return Err(error);
        }

        Ok(Self {
            ifname: ifname.to_string(),
        })
    }

    async fn apply(ifname: &str, dns_servers: &[IpAddr]) -> Result<(), Error> {
        run(
            "ip",
            &["-6", "route", "replace", YGGDRASIL_ROUTE, "dev", ifname],
        )
        .await?;

        let mut dns_args = vec!["dns".to_string(), ifname.to_string()];
        dns_args.extend(dns_servers.iter().map(|ip| ip.to_string()));
        let dns_refs: Vec<&str> = dns_args.iter().map(String::as_str).collect();
        run("resolvectl", &dns_refs).await?;
        run("resolvectl", &["domain", ifname, "~."]).await?;
        Ok(())
    }

    fn revert(ifname: &str) {
        let _ = Command::new("resolvectl").args(["revert", ifname]).status();
        let _ = Command::new("ip")
            .args(["-6", "route", "del", YGGDRASIL_ROUTE, "dev", ifname])
            .status();
    }
}

impl Drop for NetworkGuard {
    fn drop(&mut self) {
        Self::revert(&self.ifname);
    }
}

async fn run(program: &str, args: &[&str]) -> Result<(), Error> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|error| {
            Error(format!(
                "cannot run {program}: {error} (is {program} installed?)"
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error(format!(
            "{program} {} failed: {}",
            args.join(" "),
            if stderr.is_empty() {
                output.status.to_string()
            } else {
                stderr
            }
        )));
    }
    Ok(())
}
