//! The audit log: an append-only record of security-relevant actions.
//!
//! Two sources feed it: domain events (through [`AuditEventSink`]) and the HTTP layer,
//! which records admin access, granted or refused. Reading it back is an admin endpoint.

use std::net::IpAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use nullbox_domain::DomainEvent;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::events::{EventSink, TracingEventSink};

pub const MAX_AUDIT_PAGE: u32 = 500;

/// One thing that happened. Build with [`AuditEntry::new`] and the chained setters.
#[derive(Debug, Clone, Default)]
pub struct AuditEntry {
    pub kind: String,
    pub actor: Option<String>,
    pub ticket_id: Option<Uuid>,
    pub client_ip: Option<IpAddr>,
    pub detail: Option<String>,
}

impl AuditEntry {
    pub fn new(kind: impl Into<String>) -> Self {
        Self { kind: kind.into(), ..Self::default() }
    }

    pub fn actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    pub fn ticket(mut self, ticket_id: Uuid) -> Self {
        self.ticket_id = Some(ticket_id);
        self
    }

    pub fn client_ip(mut self, client_ip: IpAddr) -> Self {
        self.client_ip = Some(client_ip);
        self
    }

    /// Free text or JSON. Never put a secret here: the log is readable by admins.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// A stored entry, as returned by `GET /api/admin/audit`.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct AuditRecord {
    pub id: String,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub actor: Option<String>,
    pub ticket_id: Option<String>,
    pub client_ip: Option<String>,
    pub detail: Option<String>,
}

pub struct AuditLog {
    pool: SqlitePool,
}

impl AuditLog {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Append an entry. A failure is logged and swallowed: a broken audit log must
    /// not take the request down with it, but it must be loud.
    pub async fn record(&self, entry: AuditEntry) {
        tracing::info!(target: "nullbox::audit", kind = %entry.kind, actor = ?entry.actor, ticket_id = ?entry.ticket_id, client_ip = ?entry.client_ip, "audit");
        let result = sqlx::query(
            "INSERT INTO audit_log (id, at, kind, actor, ticket_id, client_ip, detail) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(Utc::now())
        .bind(&entry.kind)
        .bind(&entry.actor)
        .bind(entry.ticket_id.map(|id| id.to_string()))
        .bind(entry.client_ip.map(|ip| ip.to_string()))
        .bind(&entry.detail)
        .execute(&self.pool)
        .await;
        if let Err(error) = result {
            tracing::error!(target: "nullbox::audit", %error, kind = %entry.kind, "could not write the audit log");
        }
    }

    /// The most recent entries, newest first.
    pub async fn recent(&self, limit: u32) -> Result<Vec<AuditRecord>, sqlx::Error> {
        sqlx::query_as::<_, AuditRecord>(
            "SELECT id, at, kind, actor, ticket_id, client_ip, detail FROM audit_log ORDER BY at DESC, id DESC LIMIT ?",
        )
        .bind(i64::from(limit.clamp(1, MAX_AUDIT_PAGE)))
        .fetch_all(&self.pool)
        .await
    }
}

/// Writes every domain event to the audit log (and still logs it, like before).
pub struct AuditEventSink {
    audit: Arc<AuditLog>,
    observability: Arc<crate::observability::ObservabilityState>,
}

impl AuditEventSink {
    pub fn new(audit: Arc<AuditLog>, observability: Arc<crate::observability::ObservabilityState>) -> Self {
        Self { audit, observability }
    }
}

impl EventSink for AuditEventSink {
    fn emit(&self, event: DomainEvent) {
        let entry = entry_for(&event);
        self.observability.record_domain_event(&event);
        TracingEventSink.emit(event);
        // `emit` is synchronous and called after the write succeeded, so the audit
        // insert runs in the background instead of delaying the response.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let audit = self.audit.clone();
                runtime.spawn(async move { audit.record(entry).await });
            }
            Err(_) => tracing::error!(target: "nullbox::audit", kind = %entry.kind, "no runtime, audit entry dropped"),
        }
    }
}

/// Kind, actor and ticket come from the event; the whole event is kept as the detail.
/// Message bodies are not part of `DomainEvent`, so no customer text ends up here.
fn entry_for(event: &DomainEvent) -> AuditEntry {
    let payload = serde_json::to_value(event).unwrap_or_default();
    let text = |key: &str| payload.get(key).and_then(|value| value.as_str()).map(str::to_owned);
    AuditEntry {
        kind: text("kind").unwrap_or_else(|| "unknown".to_owned()),
        actor: text("actor"),
        ticket_id: text("ticket_id").and_then(|id| Uuid::parse_str(&id).ok()),
        client_ip: None,
        detail: Some(payload.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn entries_are_stored_and_listed_newest_first() {
        let audit = AuditLog::new(crate::db::connect_in_memory().await.unwrap());
        let ticket = Uuid::now_v7();
        audit.record(AuditEntry::new("first").actor("山田").ticket(ticket)).await;
        audit.record(AuditEntry::new("second").client_ip("203.0.113.9".parse().unwrap()).detail("why")).await;

        let records = audit.recent(10).await.unwrap();
        assert_eq!(records.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(), ["second", "first"]);
        assert_eq!(records[0].client_ip.as_deref(), Some("203.0.113.9"));
        assert_eq!(records[0].detail.as_deref(), Some("why"));
        assert_eq!(records[1].actor.as_deref(), Some("山田"));
        assert_eq!(records[1].ticket_id, Some(ticket.to_string()));
        assert_eq!(audit.recent(1).await.unwrap().len(), 1);
    }

    #[test]
    fn domain_events_keep_their_kind_actor_and_ticket() {
        let ticket_id = Uuid::now_v7();
        let entry = entry_for(&DomainEvent::TicketYeeted { ticket_id, actor: "Kenji".into() });
        assert_eq!(entry.kind, "ticket_yeeted");
        assert_eq!(entry.actor.as_deref(), Some("Kenji"));
        assert_eq!(entry.ticket_id, Some(ticket_id));
    }
}
