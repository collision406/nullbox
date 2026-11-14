//! Observability engine: structured JSON logging, request IDs, RED metrics,
//! SQLite readiness & size probes, process telemetry, and alert routing.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use nullbox_domain::DomainEvent;
use serde_json::{Map, Value, json};
use sqlx::SqlitePool;
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::{FmtContext, FormattedFields};
use tracing_subscriber::registry::LookupSpan;
use uuid::Uuid;

use crate::dto::{
    AlertRecord, AuditEventRecord, CreateAlertRequest, DomainEventCounts, ObservabilitySummary, RouteMetricsSummary,
};

const HISTOGRAM_BUCKETS: &[f64] = &[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];
const MAX_RECENT_SAMPLES: usize = 256;
const MAX_AUDIT_EVENTS: usize = 200;
const MAX_ALERTS: usize = 100;

/// Request ID header shared between Caddy (`deploy/Caddyfile`) and `nullbox-api`.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Request ID stored in request extensions so handlers and error responses can read it.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

#[derive(Default)]
struct RouteStats {
    requests: u64,
    errors_4xx: u64,
    errors_5xx: u64,
    duration_sum_secs: f64,
    bucket_counts: [u64; HISTOGRAM_BUCKETS.len()],
    recent_ms: VecDeque<f64>,
}

impl RouteStats {
    fn observe(&mut self, status: u16, duration_secs: f64) {
        self.requests += 1;
        if (400..500).contains(&status) {
            self.errors_4xx += 1;
        } else if status >= 500 {
            self.errors_5xx += 1;
        }
        self.duration_sum_secs += duration_secs;
        for (idx, &upper) in HISTOGRAM_BUCKETS.iter().enumerate() {
            if duration_secs <= upper {
                self.bucket_counts[idx] += 1;
            }
        }
        let ms = duration_secs * 1000.0;
        if self.recent_ms.len() >= MAX_RECENT_SAMPLES {
            self.recent_ms.pop_front();
        }
        self.recent_ms.push_back(ms);
    }

    fn avg_ms(&self) -> f64 {
        if self.requests == 0 { 0.0 } else { (self.duration_sum_secs * 1000.0) / (self.requests as f64) }
    }

    fn p95_ms(&self) -> f64 {
        percentile_95(self.recent_ms.iter().copied())
    }
}

fn percentile_95(iter: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<f64> = iter.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(values.len() - 1);
    values[idx]
}

#[derive(Default)]
struct TelemetryInner {
    status_counts: BTreeMap<(String, String, u16), u64>,
    route_stats: BTreeMap<(String, String), RouteStats>,
    domain_counts: DomainEventCounts,
    audit_events: VecDeque<AuditEventRecord>,
    alerts: VecDeque<AlertRecord>,
    last_db_ready: Option<bool>,
}

pub struct ObservabilityState {
    started_at: Instant,
    in_flight: AtomicU64,
    inner: Mutex<TelemetryInner>,
}

