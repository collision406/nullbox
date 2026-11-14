//! Nullbox API: the HTTP layer, services and storage around `nullbox-domain`.
//!
//! Extension seams, each in one place:
//! - auth: [`http::extract::Actor`]
//! - authorization: [`policy::can`]
//! - audit/webhooks/metrics: [`events::EventSink`] (today: [`audit::AuditEventSink`])
//! - middleware (tracing, CORS, rate limits, security headers): [`http::middleware`]
//! - storage: [`repo::TicketRepository`]
//! - settings: [`config::Config`]

pub mod accounts;
pub mod audit;
pub mod config;
pub mod db;
pub mod dto;
pub mod error;
pub mod events;
pub mod http;
pub mod observability;
pub mod passwords;
pub mod payments;
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
    pub accounts: Arc<accounts::Accounts>,
    pub limits: Arc<http::guard::RateLimits>,
    /// `None` while `STRIPE_API_KEY` is unset.
    pub payments: Option<Arc<payments::Stripe>>,
    pub pool: sqlx::SqlitePool,
    pub observability: Arc<observability::ObservabilityState>,
}

impl AppState {
    pub fn new(pool: sqlx::SqlitePool, config: Config) -> Self {
        let audit = Arc::new(audit::AuditLog::new(pool.clone()));
        let accounts = Arc::new(accounts::Accounts::new(pool.clone(), &config.hibp_api_base));
        let repo = Arc::new(repo::SqliteTicketRepository::new(pool.clone()));
        let observability = Arc::new(observability::ObservabilityState::new());
        let events = Arc::new(audit::AuditEventSink::new(audit.clone(), observability.clone()));
        Self {
            tickets: Arc::new(service::TicketService::new(repo, events)),
            limits: Arc::new(http::guard::RateLimits::new(&config)),
            payments: config
                .stripe_api_key
                .as_ref()
                .map(|key| Arc::new(payments::Stripe::new(&config.stripe_api_base, key))),
            config: Arc::new(config),
            audit,
            accounts,
            pool,
            observability,
        }
    }
}
