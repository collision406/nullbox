//! Abuse protection for every route: rate limits, a request timeout and security headers.
//! Wired up in [`super::middleware`].

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Extensions, HeaderMap, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::{ApiError, AppState, Config};

/// Largest request body the API reads. A ticket body is at most 20 000 characters,
/// which is 80 kB in the worst case (4-byte UTF-8) before JSON escaping.
pub const MAX_BODY_BYTES: usize = 128 * 1024;

/// Above this many tracked clients, expired entries are dropped before adding more.
const PRUNE_ABOVE: usize = 10_000;

/// A fixed-window counter per client IP, in memory. Good for one process; several
/// instances behind a load balancer need a shared store (Redis) instead.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl RateLimiter {
    /// `limit` requests per minute per IP. 0 disables the limiter.
    pub fn per_minute(limit: u32) -> Self {
        Self { limit, window: Duration::from_secs(60), hits: Mutex::new(HashMap::new()) }
    }

    /// Counts one request. `Err` carries the seconds until the window resets.
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        if self.limit == 0 {
            return Ok(());
        }
        // A poisoned lock only means another request panicked; the counters are still valid.
        let mut hits = self.hits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if hits.len() > PRUNE_ABOVE {
            hits.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        }
        let (started, count) = hits.entry(ip).or_insert((now, 0));
        if now.duration_since(*started) >= self.window {
            (*started, *count) = (now, 0);
        }
        if *count >= self.limit {
            let remaining = self.window.saturating_sub(now.duration_since(*started));
            return Err(remaining.as_secs().max(1));
        }
        *count += 1;
        Ok(())
    }
}

/// The limiters shared by all requests.
pub struct RateLimits {
    /// Every request.
    pub all: RateLimiter,
    /// `POST /api/tickets` and `POST /api/checkout`: public, unauthenticated writes (the
    /// second one costs a call to Stripe), so much stricter.
    pub create_ticket: RateLimiter,
}

impl RateLimits {
    pub fn new(config: &Config) -> Self {
        Self {
            all: RateLimiter::per_minute(config.rate_limit_per_minute),
            create_ticket: RateLimiter::per_minute(config.rate_limit_create_per_minute),
        }
    }
}

/// Who is calling. Behind a trusted proxy (`TRUST_PROXY`) this is the last
/// `X-Forwarded-For` entry: the one our proxy appended, which a client can't forge.
/// Otherwise it is the TCP peer, and the header is ignored.
pub fn client_ip(headers: &HeaderMap, extensions: &Extensions, trust_proxy: bool) -> IpAddr {
    let forwarded = || {
        let value = headers.get_all("x-forwarded-for").iter().next_back()?.to_str().ok()?;
        value.rsplit(',').next()?.trim().parse::<IpAddr>().ok()
    };
    let peer = || extensions.get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(addr)| addr.ip());
    let ip = if trust_proxy { forwarded().or_else(peer) } else { peer() };
    ip.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let ip = client_ip(request.headers(), request.extensions(), state.config.trust_proxy);
    let creating_ticket =
        request.method() == Method::POST && matches!(request.uri().path(), "/api/tickets" | "/api/checkout");

    let mut verdict = state.limits.all.check(ip);
    if verdict.is_ok() && creating_ticket {
        verdict = state.limits.create_ticket.check(ip);
    }
    match verdict {
        Ok(()) => next.run(request).await,
        Err(retry_after_secs) => {
            tracing::warn!(target: "nullbox::security", %ip, path = %request.uri().path(), "rate limit exceeded");
            ApiError::RateLimited { retry_after_secs }.into_response()
        }
    }
}

/// Answers with 503 when the rest of the stack takes longer than `limit`, so slow
/// requests can't pile up and hold connections forever.
pub async fn timeout(limit: Duration, request: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            tracing::warn!(target: "nullbox::security", ?limit, "request timed out");
            ApiError::Timeout.into_response()
        }
    }
}

/// Headers for every response, errors included. The API only ever returns JSON, so
/// the policy is the strictest one: load nothing, embed nowhere.
pub const SECURITY_HEADERS: [(&str, &str); 7] = [
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("content-security-policy", "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"),
    ("strict-transport-security", "max-age=63072000; includeSubDomains"),
    ("permissions-policy", "camera=(), microphone=(), geolocation=()"),
    // Responses carry customer names and emails: no cache may keep a copy.
    ("cache-control", "no-store"),
];

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in SECURITY_HEADERS {
        response.headers_mut().insert(header::HeaderName::from_static(name), HeaderValue::from_static(value));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
    const BOB: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2));

    #[test]
    fn the_limit_is_per_client_and_resets_with_the_window() {
        let limiter = RateLimiter::per_minute(2);
        let start = Instant::now();
        assert_eq!(limiter.check_at(ALICE, start), Ok(()));
        assert_eq!(limiter.check_at(ALICE, start), Ok(()));
        assert_eq!(limiter.check_at(ALICE, start + Duration::from_secs(15)), Err(45));
        // Another client is not affected.
        assert_eq!(limiter.check_at(BOB, start), Ok(()));
        // A minute later the window is new.
        assert_eq!(limiter.check_at(ALICE, start + Duration::from_secs(60)), Ok(()));
    }

    #[test]
    fn zero_disables_the_limiter() {
        let limiter = RateLimiter::per_minute(0);
        assert!((0..1000).all(|_| limiter.check(ALICE).is_ok()));
    }

    #[test]
    fn forwarded_addresses_are_only_believed_behind_a_trusted_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.7, 203.0.113.1"));
        let mut extensions = Extensions::new();
        extensions.insert(ConnectInfo(SocketAddr::from(([192, 0, 2, 44], 5000))));

        // Untrusted: the header is attacker-controlled, use the TCP peer.
        assert_eq!(client_ip(&headers, &extensions, false), IpAddr::from([192, 0, 2, 44]));
        // Trusted: the last entry is the one our proxy added; the first is whatever the client sent.
        assert_eq!(client_ip(&headers, &extensions, true), ALICE);
        // No header: fall back to the peer.
        assert_eq!(client_ip(&HeaderMap::new(), &extensions, true), IpAddr::from([192, 0, 2, 44]));
    }
}
