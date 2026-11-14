use std::sync::Arc;

use nullbox_domain::DomainEvent;

use crate::observability::ObservabilityState;

/// Where domain events go. The MVP only logs them; audit logs, email,
/// webhooks and metrics are more implementations of this trait (or a fan-out).
pub trait EventSink: Send + Sync {
    fn emit(&self, event: DomainEvent);
}

pub struct TracingEventSink;

impl EventSink for TracingEventSink {
    fn emit(&self, event: DomainEvent) {
        let payload = serde_json::to_string(&event).unwrap_or_default();
        tracing::info!(target: "nullbox::events", %payload, "domain event");
    }
}

/// Fan-out sink: emits structured tracing logs and updates in-memory metrics + audit log.
pub struct ObservabilityEventSink {
    observability: Arc<ObservabilityState>,
}

impl ObservabilityEventSink {
    pub fn new(observability: Arc<ObservabilityState>) -> Self {
        Self { observability }
    }
}

impl EventSink for ObservabilityEventSink {
    fn emit(&self, event: DomainEvent) {
        let payload = serde_json::to_string(&event).unwrap_or_default();
        tracing::info!(target: "nullbox::events", %payload, "domain event");
        self.observability.record_domain_event(&event);
    }
}
