//! Accounts and sessions: sign up, log in, log out, who am I.
//!
//! Sign-up and log-in sit behind their own rate limit in `guard`, which is what makes
//! guessing passwords impractical.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::extract::{Actor, ApiJson, BearerToken, ClientIp};
use crate::audit::AuditEntry;
use crate::dto::{AuthConfig, AuthResponse, LoginRequest, Me, SignupRequest, SsoProvider};
use crate::{ApiError, AppState, token};

pub async fn config(State(state): State<AppState>) -> Json<AuthConfig> {
    Json(AuthConfig { invite_required: state.config.signup_invite_code.is_some(), sso: sso_provider(&state) })
}

/// The identity provider to offer, when one is set up.
fn sso_provider(_state: &AppState) -> Option<SsoProvider> {
    None
}

fn respond(session: crate::accounts::Session) -> Json<AuthResponse> {
    Json(AuthResponse { token: session.token, name: session.name, expires_at: session.expires_at })
}

pub async fn signup(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(req): ApiJson<SignupRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    if let Some(expected) = &state.config.signup_invite_code {
        let presented = req.invite_code.as_deref().unwrap_or_default();
        if !token::secrets_match(presented, expected) {
            state
                .audit
                .record(AuditEntry::new("signup_refused").client_ip(ip).detail("wrong or missing invite code"))
                .await;
            return Err(ApiError::Forbidden);
        }
    }
    let session = state.accounts.sign_up(&req.username, &req.password).await?;
    state.audit.record(AuditEntry::new("account_created").actor(session.name.clone()).client_ip(ip)).await;
    Ok((StatusCode::CREATED, respond(session)))
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(req): ApiJson<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    match state.accounts.log_in(&req.username, &req.password).await {
        Ok(session) => {
            state.audit.record(AuditEntry::new("login_succeeded").actor(session.name.clone()).client_ip(ip)).await;
            Ok(respond(session))
        }
        Err(error) => {
            if matches!(error, ApiError::InvalidCredentials) {
                // The username is recorded so an attack on one account stands out; the password never is.
                let attempted: String = req.username.trim().chars().take(64).collect();
                state
                    .audit
                    .record(AuditEntry::new("login_failed").client_ip(ip).detail(format!("username: {attempted}")))
                    .await;
            }
            Err(error)
        }
    }
}

pub async fn logout(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    actor: Actor,
    BearerToken(session_token): BearerToken,
) -> Result<StatusCode, ApiError> {
    state.accounts.log_out(&session_token).await.map_err(|error| ApiError::WorksOnMyMachine(error.to_string()))?;
    state.audit.record(AuditEntry::new("logout").actor(actor.name).client_ip(ip)).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(actor: Actor) -> Json<Me> {
    Json(Me { name: actor.name })
}
