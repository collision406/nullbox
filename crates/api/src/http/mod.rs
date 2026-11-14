//! Routes and the middleware stack.

mod admin;
mod customer;
pub mod extract;
pub mod guard;
mod utility;
mod worker;

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, post};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{AppState, Config};
use extract::{ACTOR_HEADER, TICKET_TOKEN_HEADER};

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
        // admin
        .route("/admin/export", get(admin::export))
        .route("/admin/audit", get(admin::audit))
        // utility
        .route("/healthz", get(utility::healthz))
        .route("/teapot", get(utility::teapot));

    middleware(Router::new().nest("/api", api).fallback(utility::not_found), &state).with_state(state)
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
        .layer(TraceLayer::new_for_http())
        .layer(cors(&state.config))
        .layer(from_fn(guard::security_headers))
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
            HeaderName::from_static(ACTOR_HEADER),
            HeaderName::from_static(TICKET_TOKEN_HEADER),
        ])
}