impl Default for ObservabilityState {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservabilityState {
    pub fn new() -> Self {
        Self { started_at: Instant::now(), in_flight: AtomicU64::new(0), inner: Mutex::new(TelemetryInner::default()) }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn inc_in_flight(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_in_flight(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    pub fn record_http(&self, method: &str, route: &str, status: u16, duration: Duration, request_id: &str) {
        let secs = duration.as_secs_f64();
        let mut guard = self.inner.lock().expect("telemetry lock");
        *guard.status_counts.entry((method.to_owned(), route.to_owned(), status)).or_insert(0) += 1;
        guard.route_stats.entry((method.to_owned(), route.to_owned())).or_default().observe(status, secs);

        if status >= 500 {
            push_alert(
                &mut guard.alerts,
                "error",
                "http_5xx",
                format!("HTTP {status} on {method} {route}"),
                format!("Request {request_id} returned status {status} in {:.2}ms", secs * 1000.0),
            );
        }
    }

    pub fn record_domain_event(&self, event: &DomainEvent) {
        let mut guard = self.inner.lock().expect("telemetry lock");
        let (kind, ticket_id, actor, summary) = match event {
            DomainEvent::TicketCreated { ticket_id, number, severity } => {
                guard.domain_counts.ticket_created += 1;
                (
                    "ticket_created".to_owned(),
                    Some(ticket_id.to_string()),
                    None,
                    format!("Ticket #{number} created (severity: {})", severity.as_str()),
                )
            }
            DomainEvent::StatusChanged { ticket_id, from, to, actor } => {
                guard.domain_counts.status_changed += 1;
                (
                    "status_changed".to_owned(),
                    Some(ticket_id.to_string()),
                    actor.clone(),
                    format!("Status changed: {} -> {}", from.as_str(), to.as_str()),
                )
            }
            DomainEvent::MessageAdded { ticket_id, message_id: _, internal, actor } => {
                guard.domain_counts.message_added += 1;
                let visibility = if *internal { "internal note" } else { "reply" };
                ("message_added".to_owned(), Some(ticket_id.to_string()), actor.clone(), format!("Added {visibility}"))
            }
            DomainEvent::TicketYeeted { ticket_id, actor } => {
                guard.domain_counts.ticket_yeeted += 1;
                (
                    "ticket_yeeted".to_owned(),
                    Some(ticket_id.to_string()),
                    Some(actor.clone()),
                    "Ticket yeeted (resolved)".to_owned(),
                )
            }
        };

        if guard.audit_events.len() >= MAX_AUDIT_EVENTS {
            guard.audit_events.pop_back();
        }
        guard.audit_events.push_front(AuditEventRecord {
            id: Uuid::now_v7().to_string(),
            kind,
            ticket_id,
            actor,
            summary,
            created_at: Utc::now(),
        });
    }

    pub fn record_readiness(&self, ready: bool, detail: Option<&str>) {
        let mut guard = self.inner.lock().expect("telemetry lock");
        let changed = guard.last_db_ready != Some(ready);
        guard.last_db_ready = Some(ready);
        if !ready && changed {
            push_alert(
                &mut guard.alerts,
                "critical",
                "readyz",
                "Database readiness check failed".to_owned(),
                detail.unwrap_or("SQLite readiness probe failed").to_owned(),
            );
        }
    }

    pub fn trigger_alert(&self, req: CreateAlertRequest) -> AlertRecord {
        let mut guard = self.inner.lock().expect("telemetry lock");
        let severity = req.severity.unwrap_or_else(|| "warning".to_owned());
        let source = req.source.unwrap_or_else(|| "external_uptime".to_owned());
        let alert = push_alert(&mut guard.alerts, &severity, &source, req.title, req.message);
        tracing::warn!(
            target: "nullbox::alerts",
            alert_id = %alert.id,
            severity = %alert.severity,
            source = %alert.source,
            title = %alert.title,
            message = %alert.message,
            "alert triggered"
        );
        alert
    }

    pub fn list_alerts(&self) -> Vec<AlertRecord> {
        let guard = self.inner.lock().expect("telemetry lock");
        guard.alerts.iter().cloned().collect()
    }

    pub fn snapshot(&self, db_ready: bool, db_latency_us: Option<u64>, db_size_bytes: u64) -> ObservabilitySummary {
        let guard = self.inner.lock().expect("telemetry lock");
        let (rss_bytes, open_fds) = process_metrics();

        let mut total_requests = 0;
        let mut total_errors_4xx = 0;
        let mut total_errors_5xx = 0;
        let mut total_duration_secs = 0.0;
        let mut all_recent_ms = Vec::new();
        let mut routes = Vec::new();

        for ((method, route), stats) in &guard.route_stats {
            total_requests += stats.requests;
            total_errors_4xx += stats.errors_4xx;
            total_errors_5xx += stats.errors_5xx;
            total_duration_secs += stats.duration_sum_secs;
            all_recent_ms.extend(stats.recent_ms.iter().copied());

            routes.push(RouteMetricsSummary {
                method: method.clone(),
                route: route.clone(),
                requests: stats.requests,
                errors_4xx: stats.errors_4xx,
                errors_5xx: stats.errors_5xx,
                avg_latency_ms: round2(stats.avg_ms()),
                p95_latency_ms: round2(stats.p95_ms()),
            });
        }
        routes.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.route.cmp(&b.route)));

        let avg_latency_ms =
            if total_requests == 0 { 0.0 } else { (total_duration_secs * 1000.0) / (total_requests as f64) };
        let p95_latency_ms = percentile_95(all_recent_ms.into_iter());

        let status = if db_ready && total_errors_5xx == 0 { "healthy" } else { "degraded" };

        ObservabilitySummary {
            status: status.to_owned(),
            uptime_secs: self.uptime_secs(),
            in_flight_requests: self.in_flight(),
            db_ready,
            db_latency_us,
            db_size_bytes,
            process_rss_bytes: rss_bytes,
            process_open_fds: open_fds,
            total_requests,
            total_errors_4xx,
            total_errors_5xx,
            avg_latency_ms: round2(avg_latency_ms),
            p95_latency_ms: round2(p95_latency_ms),
            routes,
            domain_events: guard.domain_counts.clone(),
            recent_events: guard.audit_events.iter().cloned().collect(),
            recent_alerts: guard.alerts.iter().cloned().collect(),
        }
    }

    pub fn render_prometheus(&self, db_ready: bool, db_size_bytes: u64, open_tickets: u64) -> String {
        let guard = self.inner.lock().expect("telemetry lock");
        let (rss_bytes, open_fds) = process_metrics();
        let mut out = String::with_capacity(4096);

        out.push_str(
            "# HELP nullbox_http_requests_total Total HTTP requests processed by method, route, and status.\n",
        );
        out.push_str("# TYPE nullbox_http_requests_total counter\n");
        for ((method, route, status), count) in &guard.status_counts {
            let route_esc = escape_label(route);
            out.push_str(&format!(
                "nullbox_http_requests_total{{method=\"{method}\",route=\"{route_esc}\",status=\"{status}\"}} {count}\n"
            ));
        }

        out.push_str("# HELP nullbox_http_errors_total Total HTTP 4xx and 5xx error responses by route and class.\n");
        out.push_str("# TYPE nullbox_http_errors_total counter\n");
        for ((method, route), stats) in &guard.route_stats {
            let route_esc = escape_label(route);
            out.push_str(&format!(
                "nullbox_http_errors_total{{method=\"{method}\",route=\"{route_esc}\",class=\"4xx\"}} {}\n",
                stats.errors_4xx
            ));
            out.push_str(&format!(
                "nullbox_http_errors_total{{method=\"{method}\",route=\"{route_esc}\",class=\"5xx\"}} {}\n",
                stats.errors_5xx
            ));
        }

        out.push_str("# HELP nullbox_http_request_duration_seconds HTTP request latency histogram in seconds.\n");
        out.push_str("# TYPE nullbox_http_request_duration_seconds histogram\n");
        for ((method, route), stats) in &guard.route_stats {
            let route_esc = escape_label(route);
            for (idx, &upper) in HISTOGRAM_BUCKETS.iter().enumerate() {
                out.push_str(&format!(
                    "nullbox_http_request_duration_seconds_bucket{{method=\"{method}\",route=\"{route_esc}\",le=\"{upper}\"}} {}\n",
                    stats.bucket_counts[idx]
                ));
            }
            out.push_str(&format!(
                "nullbox_http_request_duration_seconds_bucket{{method=\"{method}\",route=\"{route_esc}\",le=\"+Inf\"}} {}\n",
                stats.requests
            ));
            out.push_str(&format!(
                "nullbox_http_request_duration_seconds_sum{{method=\"{method}\",route=\"{route_esc}\"}} {:.6}\n",
                stats.duration_sum_secs
            ));
            out.push_str(&format!(
                "nullbox_http_request_duration_seconds_count{{method=\"{method}\",route=\"{route_esc}\"}} {}\n",
                stats.requests
            ));
        }

        out.push_str("# HELP nullbox_http_in_flight_requests Current number of in-flight HTTP requests.\n");
        out.push_str("# TYPE nullbox_http_in_flight_requests gauge\n");
        out.push_str(&format!("nullbox_http_in_flight_requests {}\n", self.in_flight()));

        out.push_str("# HELP nullbox_db_ready Database readiness status (1 = ready, 0 = unreachable).\n");
        out.push_str("# TYPE nullbox_db_ready gauge\n");
        out.push_str(&format!("nullbox_db_ready {}\n", u8::from(db_ready)));

        out.push_str("# HELP nullbox_db_size_bytes SQLite database size in bytes.\n");
        out.push_str("# TYPE nullbox_db_size_bytes gauge\n");
        out.push_str(&format!("nullbox_db_size_bytes {db_size_bytes}\n"));

        out.push_str("# HELP nullbox_tickets_open Current number of open/active tickets.\n");
        out.push_str("# TYPE nullbox_tickets_open gauge\n");
        out.push_str(&format!("nullbox_tickets_open {open_tickets}\n"));

        out.push_str("# HELP nullbox_domain_events_total Domain events emitted through EventSink.\n");
        out.push_str("# TYPE nullbox_domain_events_total counter\n");
        out.push_str(&format!(
            "nullbox_domain_events_total{{kind=\"ticket_created\"}} {}\n",
            guard.domain_counts.ticket_created
        ));
        out.push_str(&format!(
            "nullbox_domain_events_total{{kind=\"status_changed\"}} {}\n",
            guard.domain_counts.status_changed
        ));
        out.push_str(&format!(
            "nullbox_domain_events_total{{kind=\"message_added\"}} {}\n",
            guard.domain_counts.message_added
        ));
        out.push_str(&format!(
            "nullbox_domain_events_total{{kind=\"ticket_yeeted\"}} {}\n",
            guard.domain_counts.ticket_yeeted
        ));

        out.push_str("# HELP nullbox_alerts_total Total alerts recorded by the alert router.\n");
        out.push_str("# TYPE nullbox_alerts_total gauge\n");
        out.push_str(&format!("nullbox_alerts_total {}\n", guard.alerts.len()));

        out.push_str("# HELP nullbox_process_uptime_seconds Process uptime in seconds.\n");
        out.push_str("# TYPE nullbox_process_uptime_seconds gauge\n");
        out.push_str(&format!("nullbox_process_uptime_seconds {}\n", self.uptime_secs()));

        out.push_str("# HELP nullbox_process_resident_memory_bytes Process resident set size (RSS) in bytes.\n");
        out.push_str("# TYPE nullbox_process_resident_memory_bytes gauge\n");
        out.push_str(&format!("nullbox_process_resident_memory_bytes {rss_bytes}\n"));

        out.push_str("# HELP nullbox_process_open_fds Open file descriptors held by the API process.\n");
        out.push_str("# TYPE nullbox_process_open_fds gauge\n");
        out.push_str(&format!("nullbox_process_open_fds {open_fds}\n"));

        out
    }
}

fn push_alert(
    alerts: &mut VecDeque<AlertRecord>,
    severity: &str,
    source: &str,
    title: String,
    message: String,
) -> AlertRecord {
    let record = AlertRecord {
        id: Uuid::now_v7().to_string(),
        severity: severity.to_owned(),
        source: source.to_owned(),
        title,
        message,
        created_at: Utc::now(),
    };
    if alerts.len() >= MAX_ALERTS {
        alerts.pop_back();
    }
    alerts.push_front(record.clone());
    record
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

/// Probe SQLite readiness with a fast `SELECT 1` and return latency in microseconds.
pub async fn check_db_readiness(pool: &SqlitePool) -> Result<u64, String> {
    if pool.is_closed() {
        return Err("database pool is closed".to_owned());
    }
    let start = Instant::now();
    let probe = tokio::time::timeout(Duration::from_secs(2), sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(pool))
        .await
        .map_err(|_| "database readiness probe timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    if probe != 1 {
        return Err(format!("unexpected readiness probe value: {probe}"));
    }
    Ok(start.elapsed().as_micros() as u64)
}

/// Compute SQLite database size in bytes via `PRAGMA page_count * PRAGMA page_size`
/// (works for both in-memory and on-disk databases) plus WAL file size when present.
pub async fn query_db_size_bytes(pool: &SqlitePool, database_url: &str) -> u64 {
    let pragma_bytes = if !pool.is_closed() {
        let page_count =
            sqlx::query_scalar::<_, i64>("PRAGMA page_count").fetch_one(pool).await.unwrap_or(0).max(0) as u64;
        let page_size =
            sqlx::query_scalar::<_, i64>("PRAGMA page_size").fetch_one(pool).await.unwrap_or(4096).max(0) as u64;
        page_count.saturating_mul(page_size)
    } else {
        0
    };

    let fs_bytes = sqlite_file_size(database_url);
    pragma_bytes.max(fs_bytes)
}

fn sqlite_file_size(database_url: &str) -> u64 {
    let Some(path) = database_url.strip_prefix("sqlite://") else {
        return 0;
    };
    if path.is_empty() || path.starts_with(':') {
        return 0;
    }
    let main_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let wal_len = std::fs::metadata(format!("{path}-wal")).map(|m| m.len()).unwrap_or(0);
    main_len.saturating_add(wal_len)
}

/// Process resident memory (RSS in bytes) and open file descriptor count.
pub fn process_metrics() -> (u64, u64) {
    let rss = read_linux_rss_bytes().unwrap_or(16 * 1024 * 1024);
    let fds = count_open_fds().unwrap_or(16);
    (rss, fds)
}

fn read_linux_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(resident_pages.saturating_mul(4096))
}

fn count_open_fds() -> Option<u64> {
    if let Ok(entries) = std::fs::read_dir("/proc/self/fd") {
        return Some(entries.count() as u64);
    }
    if let Ok(entries) = std::fs::read_dir("/dev/fd") {
        return Some(entries.count() as u64);
    }
    None
}

/// Structured single-line JSON log formatter for `tracing_subscriber`.
/// Produces JSON lines with `ts`, `level`, `target`, `message`, and all event/span fields.
pub struct JsonLogFormatter;

struct JsonFieldVisitor<'a>(&'a mut Map<String, Value>);

impl tracing::field::Visit for JsonFieldVisitor<'_> {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().to_owned(), json!(format!("{value:?}")));
    }
}

impl<S, N> FormatEvent<S, N> for JsonLogFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(&self, ctx: &FmtContext<'_, S, N>, mut writer: Writer<'_>, event: &Event<'_>) -> fmt::Result {
        let meta = event.metadata();
        let mut map = Map::new();
        map.insert("ts".to_owned(), json!(Utc::now().to_rfc3339()));
        map.insert("level".to_owned(), json!(meta.level().as_str()));
        map.insert("target".to_owned(), json!(meta.target()));

        if let Some(scope) = ctx.event_scope() {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<FormattedFields<N>>()
                    && !fields.fields.is_empty()
                {
                    map.insert(format!("span_{}", span.name()), json!(fields.fields.as_str()));
                }
            }
        }

        let mut visitor = JsonFieldVisitor(&mut map);
        event.record(&mut visitor);

        let line = Value::Object(map).to_string();
        writeln!(writer, "{line}")
    }
}
