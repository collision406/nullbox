//! Nullbox API: the HTTP layer, services and storage around `nullbox-domain`.
//!
//! Extension seams, each in one place:
//! - auth: [`http::extract::Actor`]
//! - authorization: [`policy::can`]
//! - audit/webhooks/metrics: [`events::EventSink`] (today: [`audit::AuditEventSink`])
//! - middleware (tracing, CORS, rate limits, security headers): [`http::middleware`]
//! - storage: [`repo::TicketRepository`]
//! - settings: [`config::Config`]

pub mod audit;
pub mod config;
pub mod db;
pub mod dto;
pub mod error;
pub mod events;
pub mod http;
pub mod policy;
pub mod repo;
pub mod service;
pub mod token;

use std::sync::Arc;

pub use config::Config;
pub use error::ApiError;

/// Shared state for all handlers.
#[derive(Clone)]
pub struct AppState {
    pub tickets: Arc<service::TicketService>,
    pub config: Arc<Config>,
    pub audit: Arc<audit::AuditLog>,
    pub limits: Arc<http::guard::RateLimits>,
}

impl AppState {
    pub fn new(pool: sqlx::SqlitePool, config: Config) -> Self {
        let audit = Arc::new(audit::AuditLog::new(pool.clone()));
        let repo = Arc::new(repo::SqliteTicketRepository::new(pool));
        let events = Arc::new(audit::AuditEventSink::new(audit.clone()));
        Self {
            tickets: Arc::new(service::TicketService::new(repo, events)),
            limits: Arc::new(http::guard::RateLimits::new(&config)),
            config: Arc::new(config),
            audit,
        }
    }
}
