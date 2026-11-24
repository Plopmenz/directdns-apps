//! Persistent node seed and derived address helpers.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::Error;

/// A 32-byte Yggdrasil private seed.
pub type Seed = [u8; 32];

/// Default on-disk location for the seed (`$XDG_DATA_HOME/directdns/seed.key` on Linux).
pub fn default_seed_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("directdns").join("seed.key"))
}

/// Generates a new cryptographically random seed.
pub fn generate_seed() -> Result<Seed, Error> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| Error(format!("key generation failed: {error}")))?;
    Ok(seed)
}

/// Reads the 32-byte seed at `path`.
pub fn read_seed(path: impl AsRef<Path>) -> Result<Seed, Error> {
    let mut bytes = Vec::with_capacity(32);
    std::fs::File::open(path)
        .map_err(|error| Error(format!("cannot open seed: {error}")))?
        .take(33)
        .read_to_end(&mut bytes)
        .map_err(|error| Error(format!("cannot read seed: {error}")))?;
    bytes
        .try_into()
        .map_err(|_| Error("seed file must contain exactly 32 bytes".into()))
}

/// Reads the seed if it exists, otherwise generates one and persists it.
///
/// On Unix the file is created with mode `0600`. Parent directories are created
/// as needed.
pub fn load_or_create_seed(path: impl AsRef<Path>) -> Result<Seed, Error> {
    let path = path.as_ref();
    if path
        .try_exists()
        .map_err(|error| Error(format!("cannot stat seed: {error}")))?
    {
        return read_seed(path);
    }

    let seed = generate_seed()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error(format!("cannot create seed directory: {error}")))?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| Error(format!("cannot create seed file: {error}")))?;
    file.write_all(&seed)
        .map_err(|error| Error(format!("cannot write seed: {error}")))?;
    file.flush()
        .map_err(|error| Error(format!("cannot flush seed: {error}")))?;
    Ok(seed)
}

/// Derives the node's Yggdrasil IPv6 address from a seed.
pub fn address_from_seed(seed: &Seed) -> String {
    let secret = yggdrasil::PrivateKey::from_seed(seed);
    yggdrasil::address::addr_for_key(&secret.public_key()).to_string()
}

/// Formats an address for display, as `ip:<addr>`.
pub fn display_address(address: &str) -> String {
    format!("ip:{address}")
}

/// Converts an I/O error into our error type.
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[test]
    fn seed_is_persisted_and_stable() {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "directdns-key-test-{}-{unique}",
            std::process::id()
        ));
        let path = dir.join("seed.key");
        let _ = std::fs::remove_dir_all(&dir);

        let first = load_or_create_seed(&path).expect("seed created");
        let second = load_or_create_seed(&path).expect("seed loaded");
        assert_eq!(first, second);
        assert_eq!(std::fs::read(&path).unwrap().len(), 32);

        let address = address_from_seed(&first);
        assert!(address.starts_with('2'), "unexpected address: {address}");
        assert_eq!(display_address(&address), format!("ip:{address}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
