//! Request extractors. `Actor` is the auth seam.

use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, OriginalUri, Path, Query};
use axum::http::header;
use axum::http::request::Parts;

use super::guard::client_ip;
use crate::audit::AuditEntry;
use crate::{ApiError, AppState, token};

pub const ADMIN_TOKEN_HEADER: &str = "x-admin-token";
pub const TICKET_TOKEN_HEADER: &str = "x-ticket-token";

/// What an audit entry says about a refused request: the route, the reason, who asked
/// and how. Credentials are only ever described (a fingerprint and a length), never kept.
pub(super) fn refusal_detail(parts: &Parts, reason: &str) -> String {
    // Inside the `/api` nest the URI has lost its prefix; log the path the client asked for.
    let path = parts.extensions.get::<OriginalUri>().map_or(parts.uri.path(), |original| original.path());
    let header = |name| parts.headers.get(name).and_then(|value| value.to_str().ok());
    let mut detail = format!("{} {path}: {reason}", parts.method);
    if let Some(credential) = header(header::AUTHORIZATION) {
        let credential = credential.strip_prefix("Bearer ").unwrap_or(credential);
        detail.push_str(&format!(
            "; credential sha256:{} ({} chars)",
            token::fingerprint(credential),
            credential.chars().count()
        ));
    }
    if let Some(agent) = header(header::USER_AGENT) {
        detail.push_str(&format!("; user-agent: {}", agent.chars().take(120).collect::<String>()));
    }
    detail
}

/// The worker making the request: the account behind the session token in
/// `Authorization: Bearer <token>`. Anything else (no header, an unknown or expired token,
/// the retired `X-Actor-Name` header) is a 401, and is written to the audit log.
///
/// This is the auth seam: SSO and password sign-in both end in a session token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub name: String,
}

/// `Authorization: Bearer <token>`, without checking the token.
pub struct BearerToken(pub String);

pub(super) fn bearer_token(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()).then(|| token.trim().to_owned())
}

impl FromRequestParts<AppState> for BearerToken {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, Self::Rejection> {
        bearer_token(parts).map(BearerToken).ok_or(ApiError::MissingActor)
    }
}

impl FromRequestParts<AppState> for Actor {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let reason = match bearer_token(parts) {
            None => "no session token",
            Some(session_token) => match state.accounts.name_for_token(&session_token).await {
                Ok(Some(name)) => return Ok(Actor { name }),
                Ok(None) => "unknown or expired session token",
                Err(error) => return Err(ApiError::WorksOnMyMachine(error.to_string())),
            },
        };
        let ip = client_ip(&parts.headers, &parts.extensions, state.config.trust_proxy);
        let entry = AuditEntry::new("worker_access_denied").client_ip(ip).detail(refusal_detail(parts, reason));
        state.audit.record(entry).await;
        Err(ApiError::MissingActor)
    }
}

/// Proof that the request carries the admin token (`X-Admin-Token`). Admin handlers
/// take this as an argument, so a new admin route can't forget the check.
///
/// With no `ADMIN_TOKEN` configured nobody gets in. Every refusal is written to the
/// audit log, with the caller's address but never the token that was tried.
#[derive(Debug, Clone, Copy)]
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let presented = parts.headers.get(ADMIN_TOKEN_HEADER).and_then(|value| value.to_str().ok());
        let granted = match (presented, state.config.admin_token.as_deref()) {
            (Some(presented), Some(expected)) => token::secrets_match(presented, expected),
            _ => false,
        };
        if granted {
            return Ok(Admin);
        }
        let reason = match (presented, &state.config.admin_token) {
            (_, None) => "no admin token is configured",
            (None, _) => "no token presented",
            _ => "wrong token",
        };
        let ip = client_ip(&parts.headers, &parts.extensions, state.config.trust_proxy);
        let entry = AuditEntry::new("admin_access_denied").client_ip(ip).detail(refusal_detail(parts, reason));
        state.audit.record(entry).await;
        Err(ApiError::Forbidden)
    }
}

/// The address the request came from, for audit entries. See [`client_ip`].
pub struct ClientIp(pub std::net::IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(client_ip(&parts.headers, &parts.extensions, state.config.trust_proxy)))
    }
}

/// The customer's secret, from `X-Ticket-Token`. The frontend reads it from the URL fragment.
pub struct TicketToken(pub String);

impl<S: Send + Sync> FromRequestParts<S> for TicketToken {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(TICKET_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(|v| TicketToken(v.to_owned()))
            .ok_or(ApiError::MissingTicketToken)
    }
}

/// `Json`, `Path` and `Query` with rejections in our `{ code, message }` shape.
#[derive(FromRequest)]
#[from_request(via(Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(Path), rejection(ApiError))]
pub struct ApiPath<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(Query), rejection(ApiError))]
pub struct ApiQuery<T>(pub T);
