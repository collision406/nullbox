use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde_json::{Value, json};

use crate::dto::{AlertRecord, CreateAlertRequest, ObservabilitySummary};
use crate::http::extract::ApiJson;
use crate::{ApiError, AppState, observability};

/// Liveness probe: answers as long as the process is running, independent of storage.
pub async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness probe: verifies the database is reachable (`SELECT 1`).
/// A stopped or unreachable database flips `/api/readyz` to 503 without killing `/api/healthz`.
pub async fn readyz(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    match observability::check_db_readiness(&state.pool).await {
        Ok(latency_us) => {
            state.observability.record_readiness(true, None);
            Ok(Json(json!({
                "status": "ok",
                "db": "ok",
                "latency_us": latency_us,
            })))
        }
        Err(reason) => {
            state.observability.record_readiness(false, Some(&reason));
            Err(ApiError::ProbablyDns(reason))
        }
    }
}

/// Prometheus metrics exposition (`text/plain; version=0.0.4`).
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let db_ready = observability::check_db_readiness(&state.pool).await.is_ok();
    state.observability.record_readiness(db_ready, None);
    let db_size_bytes = observability::query_db_size_bytes(&state.pool, &state.config.database_url).await;
    let open_tickets = state.tickets.backlog_stats().await.map(|s| s.open_count).unwrap_or(0);
    let body = state.observability.render_prometheus(db_ready, db_size_bytes, open_tickets);
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")], body)
}

/// Structured JSON summary for the in-app Observability dashboard.
pub async fn observability_summary(State(state): State<AppState>) -> Json<ObservabilitySummary> {
    let db_latency_us = observability::check_db_readiness(&state.pool).await.ok();
    let db_ready = db_latency_us.is_some();
    state.observability.record_readiness(db_ready, None);
    let db_size_bytes = observability::query_db_size_bytes(&state.pool, &state.config.database_url).await;
    Json(state.observability.snapshot(db_ready, db_latency_us, db_size_bytes))
}

/// List recent alerts captured by the alert route or internal monitors.
pub async fn list_alerts(State(state): State<AppState>) -> Json<Vec<AlertRecord>> {
    Json(state.observability.list_alerts())
}

/// Alert receiver route for external uptime checks, Alertmanager, or manual test alerts.
pub async fn trigger_alert(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<CreateAlertRequest>,
) -> Result<(StatusCode, Json<AlertRecord>), ApiError> {
    if req.title.trim().is_empty() {
        return Err(ApiError::Validation("title: required".into()));
    }
    let alert = state.observability.trigger_alert(req);
    Ok((StatusCode::CREATED, Json(alert)))
}

/// RFC 2324 compliance is not optional.
pub async fn teapot() -> ApiError {
    ApiError::Teapot
}

pub async fn not_found() -> ApiError {
    ApiError::NotFound
}
