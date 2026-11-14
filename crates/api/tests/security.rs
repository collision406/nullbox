//! One test per security finding. Each one fails on the code before the fix.

mod common;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use common::{ADMIN_TOKEN, WORKER, app, app_with, config, create_ticket, send, send_full};
use nullbox_api::http::guard;
use serde_json::{Value, json};
use tower::ServiceExt;

// --- 1. The admin token had a hard-coded default that was leaked ------------------

#[tokio::test]
async fn without_an_admin_token_configured_nobody_can_export() {
    let app = app_with(nullbox_api::Config { admin_token: None, ..config() }).await;
    create_ticket(&app, "Private").await;

    // Nothing, an empty header, the old placeholder default, a guess: all refused.
    let attempts: [&[(&str, &str)]; 4] = [
        &[],
        &[("x-admin-token", "")],
        &[("x-admin-token", "REDACTED-leaked-token-rotated")],
        &[("x-admin-token", "admin")],
    ];
    for headers in attempts {
        let (status, body) = send(&app, "GET", "/api/admin/export", headers, None).await;
        assert_eq!((status, &body["code"]), (StatusCode::FORBIDDEN, &json!("forbidden")), "{headers:?}");
        assert!(body.get("0").is_none() && !body.is_array(), "no ticket data in a refusal");
    }
}

