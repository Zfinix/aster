//! RFC 7636 PKCE pair, shared by every browser sign-in flow.

use base64::Engine;
use sha2::{Digest, Sha256};

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// 32 OS-random bytes, base64url: 43 chars, inside the 43-128 range.
pub fn random_urlsafe() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS entropy unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A fresh verifier and its S256 challenge.
pub fn pkce() -> Pkce {
    let verifier = random_urlsafe();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}
