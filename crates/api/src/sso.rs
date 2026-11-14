//! Single sign-on with an OAuth 2 provider (GitHub, Google): authorization code flow with PKCE.
//!
//! Nothing here trusts the browser:
//! - `state` is random, stored hashed in `sso_states`, valid for ten minutes and **used once**;
//!   a second callback with the same state finds nothing.
//! - A cookie set when the flow starts must come back with the callback (see `http::sso`), so
//!   a callback link an attacker made cannot sign a victim in to the attacker's account.
//! - The PKCE verifier never leaves this server until the code is exchanged, so a stolen
//!   authorization code is useless without it.
//! - Who the person is comes from the provider's userinfo endpoint, called here with the access
//!   token we just received over TLS, never from anything the browser sends.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use reqwest::{Client, Url, header};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::config::SsoConfig;
use crate::error::ApiError;
use crate::token;

/// How long a started sign-in may take.
pub const STATE_MINUTES: i64 = 10;
const MAX_NAME_CHARS: usize = 40;

#[derive(Debug, thiserror::Error)]
pub enum SsoError {
    /// Unknown, expired or already used `state`.
    #[error("unknown, expired or reused state")]
    BadState,
    #[error("the provider refused: {0}")]
    Provider(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Who the provider says the person is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub subject: String,
    pub name: String,
}

/// Where to send the browser, and the state value to remember in the cookie.
#[derive(Debug, Clone)]
pub struct Begin {
    pub redirect_to: String,
    pub state: String,
}

pub struct Sso {
    config: SsoConfig,
    http: Client,
    pool: SqlitePool,
    redirect_uri: String,
}

/// S256 code challenge of a PKCE verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn internal(error: impl std::fmt::Display) -> SsoError {
    SsoError::Internal(error.to_string())
}

