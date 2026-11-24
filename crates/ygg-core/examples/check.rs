//! Connectivity smoke test without a TUN interface (no root required).
//!
//! Run with: cargo run -p ygg-core --example check

use std::thread::sleep;
use std::time::Duration;

use ygg_core::{InterfaceSource, Session, generate_seed};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seed = generate_seed()?;
    let mut session = Session::start(seed, InterfaceSource::Name("none".into()))?;
    println!("address: ip:{}", session.address());

    for _ in 0..12 {
        sleep(Duration::from_secs(1));
        let status = session.status();
        println!(
            "{} | peers {}/{} | dns {:?}",
            status.message, status.peers_up, status.peers_total, status.dns_servers
        );
        if status.peers_up > 0 {
            break;
        }
    }
    session.stop()?;
    Ok(())
}
