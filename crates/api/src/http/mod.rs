//! Routes and the middleware stack.

mod admin;
mod auth;
mod checkout;
mod customer;
pub mod extract;
pub mod guard;
mod mcp;
mod sso;
mod utility;
mod worker;

use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::Response;
use axum::routing::{get, post};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::Instrument;
use uuid::Uuid;

use crate::observability::{REQUEST_ID_HEADER, RequestId};
use crate::{AppState, Config};
use extract::TICKET_TOKEN_HEADER;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        // customers
        .route("/tickets", post(customer::create_ticket).get(worker::list_tickets))
        .route("/public/tickets/{id}", get(customer::get_ticket))
        .route("/public/tickets/{id}/messages", post(customer::reply))
        // workers
        .route("/tickets/{id}", get(worker::get_ticket).patch(worker::update_ticket))
        .route("/tickets/{id}/messages", post(worker::add_message))
        .route("/tickets/{id}/yeet", post(worker::yeet_ticket))
        .route("/stats/backlog", get(worker::backlog_stats))
        // accounts and sessions
        .route("/auth/config", get(auth::config))
        .route("/auth/signup", post(auth::signup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/sso/login", get(sso::login))
        .route("/auth/sso/callback", get(sso::callback))
        .route("/me", get(auth::me))
        // payments
        .route("/checkout", post(checkout::create))
        // admin
        .route("/admin/export", get(admin::export))
        .route("/admin/audit", get(admin::audit))
        // observability & utility
        .route("/healthz", get(utility::healthz))
        .route("/readyz", get(utility::readyz))
        .route("/metrics", get(utility::metrics))
        .route("/observability/summary", get(utility::observability_summary))
        .route("/alerts", get(utility::list_alerts).post(utility::trigger_alert))
        .route("/alerts/webhook", post(utility::trigger_alert))
        .route("/teapot", get(utility::teapot));

    // MCP for AI agents lives next to /api, not under it, and passes the same middleware.
    let app = Router::new().nest("/api", api).merge(mcp::router(state.clone())).fallback(utility::not_found);
    middleware(app, &state).with_state(state)
}

/// The one place for cross-cutting layers. Tracing exporters and auth middleware
/// go here. Layers run bottom to top: the last one added sees the request first.
///
/// So a request passes: security headers (on every response, errors and CORS
/// preflights included), CORS (before the limits, so a browser can read a 429),
/// tracing, the rate limit, the timeout, and finally the body size limit.
///
/// Routes outside `/api` (an MCP server at `/mcp`, say) go next to the `nest` in
/// `router`, so they pass through here too.
pub fn middleware(router: Router<AppState>, state: &AppState) -> Router<AppState> {
    let timeout = Duration::from_secs(state.config.request_timeout_secs);
    router
        .layer(DefaultBodyLimit::max(guard::MAX_BODY_BYTES))
        .layer(from_fn(move |request, next| guard::timeout(timeout, request, next)))
        .layer(from_fn_with_state(state.clone(), guard::rate_limit))
        .layer(from_fn_with_state(state.clone(), observability_layer))
        .layer(TraceLayer::new_for_http())
        .layer(cors(&state.config))
        .layer(from_fn(guard::security_headers))
}

/// Propagates or generates `x-request-id`, records RED metrics, and emits a
/// structured JSON request log entry with `request_id` and `latency_ms`.
async fn observability_layer(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let route = req.extensions().get::<MatchedPath>().map(|m| m.as_str().to_owned()).unwrap_or_else(|| path.clone());

    req.extensions_mut().insert(RequestId(request_id.clone()));
    if let Ok(val) = HeaderValue::from_str(&request_id) {
        req.headers_mut().insert(HeaderName::from_static(REQUEST_ID_HEADER), val);
    }

    state.observability.inc_in_flight();
    let start = Instant::now();
    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        route = %route,
    );
    let mut response = next.run(req).instrument(span).await;
    let duration = start.elapsed();
    state.observability.dec_in_flight();

    let status = response.status().as_u16();
    let latency_ms = (duration.as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
    state.observability.record_http(&method, &route, status, duration, &request_id);

    if let Ok(val) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(HeaderName::from_static(REQUEST_ID_HEADER), val);
    }

    tracing::info!(
        target: "nullbox::http",
        request_id = %request_id,
        method = %method,
        path = %path,
        route = %route,
        status = status,
        latency_ms = latency_ms,
        "request completed"
    );

    response
}

/// CORS for the web app on another origin (Vite in dev, Tauri). `Authorization` is
/// allowed, so a bearer token works as it is. Session cookies need more: add
/// `.allow_credentials(true)` here and `credentials: "include"` to the `fetch` in
/// `apps/web/src/api/client.ts`. Behind one origin in production, none of this applies.
fn cors(config: &Config) -> CorsLayer {
    let origins: Vec<HeaderValue> = config
        .cors_allowed_origins
        .iter()
        .filter_map(|origin| match origin.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::warn!(%origin, "ignoring invalid CORS origin");
                None
            }
        })
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static(TICKET_TOKEN_HEADER),
            HeaderName::from_static(REQUEST_ID_HEADER),
        ])
        .expose_headers([HeaderName::from_static(REQUEST_ID_HEADER)])
}
