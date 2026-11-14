//! `POST /api/checkout`: open a Stripe Checkout Session and hand back its URL.

use axum::Json;
use axum::extract::State;

use super::extract::ClientIp;
use crate::audit::AuditEntry;
use crate::dto::CheckoutResponse;
use crate::{ApiError, AppState};

/// Public on purpose (anyone may buy), so it sits behind the strict rate limit in `guard`.
/// The request has no body: what is sold and for how much is decided in `payments`.
pub async fn create(State(state): State<AppState>, ClientIp(ip): ClientIp) -> Result<Json<CheckoutResponse>, ApiError> {
    let stripe =
        state.payments.as_ref().ok_or_else(|| ApiError::PaymentsUnavailable("STRIPE_API_KEY is not set".into()))?;
    let session = stripe
        .create_checkout(&state.config.public_web_url)
        .await
        .map_err(|error| ApiError::PaymentsUnavailable(error.to_string()))?;
    state
        .audit
        .record(AuditEntry::new("checkout_started").client_ip(ip).detail(format!("Stripe session {}", session.id)))
        .await;
    Ok(Json(CheckoutResponse { url: session.url }))
}
