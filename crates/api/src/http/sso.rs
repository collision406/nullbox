//! `GET /api/auth/sso/login` and `/callback`: sign in with an identity provider.
//!
//! The flow ends with the browser sent to `{PUBLIC_WEB_URL}/sso/done#token=<session>`: the
//! token is in the URL fragment, which browsers never send to servers, so it reaches no log.

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;

use super::extract::ClientIp;
use crate::audit::AuditEntry;
use crate::sso::{Sso, SsoError};
use crate::{ApiError, AppState, token};

/// Ties a started sign-in to the browser that started it. HttpOnly, Secure, and only sent to
/// our own `/api/auth/sso` paths. `Lax` still lets it ride the provider's redirect back to us.
pub const COOKIE_NAME: &str = "nullbox_sso";
const COOKIE_PATH: &str = "/api/auth/sso";

fn cookie(value: &str, max_age_secs: u32) -> String {
    format!("{COOKIE_NAME}={value}; Max-Age={max_age_secs}; Path={COOKIE_PATH}; HttpOnly; Secure; SameSite=Lax")
}

fn redirect(location: &str, set_cookie: Option<String>) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    let headers = response.headers_mut();
    headers.insert(header::LOCATION, HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static("/")));
    if let Some(value) = set_cookie.and_then(|value| HeaderValue::from_str(&value).ok()) {
        headers.append(header::SET_COOKIE, value);
    }
    response
}

fn configured(state: &AppState) -> Result<&Sso, ApiError> {
    state.sso.as_deref().ok_or(ApiError::NotFound)
}

fn cookie_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_owned())
}

pub async fn login(State(state): State<AppState>, ClientIp(ip): ClientIp) -> Result<Response, ApiError> {
    let sso = configured(&state)?;
    let begin = sso.begin().await?;
    state
        .audit
        .record(AuditEntry::new("sso_login_started").client_ip(ip).detail(format!("provider {}", sso.provider())))
        .await;
    Ok(redirect(&begin.redirect_to, Some(cookie(&begin.state, 600))))
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    /// Set by the provider when the person said no, or something went wrong on its side.
    error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ApiError> {
    let sso = configured(&state)?;
    let site = &state.config.public_web_url;
    // Whatever happens, the cookie has done its job.
    let clear = Some(cookie("", 0));
    let fail = |code: &str| redirect(&format!("{site}/sso/done#error={code}"), clear.clone());
    let refused = |reason: String| {
        let audit = state.audit.clone();
        async move {
            audit.record(AuditEntry::new("sso_login_failed").client_ip(ip).detail(reason)).await;
        }
    };

    if let Some(error) = &params.error {
        // The provider's own words, shortened: they come from the URL, so they are not trusted.
        let reason: String = error.chars().filter(|c| c.is_ascii_graphic()).take(60).collect();
        refused(format!("provider returned error: {reason}")).await;
        return Ok(fail("denied"));
    }
    let (Some(code), Some(returned_state)) = (params.code.as_deref(), params.state.as_deref()) else {
        refused("callback without code or state".into()).await;
        return Ok(fail("failed"));
    };
    // The browser that finishes the sign-in must be the one that started it.
    let bound = cookie_value(&headers).is_some_and(|value| token::secrets_match(&value, returned_state));
    if !bound {
        refused("state does not match the browser's cookie".into()).await;
        return Ok(fail("expired"));
    }

    let identity = match sso.finish(code, returned_state).await {
        Ok(identity) => identity,
        Err(SsoError::BadState) => {
            refused("unknown, expired or reused state".into()).await;
            return Ok(fail("expired"));
        }
        Err(error) => {
            tracing::warn!(target: "nullbox::security", %error, "single sign-on failed");
            refused(error.to_string()).await;
            return Ok(fail("failed"));
        }
    };
    let session = state.accounts.sign_in_with_sso(sso.provider(), &identity.subject, &identity.name).await?;
    state
        .audit
        .record(
            AuditEntry::new("sso_login_succeeded")
                .actor(session.name.clone())
                .client_ip(ip)
                .detail(format!("provider {}", sso.provider())),
        )
        .await;
    Ok(redirect(&format!("{site}/sso/done#token={}", session.token), clear))
}
