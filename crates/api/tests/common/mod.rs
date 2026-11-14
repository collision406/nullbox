//! Shared by the HTTP test files: the whole router against in-memory SQLite, via `oneshot`.
#![allow(dead_code)] // each test file uses a different subset

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use nullbox_api::{AppState, Config, db, http};
use serde_json::Value;
use tower::ServiceExt;

pub const ADMIN_TOKEN: &str = "test-admin-token-0123456789abcdefghij";
/// Strong enough for the password policy: long, not a common word, not in a breach list.
pub const PASSWORD: &str = "T7#pLm9!vQ2z-correct";

/// A signed-up worker: send `header()` with a request to act as them.
pub struct Worker {
    pub name: String,
    pub token: String,
    auth: String,
}

impl Worker {
    pub fn header(&self) -> (&str, &str) {
        ("authorization", self.auth.as_str())
    }
}

/// Creates an account through the API and returns its session.
pub async fn worker(app: &Router, name: &str) -> Worker {
    let body = serde_json::json!({ "username": name, "password": PASSWORD });
    let (status, session) = send(app, "POST", "/api/auth/signup", &[], Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let token = session["token"].as_str().unwrap().to_owned();
    Worker { name: session["name"].as_str().unwrap().to_owned(), auth: format!("Bearer {token}"), token }
}

/// Local defaults plus an admin token. Rate limits are off unless a test turns them on.
pub fn config() -> Config {
    Config {
        admin_token: Some(ADMIN_TOKEN.into()),
        rate_limit_per_minute: 0,
        rate_limit_create_per_minute: 0,
        rate_limit_auth_per_minute: 0,
        // Nothing listens here: the breach check fails open, so tests never reach the internet.
        hibp_api_base: "http://127.0.0.1:1".into(),
        ..Config::local()
    }
}

pub async fn app() -> Router {
    app_with(config()).await
}

pub async fn app_with(config: Config) -> Router {
    let pool = db::connect_in_memory().await.expect("in-memory db");
    http::router(AppState::new(pool, config))
}

/// Send a request; returns status and JSON body (`Null` if empty).
pub async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, body) = send_full(app, method, uri, headers, body).await;
    (status, body)
}

/// Like `send`, with the response headers.
pub async fn send_full(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
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
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Creates a ticket as an anonymous customer; returns `(id, access_token)`.
pub async fn create_ticket(app: &Router, subject: &str) -> (String, String) {
    let body = serde_json::json!({ "subject": subject, "body": "details", "severity": "p3" });
    let (status, created) = send(app, "POST", "/api/tickets", &[], Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (created["ticket"]["id"].as_str().unwrap().to_owned(), created["access_token"].as_str().unwrap().to_owned())
}
