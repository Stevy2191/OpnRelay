// SPDX-License-Identifier: AGPL-3.0-only
//! Cryptographic helpers for OpnRelay.
//!
//! * Envelope encryption: every object gets a random 256-bit data key (DEK),
//!   encrypted with AES-256-GCM. The DEK is wrapped with the master key (KEK).
//! * Opaque tokens: random 256-bit values; only SHA-256 hashes are stored.
//! * Password hashing: Argon2id.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid key material")]
    InvalidKey,
    #[error("ciphertext is malformed or was tampered with")]
    Decrypt,
    #[error("encryption failed")]
    Encrypt,
    #[error("password hashing failed")]
    PasswordHash,
}

/// A 256-bit symmetric key that is wiped from memory on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; KEY_LEN]);

impl SecretKey {
    pub fn generate() -> Self {
        let mut k = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut k);
        Self(k)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let arr: [u8; KEY_LEN] = bytes.try_into().map_err(|_| CryptoError::InvalidKey)?;
        Ok(Self(arr))
    }

    /// Parse a key from text: 64 hex chars or URL-safe base64 (no padding).
    pub fn from_text(text: &str) -> Result<Self, CryptoError> {
        let t = text.trim();
        if t.len() == KEY_LEN * 2
            && let Ok(b) = hex::decode(t)
        {
            return Self::from_bytes(&b);
        }
        let b = URL_SAFE_NO_PAD
            .decode(t.trim_end_matches('='))
            .map_err(|_| CryptoError::InvalidKey)?;
        Self::from_bytes(&b)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// A short, non-secret identifier for this key (used to track which KEK
    /// wrapped a DEK, for rotation).
    pub fn key_id(&self) -> String {
        let digest = Sha256::digest([b"opnrelay-kid:".as_slice(), &self.0].concat());
        hex::encode(&digest[..8])
    }

    /// Derive a purpose-specific sub key (HKDF-like construction using
    /// SHA-256 with domain separation).
    pub fn derive(&self, purpose: &str) -> SecretKey {
        let mut h = Sha256::new();
        h.update(b"opnrelay-derive-v1:");
        h.update(purpose.as_bytes());
        h.update([0u8]);
        h.update(self.0);
        let out = h.finalize();
        let mut k = [0u8; KEY_LEN];
        k.copy_from_slice(&out);
        SecretKey(k)
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretKey({})", self.key_id())
    }
}

/// AES-256-GCM encrypt. Output layout: `nonce(12) || ciphertext || tag(16)`.
/// `aad` binds the ciphertext to context (e.g. the object id).
pub fn seal(key: &SecretKey, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| CryptoError::InvalidKey)?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Inverse of [`seal`].
pub fn open(key: &SecretKey, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() < NONCE_LEN + 16 {
        return Err(CryptoError::Decrypt);
    }
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|_| CryptoError::InvalidKey)?;
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| CryptoError::Decrypt)
}

/// Result of envelope-encrypting an object.
pub struct Envelope {
    /// DEK wrapped by the KEK.
    pub wrapped_dek: Vec<u8>,
    /// Object ciphertext.
    pub ciphertext: Vec<u8>,
}

