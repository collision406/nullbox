//! Secret access tokens for customers: the only "auth" in the MVP.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};

/// 32 random bytes from the OS RNG, base64url-encoded. Never stored.
pub fn generate() -> Result<String, rand::rngs::SysError> {
    let mut bytes = [0u8; 32];
    SysRng.try_fill_bytes(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// SHA-256 (hex) of a token. This is what the database stores and looks up.
pub fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two secrets without leaking, through response time, how many leading
/// characters match. Both sides are hashed first, so the lengths don't leak either.
pub fn secrets_match(candidate: &str, expected: &str) -> bool {
    let (candidate, expected) = (Sha256::digest(candidate.as_bytes()), Sha256::digest(expected.as_bytes()));
    // No early exit: every byte is always compared.
    candidate.iter().zip(expected.iter()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_match_only_when_identical() {
        assert!(secrets_match("correct horse battery staple", "correct horse battery staple"));
        assert!(!secrets_match("correct horse battery stapl", "correct horse battery staple"));
        assert!(!secrets_match("Correct horse battery staple", "correct horse battery staple"));
        assert!(!secrets_match("", "correct horse battery staple"));
        // An empty expected secret must not turn an empty header into a password.
        assert!(secrets_match("", ""));
    }

    #[test]
    fn tokens_are_unique_and_stored_hashed() {
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43); // 32 bytes, base64url without padding
        assert_eq!(hash(&a).len(), 64);
        assert_ne!(hash(&a), a);
    }
}