impl Sso {
    pub fn new(config: SsoConfig, pool: SqlitePool, public_web_url: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // The client secret and the access token travel in requests: never follow a redirect.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the TLS backend initialises");
        Self { redirect_uri: format!("{public_web_url}/api/auth/sso/callback"), config, http, pool }
    }

    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The address to register with the provider as the callback.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Starts a sign-in: remembers a fresh state and PKCE verifier, returns the provider's URL.
    pub async fn begin(&self) -> Result<Begin, SsoError> {
        let state = token::generate().map_err(internal)?;
        let verifier = token::generate().map_err(internal)?;
        let now = Utc::now();
        // Sign-ins that were never finished are useless; forget them.
        let _ = sqlx::query("DELETE FROM sso_states WHERE expires_at <= ?").bind(now).execute(&self.pool).await;
        sqlx::query("INSERT INTO sso_states (state_hash, verifier, nonce, expires_at) VALUES (?, ?, '', ?)")
            .bind(token::hash(&state))
            .bind(&verifier)
            .bind(now + chrono::Duration::minutes(STATE_MINUTES))
            .execute(&self.pool)
            .await
            .map_err(internal)?;

        let url = Url::parse_with_params(
            &self.config.auth_url,
            &[
                ("response_type", "code"),
                ("client_id", self.config.client_id.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("scope", self.config.scope.as_str()),
                ("state", state.as_str()),
                ("code_challenge", pkce_challenge(&verifier).as_str()),
                ("code_challenge_method", "S256"),
            ],
        )
        .map_err(internal)?;
        Ok(Begin { redirect_to: url.into(), state })
    }

    /// Finishes a sign-in: checks and burns the state, trades the code for an access token,
    /// and asks the provider who the person is.
    pub async fn finish(&self, code: &str, state: &str) -> Result<Identity, SsoError> {
        // Taking the row and deleting it is one statement: a state can only be used once.
        let row: Option<(String,)> =
            sqlx::query_as("DELETE FROM sso_states WHERE state_hash = ? AND expires_at > ? RETURNING verifier")
                .bind(token::hash(state))
                .bind(Utc::now())
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let (verifier,) = row.ok_or(SsoError::BadState)?;

        let form = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("code_verifier", verifier.as_str()),
        ];
        let response = self
            .http
            .post(&self.config.token_url)
            // GitHub answers with a query string unless asked for JSON.
            .header(header::ACCEPT, "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|error| SsoError::Provider(error.without_url().to_string()))?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|error| SsoError::Provider(error.without_url().to_string()))?;
        let access_token = body.get("access_token").and_then(Value::as_str).filter(|_| status.is_success());
        let Some(access_token) = access_token else {
            let reason = body.get("error").and_then(Value::as_str).unwrap_or("no access token");
            return Err(SsoError::Provider(format!("token endpoint answered {status}: {reason}")));
        };

        let response = self
            .http
            .get(&self.config.userinfo_url)
            .bearer_auth(access_token)
            .header(header::ACCEPT, "application/json")
            // GitHub's API refuses requests without one.
            .header(header::USER_AGENT, "nullbox")
            .send()
            .await
            .map_err(|error| SsoError::Provider(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(SsoError::Provider(format!("userinfo answered {}", response.status())));
        }
        let info: Value = response.json().await.map_err(|error| SsoError::Provider(error.without_url().to_string()))?;
        identity_from(&info, &self.config)
    }
}

/// Reads the person's id and display name out of a userinfo answer.
fn identity_from(info: &Value, config: &SsoConfig) -> Result<Identity, SsoError> {
    let subject = match info.get(&config.subject_field) {
        Some(Value::String(text)) if !text.is_empty() => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return Err(SsoError::Provider(format!("userinfo has no {}", config.subject_field))),
    };
    let name = config
        .name_fields
        .iter()
        .filter_map(|field| info.get(field).and_then(Value::as_str))
        .map(clean_name)
        .find(|name| !name.is_empty())
        .unwrap_or_default();
    Ok(Identity { subject, name })
}

/// Characters that are not control characters but still change how text looks: zero-width
/// ones and the ones that reverse the writing direction (a name that displays backwards can
/// pass for somebody else's).
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// A name from outside is shown next to replies: no control or invisible characters, no runs
/// of spaces.
fn clean_name(raw: &str) -> String {
    let printable: String = raw.chars().filter(|c| !c.is_control() && !is_invisible(*c)).collect();
    printable.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_NAME_CHARS).collect()
}

impl From<SsoError> for ApiError {
    fn from(error: SsoError) -> Self {
        ApiError::WorksOnMyMachine(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_pkce_challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    fn github() -> SsoConfig {
        let mut config = crate::config::Config::from_lookup(|name| {
            let value = match name {
                "SSO_PROVIDER" => "github",
                "SSO_CLIENT_ID" => "id",
                "SSO_CLIENT_SECRET" => "secret",
                _ => return None,
            };
            Some(value.to_owned())
        })
        .unwrap();
        config.sso.take().unwrap()
    }

    #[test]
    fn identities_come_from_the_configured_fields() {
        let config = github();
        // GitHub: a numeric id, and a name that may be missing.
        let named =
            identity_from(&json!({ "id": 4711, "login": "hanako-gh", "name": "Hanako  Tanaka" }), &config).unwrap();
        assert_eq!(named, Identity { subject: "4711".into(), name: "Hanako Tanaka".into() });
        let unnamed = identity_from(&json!({ "id": 9, "login": "octocat", "name": null }), &config).unwrap();
        assert_eq!(unnamed.name, "octocat");
        // No id: nobody to sign in.
        assert!(identity_from(&json!({ "login": "octocat" }), &config).is_err());
        assert!(identity_from(&json!({ "id": "", "login": "octocat" }), &config).is_err());
    }

    #[test]
    fn names_from_outside_are_made_safe_to_show() {
        assert_eq!(clean_name("  Hanako\u{0}\n\t Tanaka\u{202e} "), "Hanako Tanaka");
        assert_eq!(clean_name(&"あ".repeat(100)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_name("\u{0}\n"), "");
    }
}