/// Encrypt `plaintext` with a fresh DEK and wrap the DEK with `kek`.
pub fn envelope_seal(
    kek: &SecretKey,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Envelope, CryptoError> {
    let dek = SecretKey::generate();
    let ciphertext = seal(&dek, plaintext, aad)?;
    let wrapped_dek = seal(kek, dek.as_bytes(), aad)?;
    Ok(Envelope {
        wrapped_dek,
        ciphertext,
    })
}

/// Decrypt an envelope produced by [`envelope_seal`].
pub fn envelope_open(
    kek: &SecretKey,
    wrapped_dek: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut dek_bytes = open(kek, wrapped_dek, aad)?;
    let dek = SecretKey::from_bytes(&dek_bytes);
    dek_bytes.zeroize();
    open(&dek?, ciphertext, aad)
}

/// Re-wrap a DEK under a new KEK (key rotation; object data is untouched).
pub fn rewrap(
    old: &SecretKey,
    new: &SecretKey,
    wrapped_dek: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut dek = open(old, wrapped_dek, aad)?;
    let out = seal(new, &dek, aad);
    dek.zeroize();
    out
}

/// Generate an opaque, URL-safe random token (256 bits).
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

/// Random bytes helper.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// SHA-256 of a token, for storage/lookup. Tokens are high-entropy so a fast
/// hash is appropriate.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Constant-time byte comparison.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub mod password {
    //! Argon2id password hashing.
    use super::CryptoError;
    use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
    use argon2::{Algorithm, Argon2, Params, Version};

    pub const MIN_LEN: usize = 12;
    pub const MAX_LEN: usize = 256;

    fn hasher() -> Argon2<'static> {
        // m = 64 MiB, t = 3, p = 1
        let params = Params::new(64 * 1024, 3, 1, None).expect("static argon2 params are valid");
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
    }

    pub fn hash(password: &str) -> Result<String, CryptoError> {
        let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
        hasher()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|_| CryptoError::PasswordHash)
    }

    /// Verify a password against a PHC string. Returns false on mismatch or
    /// malformed hash.
    pub fn verify(password: &str, phc: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(phc) else {
            return false;
        };
        hasher()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    /// A hash of a random password, used to equalise timing when a user does
    /// not exist.
    pub fn dummy_hash() -> &'static str {
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        DUMMY.get_or_init(|| hash(&super::random_token()).expect("hashing works"))
    }

    /// Basic policy: length only (NIST SP 800-63B style, no composition rules).
    pub fn check_policy(password: &str) -> Result<(), &'static str> {
        let n = password.chars().count();
        if n < MIN_LEN {
            return Err("password must be at least 12 characters");
        }
        if n > MAX_LEN {
            return Err("password is too long");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_tamper() {
        let k = SecretKey::generate();
        let ct = seal(&k, b"hello", b"id-1").unwrap();
        assert_eq!(open(&k, &ct, b"id-1").unwrap(), b"hello");
        assert!(open(&k, &ct, b"id-2").is_err(), "aad must bind");
        let mut bad = ct.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(open(&k, &bad, b"id-1").is_err());
        assert!(open(&k, &ct[..5], b"id-1").is_err());
    }

    #[test]
    fn envelope_and_rotation() {
        let kek = SecretKey::generate();
        let env = envelope_seal(&kek, b"screenshot bytes", b"m1").unwrap();
        assert_eq!(
            envelope_open(&kek, &env.wrapped_dek, &env.ciphertext, b"m1").unwrap(),
            b"screenshot bytes"
        );
        let kek2 = SecretKey::generate();
        let rewrapped = rewrap(&kek, &kek2, &env.wrapped_dek, b"m1").unwrap();
        assert_eq!(
            envelope_open(&kek2, &rewrapped, &env.ciphertext, b"m1").unwrap(),
            b"screenshot bytes"
        );
        assert!(envelope_open(&kek, &rewrapped, &env.ciphertext, b"m1").is_err());
    }

    #[test]
    fn key_text_parsing() {
        let k = SecretKey::generate();
        let parsed = SecretKey::from_text(&k.to_hex()).unwrap();
        assert_eq!(parsed.as_bytes(), k.as_bytes());
        let b64 = URL_SAFE_NO_PAD.encode(k.as_bytes());
        assert_eq!(SecretKey::from_text(&b64).unwrap().as_bytes(), k.as_bytes());
        assert!(SecretKey::from_text("short").is_err());
        assert_ne!(k.derive("a").as_bytes(), k.derive("b").as_bytes());
    }

    #[test]
    fn passwords() {
        let h = password::hash("correct horse battery staple").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(password::verify("correct horse battery staple", &h));
        assert!(!password::verify("wrong", &h));
        assert!(!password::verify("x", "not a hash"));
        assert!(password::check_policy("short").is_err());
        assert!(password::check_policy("long enough pass").is_ok());
    }

    #[test]
    fn tokens_are_unique_and_hashed() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(hash_token(&a).len(), 32);
        assert!(ct_eq(&hash_token(&a), &hash_token(&a)));
        assert!(!ct_eq(&hash_token(&a), &hash_token(&b)));
    }
}
