//! Request/response bodies. Exported to TypeScript by `cargo test`.

use chrono::{DateTime, Utc};
use nullbox_domain::{BeastMood, Message, Severity, Ticket, TicketStatus};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateTicketRequest {
    pub subject: String,
    pub body: String,
    pub severity: Severity,
    /// Leave both name and email out to stay anonymous.
    #[serde(default)]
    #[ts(optional)]
    pub customer_name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub customer_email: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CreateTicketResponse {
    pub ticket: Ticket,
    /// Shown once. Only its hash is stored.
    pub access_token: String,
    /// `{PUBLIC_WEB_URL}/t/{id}#{token}`: the token sits in the fragment so it never reaches server logs.
    pub access_link: String,
}

/// A ticket with its messages. The customer view never contains internal notes.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TicketThread {
    pub ticket: Ticket,
    pub messages: Vec<Message>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CustomerReplyRequest {
    pub body: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct WorkerMessageRequest {
    pub body: String,
    /// Internal notes are hidden from the customer.
    #[serde(default)]
    #[ts(optional)]
    pub internal: Option<bool>,
}

#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateTicketRequest {
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<TicketStatus>,
    #[serde(default)]
    #[ts(optional)]
    pub severity: Option<Severity>,
    /// An empty string unassigns.
    #[serde(default)]
    #[ts(optional)]
    pub assignee_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListTicketsQuery {
    /// Without a status, all active tickets (open, in progress, waiting) are listed.
    pub status: Option<TicketStatus>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BacklogStats {
    #[ts(type = "number")]
    pub open_count: u64,
    pub beast_mood: BeastMood,
    #[ts(type = "number | null")]
    pub oldest_open_age_secs: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct DomainEventCounts {
    #[ts(type = "number")]
    pub ticket_created: u64,
    #[ts(type = "number")]
    pub status_changed: u64,
    #[ts(type = "number")]
    pub message_added: u64,
    #[ts(type = "number")]
    pub ticket_yeeted: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RouteMetricsSummary {
    pub method: String,
    pub route: String,
    #[ts(type = "number")]
    pub requests: u64,
    #[ts(type = "number")]
    pub errors_4xx: u64,
    #[ts(type = "number")]
    pub errors_5xx: u64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AuditEventRecord {
    pub id: String,
    pub kind: String,
    pub ticket_id: Option<String>,
    pub actor: Option<String>,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AlertRecord {
    pub id: String,
    pub severity: String,
    pub source: String,
    pub title: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateAlertRequest {
    pub title: String,
    pub message: String,
    #[serde(default)]
    #[ts(optional)]
    pub severity: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ObservabilitySummary {
    pub status: String,
    #[ts(type = "number")]
    pub uptime_secs: u64,
    #[ts(type = "number")]
    pub in_flight_requests: u64,
    pub db_ready: bool,
    #[ts(type = "number | null")]
    pub db_latency_us: Option<u64>,
    #[ts(type = "number")]
    pub db_size_bytes: u64,
    #[ts(type = "number")]
    pub process_rss_bytes: u64,
    #[ts(type = "number")]
    pub process_open_fds: u64,
    #[ts(type = "number")]
    pub total_requests: u64,
    #[ts(type = "number")]
    pub total_errors_4xx: u64,
    #[ts(type = "number")]
    pub total_errors_5xx: u64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub routes: Vec<RouteMetricsSummary>,
    pub domain_events: DomainEventCounts,
    pub recent_events: Vec<AuditEventRecord>,
    pub recent_alerts: Vec<AlertRecord>,
}
