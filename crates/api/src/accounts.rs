//! Worker accounts and sessions.
//!
//! Passwords are hashed with Argon2id and never stored, logged or returned. A session is a
//! random 256-bit token handed out once at sign-in; only its SHA-256 is stored, so a copy of
//! the database can't be used to sign in. Logging out deletes the session on the server.

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::error::ApiError;
use crate::passwords::{MAX_PASSWORD_CHARS, PasswordPolicy};
use crate::token;

pub const SESSION_HOURS: i64 = 12;
const MIN_USERNAME_CHARS: usize = 3;
const MAX_USERNAME_CHARS: usize = 32;

/// A freshly opened session. `token` is shown to the user once and never stored.
#[derive(Debug, Clone)]
pub struct Session {
    pub token: String,
    pub name: String,
    pub expires_at: DateTime<Utc>,
}

pub struct Accounts {
    pool: SqlitePool,
    policy: PasswordPolicy,
    /// Verified against when the username doesn't exist, so an unknown user costs as much
    /// time as a wrong password and the two can't be told apart by timing.
    dummy_hash: String,
}

impl Accounts {
    pub fn new(pool: SqlitePool, hibp_base: &str) -> Self {
        let dummy_hash = hash_now("a password nobody has").expect("Argon2 hashes a short string");
        Self { pool, policy: PasswordPolicy::new(hibp_base), dummy_hash }
    }

    pub async fn sign_up(&self, username: &str, password: &str) -> Result<Session, ApiError> {
        let (name, key) = normalise_username(username)?;
        self.policy.check(&name, password).await?;
        let hash = hash_blocking(password.to_owned()).await?;

        let id = Uuid::now_v7().to_string();
        let inserted = sqlx::query(
            "INSERT INTO users (id, username, username_key, password_hash, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(&key)
        .bind(&hash)
        .bind(Utc::now())
        .execute(&self.pool)
        .await;
        match inserted {
            Ok(_) => self.open_session(&id, &name).await,
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => Err(ApiError::UsernameTaken),
            Err(error) => Err(database(error)),
        }
    }

    /// Wrong password, unknown user and an SSO-only account all answer the same way.
    pub async fn log_in(&self, username: &str, password: &str) -> Result<Session, ApiError> {
        let key = username.trim().to_lowercase();
        let account: Option<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, username, password_hash FROM users WHERE username_key = ?")
                .bind(&key)
                .fetch_optional(&self.pool)
                .await
                .map_err(database)?;
        if password.chars().count() > MAX_PASSWORD_CHARS {
            return Err(ApiError::InvalidCredentials);
        }
        let stored = account.as_ref().and_then(|(_, _, hash)| hash.clone());
        let matches = verify_blocking(password.to_owned(), stored.clone().unwrap_or_else(|| self.dummy_hash.clone()))
            .await
            && stored.is_some();
        match account {
            Some((id, name, _)) if matches => self.open_session(&id, &name).await,
            _ => Err(ApiError::InvalidCredentials),
        }
    }

    /// Finds or creates the account for an identity provider's user, then opens a session.
    pub async fn sign_in_with_sso(&self, issuer: &str, subject: &str, display_name: &str) -> Result<Session, ApiError> {
        let existing: Option<(String, String)> =
            sqlx::query_as("SELECT id, username FROM users WHERE sso_issuer = ? AND sso_subject = ?")
                .bind(issuer)
                .bind(subject)
                .fetch_optional(&self.pool)
                .await
                .map_err(database)?;
        if let Some((id, name)) = existing {
            return self.open_session(&id, &name).await;
        }
        // The provider's name can clash with a local username: make it unique, never merge.
        let base: String = display_name.trim().chars().take(MAX_USERNAME_CHARS - 5).collect();
        let base = if base.is_empty() { "sso-user".to_owned() } else { base };
        let id = Uuid::now_v7().to_string();
        for attempt in 0..5u32 {
            let name = if attempt == 0 { base.clone() } else { format!("{base}-{}", rand::random::<u16>() % 10_000) };
            let inserted = sqlx::query(
                "INSERT INTO users (id, username, username_key, sso_issuer, sso_subject, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&name)
            .bind(name.to_lowercase())
            .bind(issuer)
            .bind(subject)
            .bind(Utc::now())
            .execute(&self.pool)
            .await;
            match inserted {
                Ok(_) => return self.open_session(&id, &name).await,
                Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                    // Either the name is taken (try another) or a parallel callback created the account.
                    let raced: Option<(String, String)> =
                        sqlx::query_as("SELECT id, username FROM users WHERE sso_issuer = ? AND sso_subject = ?")
                            .bind(issuer)
                            .bind(subject)
                            .fetch_optional(&self.pool)
                            .await
                            .map_err(database)?;
                    if let Some((id, name)) = raced {
                        return self.open_session(&id, &name).await;
                    }
                }
                Err(error) => return Err(database(error)),
            }
        }
        Err(ApiError::UsernameTaken)
    }

    /// The account name behind a session token, if the session exists and hasn't expired.
    pub async fn name_for_token(&self, session_token: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT u.username FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(token::hash(session_token))
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(name,)| name))
    }

    /// Ends a session on the server. `false` when there was none.
    pub async fn log_out(&self, session_token: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token::hash(session_token))
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn open_session(&self, user_id: &str, name: &str) -> Result<Session, ApiError> {
        let session_token = token::generate().map_err(|error| ApiError::WorksOnMyMachine(error.to_string()))?;
        let now = Utc::now();
        let expires_at = now + Duration::hours(SESSION_HOURS);
        sqlx::query("INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
            .bind(token::hash(&session_token))
            .bind(user_id)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(database)?;
        // Expired sessions are useless; forget them whenever a new one is opened.
        let _ = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(now).execute(&self.pool).await;
        Ok(Session { token: session_token, name: name.to_owned(), expires_at })
    }
}