#[tokio::test]
async fn only_the_exact_admin_token_opens_the_export() {
    let app = app().await;
    create_ticket(&app, "Private").await;

    let almost = &ADMIN_TOKEN[..ADMIN_TOKEN.len() - 1];
    for wrong in [almost, &ADMIN_TOKEN.to_uppercase(), &format!("{ADMIN_TOKEN} ")] {
        let (status, _) = send(&app, "GET", "/api/admin/export", &[("x-admin-token", wrong)], None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{wrong:?}");
    }
    let (status, tickets) = send(&app, "GET", "/api/admin/export", &[("x-admin-token", ADMIN_TOKEN)], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tickets.as_array().unwrap().len(), 1);
}

// --- 2. A customer token opened every ticket, not just its own --------------------

#[tokio::test]
async fn a_customer_token_only_opens_its_own_ticket() {
    let app = app().await;
    let (alice_id, alice_token) = create_ticket(&app, "Alice's secret").await;
    let (bob_id, bob_token) = create_ticket(&app, "Bob's secret").await;

    // Alice's token on Bob's ticket: looks exactly like a ticket that doesn't exist.
    let (status, body) =
        send(&app, "GET", &format!("/api/public/tickets/{bob_id}"), &[("x-ticket-token", &alice_token)], None).await;
    assert_eq!((status, &body["code"]), (StatusCode::NOT_FOUND, &json!("not_found")));
    assert!(!body.to_string().contains("Bob's secret"));

    // She can't write into it either.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/public/tickets/{bob_id}/messages"),
        &[("x-ticket-token", &alice_token)],
        Some(json!({ "body": "I am totally Bob" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, thread) = send(&app, "GET", &format!("/api/tickets/{bob_id}"), &[WORKER], None).await;
    assert_eq!(thread["messages"], json!([]), "nothing was added to Bob's ticket");

    // Both tokens still open their own ticket.
    for (id, token, subject) in [(&alice_id, &alice_token, "Alice's secret"), (&bob_id, &bob_token, "Bob's secret")] {
        let (status, thread) =
            send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", token)], None).await;
        assert_eq!((status, &thread["ticket"]["subject"]), (StatusCode::OK, &json!(subject)));
    }
}

// --- 4. No rate limiting ----------------------------------------------------------

/// Behind a trusted proxy, the client address comes from `X-Forwarded-For`.
fn from_ip(ip: &'static str) -> [(&'static str, &'static str); 1] {
    [("x-forwarded-for", ip)]
}

#[tokio::test]
async fn ticket_creation_is_rate_limited_per_client() {
    let app = app_with(nullbox_api::Config { rate_limit_create_per_minute: 3, trust_proxy: true, ..config() }).await;
    let ticket = || Some(json!({ "subject": "spam", "body": "spam", "severity": "p4" }));

    for _ in 0..3 {
        let (status, _) = send(&app, "POST", "/api/tickets", &from_ip("203.0.113.1"), ticket()).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, headers, body) = send_full(&app, "POST", "/api/tickets", &from_ip("203.0.113.1"), ticket()).await;
    assert_eq!((status, &body["code"]), (StatusCode::TOO_MANY_REQUESTS, &json!("rate_limited")));
    let retry_after: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry_after));

    // The refused request stored nothing, and another client is not affected.
    let (_, inbox) = send(&app, "GET", "/api/tickets", &[WORKER, from_ip("203.0.113.1")[0]], None).await;
    assert_eq!(inbox.as_array().unwrap().len(), 3);
    let (status, _) = send(&app, "POST", "/api/tickets", &from_ip("203.0.113.2"), ticket()).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn every_route_has_a_general_rate_limit() {
    let app = app_with(nullbox_api::Config { rate_limit_per_minute: 5, trust_proxy: true, ..config() }).await;
    for _ in 0..5 {
        let (status, _) = send(&app, "GET", "/api/healthz", &from_ip("203.0.113.1"), None).await;
        assert_eq!(status, StatusCode::OK);
    }
    // Also guessing the admin token is cut off after five tries.
    let headers = [("x-admin-token", "guess"), from_ip("203.0.113.1")[0]];
    let (status, body) = send(&app, "GET", "/api/admin/export", &headers, None).await;
    assert_eq!((status, &body["code"]), (StatusCode::TOO_MANY_REQUESTS, &json!("rate_limited")));
}

#[tokio::test]
async fn a_spoofed_forwarded_header_does_not_dodge_the_limit() {
    // Not behind a proxy: X-Forwarded-For is attacker-controlled and must be ignored.
    let app = app_with(nullbox_api::Config { rate_limit_per_minute: 2, trust_proxy: false, ..config() }).await;
    let mut statuses = Vec::new();
    for ip in ["203.0.113.1", "203.0.113.2", "203.0.113.3"] {
        statuses.push(send(&app, "GET", "/api/healthz", &from_ip(ip), None).await.0);
    }
    assert_eq!(statuses, [StatusCode::OK, StatusCode::OK, StatusCode::TOO_MANY_REQUESTS]);
}

// --- 5. No body size limit, no request timeout ------------------------------------

#[tokio::test]
async fn oversized_bodies_are_refused_before_they_are_parsed() {
    let app = app().await;
    let huge = json!({ "subject": "big", "body": "x".repeat(guard::MAX_BODY_BYTES + 1), "severity": "p4" });
    let (status, body) = send(&app, "POST", "/api/tickets", &[], Some(huge)).await;
    assert_eq!((status, &body["code"]), (StatusCode::PAYLOAD_TOO_LARGE, &json!("payload_too_large")));

    // The longest ticket the validation allows (20 000 four-byte characters) still fits.
    let longest = json!({ "subject": "big", "body": "🔥".repeat(20_000), "severity": "p4" });
    let (status, _) = send(&app, "POST", "/api/tickets", &[], Some(longest)).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn slow_requests_are_cut_off() {
    let limit = Duration::from_millis(50);
    let app = Router::new()
        .route("/slow", get(|| async { tokio::time::sleep(Duration::from_secs(30)).await }))
        .route("/fast", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(move |request, next| guard::timeout(limit, request, next)));

    let call = |path: &'static str| {
        let app = app.clone();
        async move { app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap().status() }
    };
    assert_eq!(call("/fast").await, StatusCode::OK);
    let started = std::time::Instant::now();
    assert_eq!(call("/slow").await, StatusCode::SERVICE_UNAVAILABLE);
    assert!(started.elapsed() < Duration::from_secs(5), "the handler was not waited for");
}

// --- 6. No security headers -------------------------------------------------------

#[tokio::test]
async fn every_response_carries_the_security_headers() {
    // Preflights are answered by the CORS layer before the limiter, so three requests count.
    let app = app_with(nullbox_api::Config { rate_limit_per_minute: 3, ..config() }).await;
    const PREFLIGHT: &[(&str, &str)] = &[
        ("origin", "http://localhost:5173"),
        ("access-control-request-method", "POST"),
        ("access-control-request-headers", "content-type"),
    ];
    // A success, a preflight, an auth error, an unknown path, and finally a 429.
    type Case = (&'static str, &'static str, &'static [(&'static str, &'static str)], StatusCode);
    let requests: [Case; 5] = [
        ("GET", "/api/healthz", &[], StatusCode::OK),
        ("OPTIONS", "/api/tickets", PREFLIGHT, StatusCode::OK),
        ("GET", "/api/tickets", &[], StatusCode::UNAUTHORIZED),
        ("GET", "/nowhere", &[], StatusCode::NOT_FOUND),
        ("GET", "/api/healthz", &[], StatusCode::TOO_MANY_REQUESTS),
    ];
    for (method, uri, request_headers, expected) in requests {
        let (status, headers, _) = send_full(&app, method, uri, request_headers, None).await;
        assert_eq!(status, expected, "{method} {uri}");
        for (name, value) in guard::SECURITY_HEADERS {
            assert_eq!(headers.get(name).and_then(|v| v.to_str().ok()), Some(value), "{name} on {method} {uri}");
        }
    }
}

// --- 8. Sensitive access left no trace --------------------------------------------

async fn audit_log(app: &Router) -> Vec<Value> {
    let (status, log) = send(app, "GET", "/api/admin/audit", &[("x-admin-token", ADMIN_TOKEN)], None).await;
    assert_eq!(status, StatusCode::OK, "{log}");
    log.as_array().unwrap().clone()
}

#[tokio::test]
async fn admin_access_is_audited_granted_or_refused() {
    let app = app_with(nullbox_api::Config { trust_proxy: true, ..config() }).await;
    create_ticket(&app, "Export me").await;

    let attacker = [("x-admin-token", "let-me-in-please"), ("x-forwarded-for", "198.51.100.66")];
    assert_eq!(send(&app, "GET", "/api/admin/export", &attacker, None).await.0, StatusCode::FORBIDDEN);
    let ceo = [("x-admin-token", ADMIN_TOKEN), ("x-forwarded-for", "203.0.113.10")];
    assert_eq!(send(&app, "GET", "/api/admin/export", &ceo, None).await.0, StatusCode::OK);

    let log = audit_log(&app).await;
    let find = |kind: &str| log.iter().find(|entry| entry["kind"] == kind).unwrap_or_else(|| panic!("no {kind}"));
    let denied = find("admin_access_denied");
    assert_eq!(denied["client_ip"], "198.51.100.66");
    assert_eq!(denied["detail"], "GET /api/admin/export: wrong token");
    let export = find("admin_export");
    assert_eq!((&export["client_ip"], &export["detail"]), (&json!("203.0.113.10"), &json!("1 tickets")));

    // Neither the tried token nor the real one is ever written down.
    let dump = Value::Array(log).to_string();
    assert!(!dump.contains("let-me-in-please") && !dump.contains(ADMIN_TOKEN));
}

#[tokio::test]
async fn the_audit_log_itself_needs_the_admin_token() {
    let app = app().await;
    for headers in [&[][..], &[("x-admin-token", "nope")][..], &[WORKER][..]] {
        let (status, body) = send(&app, "GET", "/api/admin/audit", headers, None).await;
        assert_eq!((status, &body["code"]), (StatusCode::FORBIDDEN, &json!("forbidden")));
    }
}

#[tokio::test]
async fn ticket_changes_are_audited_with_their_actor() {
    let app = app().await;
    let (id, _) = create_ticket(&app, "Yeet me").await;
    let (status, _) = send(&app, "POST", &format!("/api/tickets/{id}/yeet"), &[WORKER], None).await;
    assert_eq!(status, StatusCode::OK);

    // Domain events are written in the background, so give them a moment.
    let mut log = Vec::new();
    for _ in 0..100 {
        log = audit_log(&app).await;
        if log.iter().any(|entry| entry["kind"] == "ticket_yeeted") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let yeeted = log.iter().find(|entry| entry["kind"] == "ticket_yeeted").expect("ticket_yeeted in the audit log");
    assert_eq!((&yeeted["actor"], &yeeted["ticket_id"]), (&json!("山田"), &json!(id)));
    assert!(log.iter().any(|entry| entry["kind"] == "ticket_created" && entry["ticket_id"] == json!(id)));
}
