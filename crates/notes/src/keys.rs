//! The issuer's key: kept in a file, made on first use; or the toy one,
//! from a label anyone can derive (tests only).

use anyhow::{Context, Result};
use k256::schnorr::SigningKey;
use sha2::{Digest, Sha256};

/// A key from a label (anyone can derive it: tests only).
pub fn key_from_label(label: &str) -> SigningKey {
    SigningKey::from_bytes(&Sha256::digest(label.as_bytes())).expect("a valid scalar")
}

/// The toy deployment's issuer key: tests.
pub fn regtest_hub() -> SigningKey {
    key_from_label("lngap-toy/hub")
}

/// A fresh random key.
pub fn fresh_key() -> SigningKey {
    loop {
        if let Ok(k) = SigningKey::from_bytes(&lichen_proto::fresh_rho()) {
            return k;
        }
    }
}

/// The key in `path` (hex), made and kept there (the owner's alone) if
/// there is none.
pub fn load_or_create_key(path: &std::path::Path) -> Result<SigningKey> {
    if path.exists() {
        let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        return SigningKey::from_bytes(&hex::decode(s.trim()).context("a hex key")?).context("a valid key");
    }
    let k = fresh_key();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    lichen_proto::files::write_private(path, hex::encode(k.to_bytes()).as_bytes())?;
    Ok(k)
}

/// A key's x-only public key.
pub fn xonly_of(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes().into()
}
