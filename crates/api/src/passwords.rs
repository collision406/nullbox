//! What a password must satisfy before an account is created with it.
//!
//! Length first, then a list of obvious choices, then Have I Been Pwned's breach corpus.
//! The breach check sends only the first five characters of the password's SHA-1 (the
//! k-anonymity range API): the password, and even its full hash, never leave this server.

use std::time::Duration;

use reqwest::Client;
use sha1::{Digest, Sha1};

pub const MIN_PASSWORD_CHARS: usize = 10;
pub const MAX_PASSWORD_CHARS: usize = 128;
pub const HIBP_API_BASE: &str = "https://api.pwnedpasswords.com";

/// Bases of the passwords everybody tries first. A password is refused when it equals one of
/// these, or one of these followed by digits and punctuation (`password2026!`).
const COMMON_BASES: &[&str] = &[
    "password",
    "passw0rd",
    "p@ssw0rd",
    "passphrase",
    "qwerty",
    "qwertyuiop",
    "qwertyuiopasdfghjkl",
    "azerty",
    "azertyuiop",
    "letmein",
    "welcome",
    "admin",
    "administrator",
    "iloveyou",
    "monkey",
    "dragon",
    "football",
    "baseball",
    "sunshine",
    "princess",
    "superman",
    "batman",
    "trustno",
    "changeme",
    "secret",
    "master",
    "login",
    "abc",
    "abcdefgh",
    "abcdefghij",
    "asdfghjkl",
    "zxcvbnm",
    "starwars",
    "whatever",
    "freedom",
    "shadow",
    "mustang",
    "michael",
    "jennifer",
    "nullbox",
    "support",
    "helpdesk",
    "hackathon",
    "fasthacks",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordProblem {
    TooShort,
    TooLong,
    /// Too predictable: a common password, the username, or hardly any variety.
    TooWeak,
    /// Found in a known data breach.
    Leaked,
}

pub struct PasswordPolicy {
    http: Client,
    hibp_base: String,
}

impl PasswordPolicy {
    pub fn new(hibp_base: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(3))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .expect("the TLS backend initialises");
        Self { http, hibp_base: hibp_base.trim_end_matches('/').to_owned() }
    }

    /// Fails with the first rule the password breaks. When the breach service can't be
    /// reached the check is skipped with a warning (the local rules still applied): signing
    /// up must not depend on a third party being up.
    pub async fn check(&self, username: &str, password: &str) -> Result<(), PasswordProblem> {
        let length = password.chars().count();
        if length < MIN_PASSWORD_CHARS {
            return Err(PasswordProblem::TooShort);
        }
        if length > MAX_PASSWORD_CHARS {
            return Err(PasswordProblem::TooLong);
        }
        if is_weak(username, password) {
            return Err(PasswordProblem::TooWeak);
        }
        match self.pwned_count(password).await {
            Ok(0) => Ok(()),
            Ok(_) => Err(PasswordProblem::Leaked),
            Err(error) => {
                tracing::warn!(target: "nullbox::security", %error, "breach check unavailable, skipped");
                Ok(())
            }
        }
    }

    /// How many times the password appears in known breaches.
    async fn pwned_count(&self, password: &str) -> Result<u64, reqwest::Error> {
        let digest = Sha1::digest(password.as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02X}")).collect();
        let (prefix, suffix) = hex.split_at(5);
        let body = self
            .http
            .get(format!("{}/range/{prefix}", self.hibp_base))
            // Asks for fake entries too, so the size of the answer says nothing.
            .header("Add-Padding", "true")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let count = body
            .lines()
            .filter_map(|line| line.trim().split_once(':'))
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(suffix))
            .map_or(0, |(_, count)| count.trim().parse().unwrap_or(1));
        Ok(count)
    }
}

fn is_weak(username: &str, password: &str) -> bool {
    let lower = password.to_lowercase();
    let user = username.to_lowercase();
    if lower == user || (user.chars().count() >= 4 && lower.contains(&user)) {
        return true;
    }
    let mut distinct: Vec<char> = lower.chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() <= 3 {
        return true;
    }
    let base = lower.trim_end_matches(|c: char| c.is_ascii_digit() || "!@#$%^&*.?_-".contains(c));
    COMMON_BASES.contains(&lower.as_str()) || COMMON_BASES.contains(&base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obvious_passwords_are_weak() {
        for weak in ["password2026!", "Password123456", "qwertyuiop", "aaaaaaaaaaaa", "1212121212", "Nullbox-2026"] {
            assert!(is_weak("yamada", weak), "{weak}");
        }
        // Containing the username is weak, however it is dressed up.
        assert!(is_weak("yamada", "Yamada-is-the-best-2026"));
        assert!(is_weak("kenji", "kenji"));
        for fine in ["correct horse battery staple", "T7#pLm9!vQ2z", "山田さんの秘密のパスワード"] {
            assert!(!is_weak("tanaka", fine), "{fine}");
        }
    }

    #[tokio::test]
    async fn length_is_checked_before_anything_else() {
        let policy = PasswordPolicy::new("http://127.0.0.1:1");
        assert_eq!(policy.check("u", "short").await, Err(PasswordProblem::TooShort));
        assert_eq!(policy.check("u", &"x1".repeat(MAX_PASSWORD_CHARS)).await, Err(PasswordProblem::TooLong));
        // Ten characters is enough; an unreachable breach service doesn't block sign-up.
        assert_eq!(policy.check("u", "T7#pLm9!vQ").await, Ok(()));
    }
}
