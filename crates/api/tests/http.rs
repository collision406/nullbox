//! HTTP tests: the whole router against in-memory SQLite, via `oneshot`.
//! Copy a test, change the requests, done.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use nullbox_api::{AppState, Config, db, http};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn app_with_admin_token(admin_token: &str) -> Router {
    let pool = db::connect_in_memory().await.expect("in-memory db");
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: "sqlite::memory:".into(),
        cors_allowed_origins: vec!["http://localhost:5173".into()],
        public_web_url: "http://localhost:5173".into(),
        admin_token: admin_token.into(),
    };
    http::router(AppState::new(pool, config))
}

async fn app() -> Router {
    app_with_admin_token("test-admin-token").await
}

/// Send a request; returns status and JSON body (`Null` if empty).
async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let body = match body {
        Some(json) => {
            req = req.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

const WORKER: (&str, &str) = ("x-actor-name", "%E5%B1%B1%E7%94%B0"); // 山田, percent-encoded

#[tokio::test]
async fn customer_and_worker_round_trip() {
    let app = app().await;

    // Anonymous customer creates a ticket.
    let (status, created) = send(
        &app,
        "POST",
        "/api/tickets",
        &[],
        Some(json!({ "subject": "Printer on fire", "body": "Again.", "severity": "p1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["ticket"]["id"].as_str().unwrap().to_owned();
    let token = created["access_token"].as_str().unwrap().to_owned();
    assert_eq!(created["ticket"]["number"], 1);
    assert!(created["ticket"]["anon"]["animal"].is_string());
    assert!(created["access_link"].as_str().unwrap().ends_with(&format!("/t/{id}#{token}")));

    // The secret link works; a wrong token looks like a missing ticket.
    let (status, thread) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", &token)], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(thread["ticket"]["subject"], "Printer on fire");
    let (status, error) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", "nope")], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::NOT_FOUND, &json!("not_found")));

    // The worker sees it in the inbox, notes internally and yeets it.
    let (_, inbox) = send(&app, "GET", "/api/tickets", &[WORKER], None).await;
    assert_eq!(inbox.as_array().unwrap().len(), 1);
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/tickets/{id}/messages"),
        &[WORKER],
        Some(json!({ "body": "it's always DNS", "internal": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, yeeted) = send(&app, "POST", &format!("/api/tickets/{id}/yeet"), &[WORKER], None).await;
    assert_eq!((status, &yeeted["status"]), (StatusCode::OK, &json!("resolved")));

    let (_, stats) = send(&app, "GET", "/api/stats/backlog", &[WORKER], None).await;
    assert_eq!(stats, json!({ "open_count": 0, "beast_mood": "asleep", "oldest_open_age_secs": null }));

    // The customer never sees internal notes, and replying reopens the ticket.
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/public/tickets/{id}/messages"),
        &[("x-ticket-token", &token)],
        Some(json!({ "body": "Still on fire" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, thread) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", &token)], None).await;
    assert_eq!(thread["ticket"]["status"], "open");
    let messages = thread["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["internal"] == false));
    assert!(
        messages
            .iter()
            .any(|m| m["system_event"] == json!({ "kind": "status_changed", "from": "resolved", "to": "open" }))
    );

    // The worker view does include the note, written by 山田.
    let (_, thread) = send(&app, "GET", &format!("/api/tickets/{id}"), &[WORKER], None).await;
    let note = thread["messages"].as_array().unwrap().iter().find(|m| m["internal"] == true).unwrap();
    assert_eq!(note["author_name"], "山田");
}

#[tokio::test]
async fn errors_share_one_shape() {
    let app = app().await;

    let (status, error) = send(&app, "GET", "/api/tickets", &[], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::UNAUTHORIZED, &json!("missing_actor")));

    let (status, error) =
        send(&app, "POST", "/api/tickets", &[], Some(json!({ "subject": " ", "body": "x", "severity": "p4" }))).await;
    assert_eq!((status, &error["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("validation_failed")));

    let (status, error) = send(&app, "GET", "/api/tickets/not-a-uuid", &[WORKER], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::NOT_FOUND, &json!("not_found")));

    let (status, error) = send(&app, "GET", "/api/teapot", &[], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::IM_A_TEAPOT, &json!("im_a_teapot")));
}

#[tokio::test]
async fn admin_export_needs_the_admin_token() {
    let app = app().await;
    let ticket = json!({ "subject": "Export me", "body": "x", "severity": "p3", "customer_email": "a@example.com" });
    send(&app, "POST", "/api/tickets", &[], Some(ticket)).await;

    let (status, error) = send(&app, "GET", "/api/admin/export", &[], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::FORBIDDEN, &json!("forbidden")));
    let (status, _) = send(&app, "GET", "/api/admin/export", &[("x-admin-token", "nope")], None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let admin = [("x-admin-token", "test-admin-token")];
    let (status, tickets) = send(&app, "GET", "/api/admin/export", &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tickets[0]["customer_email"], "a@example.com");
}

#[tokio::test]
async fn admin_export_is_disabled_without_a_configured_token() {
    let app = app_with_admin_token("").await;

    // An empty header must not match an empty setting.
    let (status, error) = send(&app, "GET", "/api/admin/export", &[("x-admin-token", "")], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::FORBIDDEN, &json!("forbidden")));
}

#[tokio::test]
async fn request_id_is_propagated_and_generated() {
    let app = app().await;

    // Preserves incoming X-Request-Id from Caddy / caller.
    let req = Request::builder()
        .method("GET")
        .uri("/api/healthz")
        .header("x-request-id", "caddy-req-12345")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers().get("x-request-id").unwrap(), "caddy-req-12345");

    // Generates a UUIDv7 X-Request-Id when omitted.
    let req = Request::builder().method("GET").uri("/api/healthz").body(Body::empty()).unwrap();
    let res = app.oneshot(req).await.unwrap();
    let generated = res.headers().get("x-request-id").unwrap().to_str().unwrap();
    assert!(!generated.is_empty());
}

#[tokio::test]
async fn stopped_database_flips_readiness_without_killing_liveness() {
    let pool = db::connect_in_memory().await.expect("in-memory db");
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: "sqlite::memory:".into(),
        cors_allowed_origins: vec!["http://localhost:5173".into()],
        public_web_url: "http://localhost:5173".into(),
        admin_token: "test-admin-token".into(),
    };
    let app = http::router(AppState::new(pool.clone(), config));

    let (status, ready) = send(&app, "GET", "/api/readyz", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ready["status"], "ok");
    assert_eq!(ready["db"], "ok");

    // Stop the database pool: readiness must flip to 503 while liveness stays 200.
    pool.close().await;

    let (ready_status, ready_err) = send(&app, "GET", "/api/readyz", &[], None).await;
    assert_eq!((ready_status, &ready_err["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("service_unavailable")));

    let (live_status, live_body) = send(&app, "GET", "/api/healthz", &[], None).await;
    assert_eq!((live_status, &live_body["status"]), (StatusCode::OK, &json!("ok")));

    // Readiness failure also records an alert on the alert route.
    let (alert_status, alerts) = send(&app, "GET", "/api/alerts", &[], None).await;
    assert_eq!(alert_status, StatusCode::OK);
    assert!(alerts.as_array().unwrap().iter().any(|a| a["source"] == "readyz"));
}

#[tokio::test]
async fn metrics_and_observability_summary_track_traffic_errors_and_alerts() {
    let app = app().await;

    // Generate traffic: create a ticket (201) and hit teapot (418).
    send(
        &app,
        "POST",
        "/api/tickets",
        &[],
        Some(json!({ "subject": "Metrics test", "body": "Body", "severity": "p2" })),
    )
    .await;
    send(&app, "GET", "/api/teapot", &[], None).await;

    // Trigger an external uptime alert via the webhook route.
    let (alert_status, alert) = send(
        &app,
        "POST",
        "/api/alerts/webhook",
        &[],
        Some(json!({
            "title": "Synthetic probe warning",
            "message": "High latency from Tokyo probe",
            "severity": "warning",
            "source": "uptime-check"
        })),
    )
    .await;
    assert_eq!(alert_status, StatusCode::CREATED);
    assert_eq!(alert["source"], "uptime-check");

    // Check structured JSON summary for the dashboard.
    let (status, summary) = send(&app, "GET", "/api/observability/summary", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(summary["db_ready"], true);
    assert!(summary["db_size_bytes"].as_u64().unwrap() > 0);
    assert!(summary["process_rss_bytes"].as_u64().unwrap() > 0);
    assert!(summary["total_requests"].as_u64().unwrap() >= 3);
    assert!(summary["total_errors_4xx"].as_u64().unwrap() >= 1);
    assert_eq!(summary["domain_events"]["ticket_created"], 1);
    assert!(!summary["recent_events"].as_array().unwrap().is_empty());
    assert!(!summary["recent_alerts"].as_array().unwrap().is_empty());

    // Check Prometheus text exposition at /api/metrics.
    let req = Request::builder().method("GET").uri("/api/metrics").body(Body::empty()).unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let text = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(text.contains("nullbox_http_requests_total"));
    assert!(text.contains("nullbox_http_errors_total"));
    assert!(text.contains("nullbox_http_request_duration_seconds_bucket"));
    assert!(text.contains("nullbox_db_size_bytes"));
    assert!(text.contains("nullbox_process_resident_memory_bytes"));
    assert!(text.contains("nullbox_domain_events_total{kind=\"ticket_created\"} 1"));
}