fn database(error: sqlx::Error) -> ApiError {
    ApiError::WorksOnMyMachine(error.to_string())
}

/// 3 to 32 letters (any script), digits, `_`, `.` or `-`. Returns the name as typed and
/// its lowercase key: `Yamada` and `yamada` are the same account.
fn normalise_username(raw: &str) -> Result<(String, String), ApiError> {
    let name = raw.trim();
    let length = name.chars().count();
    let allowed = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
    if !(MIN_USERNAME_CHARS..=MAX_USERNAME_CHARS).contains(&length) || !name.chars().all(allowed) {
        return Err(ApiError::Validation(format!(
            "username: {MIN_USERNAME_CHARS} to {MAX_USERNAME_CHARS} letters, digits, '_', '.' or '-'"
        )));
    }
    Ok((name.to_owned(), name.to_lowercase()))
}

fn hash_now(password: &str) -> Result<String, argon2::password_hash::Error> {
    Ok(Argon2::default().hash_password(password.as_bytes())?.to_string())
}

/// Argon2 takes tens of milliseconds on purpose: keep it off the async threads.
async fn hash_blocking(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || hash_now(&password))
        .await
        .map_err(|error| ApiError::WorksOnMyMachine(error.to_string()))?
        .map_err(|error| ApiError::WorksOnMyMachine(error.to_string()))
}

async fn verify_blocking(password: String, stored_hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        Argon2::default().verify_password(password.as_bytes(), stored_hash.as_str()).is_ok()
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "T7#pLm9!vQ2z-correct";

    async fn accounts() -> (Accounts, SqlitePool) {
        let pool = crate::db::connect_in_memory().await.unwrap();
        (Accounts::new(pool.clone(), "http://127.0.0.1:1"), pool)
    }

    #[tokio::test]
    async fn passwords_and_session_tokens_are_stored_hashed() {
        let (accounts, pool) = accounts().await;
        let session = accounts.sign_up("yamada", PASSWORD).await.unwrap();

        let (stored,): (String,) = sqlx::query_as("SELECT password_hash FROM users").fetch_one(&pool).await.unwrap();
        assert!(stored.starts_with("$argon2id$"), "{stored}");
        assert!(!stored.contains(PASSWORD));
        let (token_hash,): (String,) =
            sqlx::query_as("SELECT token_hash FROM sessions").fetch_one(&pool).await.unwrap();
        assert_ne!(token_hash, session.token);
        assert_eq!(token_hash, token::hash(&session.token));
    }

    #[tokio::test]
    async fn two_accounts_with_the_same_password_have_different_hashes() {
        let (accounts, pool) = accounts().await;
        accounts.sign_up("yamada", PASSWORD).await.unwrap();
        accounts.sign_up("tanaka", PASSWORD).await.unwrap();
        let hashes: Vec<(String,)> = sqlx::query_as("SELECT password_hash FROM users").fetch_all(&pool).await.unwrap();
        assert_ne!(hashes[0].0, hashes[1].0, "every hash has its own salt");
    }

    #[tokio::test]
    async fn expired_and_ended_sessions_stop_working() {
        let (accounts, pool) = accounts().await;
        let session = accounts.sign_up("yamada", PASSWORD).await.unwrap();
        assert_eq!(accounts.name_for_token(&session.token).await.unwrap().as_deref(), Some("yamada"));

        // Past its expiry: refused, although the row is still there.
        sqlx::query("UPDATE sessions SET expires_at = ?")
            .bind(Utc::now() - Duration::seconds(1))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(accounts.name_for_token(&session.token).await.unwrap(), None);

        // Logging out deletes the session for good, and only once.
        let again = accounts.log_in("YAMADA", PASSWORD).await.unwrap();
        assert!(accounts.log_out(&again.token).await.unwrap());
        assert_eq!(accounts.name_for_token(&again.token).await.unwrap(), None);
        assert!(!accounts.log_out(&again.token).await.unwrap());
    }

    #[tokio::test]
    async fn usernames_are_unique_whatever_their_case() {
        let (accounts, _) = accounts().await;
        accounts.sign_up("Yamada", PASSWORD).await.unwrap();
        assert!(matches!(accounts.sign_up("yamada", PASSWORD).await, Err(ApiError::UsernameTaken)));
        // Logging in is case-insensitive too, and shows the name as it was typed at sign-up.
        assert_eq!(accounts.log_in(" yAMADA ", PASSWORD).await.unwrap().name, "Yamada");
    }

    #[tokio::test]
    async fn sso_accounts_have_no_password_and_never_merge_with_local_ones() {
        let (accounts, _) = accounts().await;
        accounts.sign_up("Hanako", PASSWORD).await.unwrap();
        let first = accounts.sign_in_with_sso("https://idp.example", "sub-1", "Hanako").await.unwrap();
        // The provider's "Hanako" is a different person from the local "Hanako".
        assert_ne!(first.name, "Hanako");
        // Same provider user again: same account.
        let second = accounts.sign_in_with_sso("https://idp.example", "sub-1", "Hanako").await.unwrap();
        assert_eq!(first.name, second.name);
        // No password can ever open it.
        assert!(matches!(accounts.log_in(&first.name, "").await, Err(ApiError::InvalidCredentials)));
        assert!(matches!(accounts.log_in(&first.name, PASSWORD).await, Err(ApiError::InvalidCredentials)));
    }
}
