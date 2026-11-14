//! Admin endpoints: the export the CEO pastes into a spreadsheet every Monday, and
//! the audit log. Both need the [`Admin`] extractor, i.e. a valid `X-Admin-Token`.

use axum::Json;
use axum::extract::State;
use nullbox_domain::Ticket;
use serde::Deserialize;

use super::extract::{Admin, ApiQuery, ClientIp};
use crate::audit::{AuditEntry, AuditRecord};
use crate::{ApiError, AppState};

/// Every ticket, with customer names and emails. Each export is written to the
/// audit log: this is the single most sensitive read in the system.
pub async fn export(
    State(state): State<AppState>,
    _admin: Admin,
    ClientIp(ip): ClientIp,
) -> Result<Json<Vec<Ticket>>, ApiError> {
    let tickets = state.tickets.export().await?;
    let entry = AuditEntry::new("admin_export").client_ip(ip).detail(format!("{} tickets", tickets.len()));
    state.audit.record(entry).await;
    Ok(Json(tickets))
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// How many entries, newest first. Default 100, at most `audit::MAX_AUDIT_PAGE`.
    pub limit: Option<u32>,
}

pub async fn audit(
    State(state): State<AppState>,
    _admin: Admin,
    ApiQuery(query): ApiQuery<AuditQuery>,
) -> Result<Json<Vec<AuditRecord>>, ApiError> {
    let records =
        state.audit.recent(query.limit.unwrap_or(100)).await.map_err(|e| ApiError::WorksOnMyMachine(e.to_string()))?;
    Ok(Json(records))
}
