// SPDX-License-Identifier: AGPL-3.0-only
//! TOTP (RFC 6238) and recovery codes.

use totp_rs::{Algorithm, TOTP};

pub const ISSUER: &str = "OpnRelay";
const STEP: u64 = 30;

pub fn new_secret() -> Vec<u8> {
    opnrelay_crypto::random_bytes::<20>().to_vec()
}

pub fn totp(secret: Vec<u8>, account: &str) -> anyhow::Result<TOTP> {
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        STEP,
        secret,
        Some(ISSUER.to_string()),
        account.replace(':', "_"),
    )
    .map_err(|e| anyhow::anyhow!("totp: {e}"))
}

/// Check `code` at `now` (unix seconds) allowing ±1 step of clock skew.
/// Returns the matched time step, which must be strictly greater than
/// `last_used_step` (prevents replay of a code within its validity window).
pub fn verify(t: &TOTP, code: &str, now: u64, last_used_step: Option<i64>) -> Option<i64> {
    let code = code.trim().replace(' ', "");
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let current = (now / STEP) as i64;
    for step in [current - 1, current, current + 1] {
        if step < 0 || last_used_step.is_some_and(|last| step <= last) {
            continue;
        }
        let candidate = t.generate(step as u64 * STEP);
        if opnrelay_crypto::ct_eq(candidate.as_bytes(), code.as_bytes()) {
            return Some(step);
        }
    }
    None
}

const CODE_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// Generate `n` human-friendly recovery codes like `k7m2-x9qa-4tz8`.
pub fn recovery_codes(n: usize) -> Vec<String> {
    (0..n)
        .map(|_| {
            let bytes = opnrelay_crypto::random_bytes::<12>();
            let chars: String = bytes
                .iter()
                .map(|b| CODE_ALPHABET[(*b as usize) % CODE_ALPHABET.len()] as char)
                .collect();
            format!("{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12])
        })
        .collect()
}

pub fn normalize_recovery(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

pub fn hash_recovery(code: &str) -> Vec<u8> {
    opnrelay_crypto::sha256(format!("opnrelay-recovery:{}", normalize_recovery(code)).as_bytes())
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_verify_with_skew_and_replay() {
        let t = totp(new_secret(), "a@b.c").unwrap();
        let now = 1_700_000_000u64;
        let code = t.generate(now);
        let step = verify(&t, &code, now, None).expect("valid");
        assert_eq!(step, (now / 30) as i64);
        assert!(
            verify(&t, &code, now, Some(step)).is_none(),
            "replay rejected"
        );
        assert!(
            verify(&t, &code, now + 30, None).is_some(),
            "skew tolerated"
        );
        assert!(verify(&t, &code, now + 120, None).is_none());
        assert!(verify(&t, "abc", now, None).is_none());
    }

    #[test]
    fn recovery_codes_format_and_hash() {
        let codes = recovery_codes(10);
        assert_eq!(codes.len(), 10);
        assert_eq!(codes[0].len(), 14);
        assert_eq!(
            hash_recovery(&codes[0]),
            hash_recovery(&codes[0].to_uppercase())
        );
        assert_ne!(hash_recovery(&codes[0]), hash_recovery(&codes[1]));
    }
}
