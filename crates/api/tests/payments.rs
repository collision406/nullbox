//! Stripe Checkout, against a fake Stripe on a local port. Nothing here reaches the internet.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use common::{ADMIN_TOKEN, config, send, send_full};
use nullbox_api::{Config, payments};
use serde_json::{Value, json};

/// A fake key, assembled at run time so that no line of source looks like a real one to a
/// secret scanner (GitHub's push protection blocks those).
fn test_key() -> String {
    ["rk", "test", "fake", "key", "for", "the", "unit", "tests"].join("_")
}

/// What the fake Stripe saw.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(HeaderMap, String)>>>);

impl Seen {
    fn requests(&self) -> Vec<(HeaderMap, String)> {
        self.0.lock().unwrap().clone()
    }
}

/// Starts a fake Stripe that answers every Checkout Session request with `status` and `body`.
async fn fake_stripe(status: StatusCode, body: Value) -> (String, Seen) {
    let seen = Seen::default();
    let state = (seen.clone(), status, body);
    let app =
        Router::new()
            .route(
                "/v1/checkout/sessions",
                post(
                    |State((seen, status, body)): State<(Seen, StatusCode, Value)>,
                     headers: HeaderMap,
                     form: String| async move {
                        seen.0.lock().unwrap().push((headers, form));
                        (status, axum::Json(body))
                    },
                ),
            )
            .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

fn session(id: &str) -> Value {
    json!({ "id": id, "url": format!("https://checkout.stripe.com/c/pay/{id}") })
}

fn with_stripe(base: &str) -> Config {
    Config { stripe_api_key: Some(test_key()), stripe_api_base: base.into(), ..config() }
}

/// The form fields Stripe received, decoded.
fn fields(form: &str) -> Vec<(String, String)> {
    url_decode_pairs(form)
}

fn url_decode_pairs(form: &str) -> Vec<(String, String)> {
    form.split('&').filter_map(|pair| pair.split_once('=')).map(|(key, value)| (decode(key), decode(value))).collect()
}

fn decode(raw: &str) -> String {
    percent_encoding::percent_decode_str(&raw.replace('+', " ")).decode_utf8_lossy().into_owned()
}

fn field<'a>(all: &'a [(String, String)], name: &str) -> Option<&'a str> {
    all.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn checkout_opens_a_stripe_session_the_server_priced() {
    let (base, seen) = fake_stripe(StatusCode::OK, session("cs_test_a1")).await;
    let app = common::app_with(with_stripe(&base)).await;

    // A client trying to choose its own price gets the same price: the body is not read.
    let hostile = Some(json!({ "amount": 1, "currency": "usd", "unit_amount": 1 }));
    let (status, body) = send(&app, "POST", "/api/checkout", &[], hostile).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "url": "https://checkout.stripe.com/c/pay/cs_test_a1" }));

    let requests = seen.requests();
    assert_eq!(requests.len(), 1);
    let (headers, form) = &requests[0];
    assert_eq!(headers["authorization"], format!("Bearer {}", test_key()));
    let sent = fields(form);
    assert_eq!(field(&sent, "mode"), Some("payment"));
    assert_eq!(field(&sent, "line_items[0][quantity]"), Some("1"));
    assert_eq!(field(&sent, "line_items[0][price_data][currency]"), Some("jpy"));
    assert_eq!(field(&sent, "line_items[0][price_data][unit_amount]"), Some("500"));
    assert_eq!(field(&sent, "line_items[0][price_data][product_data][name]"), Some(payments::PRODUCT_NAME));
    assert_eq!(field(&sent, "line_items[0][price_data][product_data][tax_code]"), Some("txcd_10000000"));
    // The buyer comes back to our site, and Stripe fills in the session id.
    assert_eq!(field(&sent, "success_url"), Some("http://localhost:5173/thanks?session_id={CHECKOUT_SESSION_ID}"));
    assert_eq!(field(&sent, "cancel_url"), Some("http://localhost:5173/buy"));
}

#[tokio::test]
async fn checkout_is_unavailable_without_a_stripe_key() {
    let (base, seen) = fake_stripe(StatusCode::OK, session("cs_test_never")).await;
    let app = common::app_with(Config { stripe_api_base: base, ..config() }).await;

    let (status, body) = send(&app, "POST", "/api/checkout", &[], None).await;
    assert_eq!((status, &body["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("service_unavailable")));
    assert!(seen.requests().is_empty(), "nothing is sent to Stripe without a key");
}

#[tokio::test]
async fn a_stripe_failure_is_a_503_that_leaks_nothing() {
    let refusal = json!({ "error": { "message": format!("Invalid API key provided: {}***", &test_key()[..12]) } });
    let (base, _) = fake_stripe(StatusCode::UNAUTHORIZED, refusal).await;
    let app = common::app_with(with_stripe(&base)).await;

    let (status, body) = send(&app, "POST", "/api/checkout", &[], None).await;
    assert_eq!((status, &body["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("service_unavailable")));
    let text = body.to_string();
    assert!(!text.contains("Invalid API key") && !text.contains("rk_test"), "{text}");
}

#[tokio::test]
async fn an_unreachable_stripe_is_a_503_too() {
    // Nothing listens on this port.
    let app = common::app_with(with_stripe("http://127.0.0.1:1")).await;
    let (status, body) = send(&app, "POST", "/api/checkout", &[], None).await;
    assert_eq!((status, &body["code"]), (StatusCode::SERVICE_UNAVAILABLE, &json!("service_unavailable")));
}

#[tokio::test]
async fn only_stripe_checkout_urls_are_handed_to_the_browser() {
    for hostile in ["https://evil.example/pay", "http://checkout.stripe.com/c/pay/x", "javascript:alert(1)"] {
        let (base, _) = fake_stripe(StatusCode::OK, json!({ "id": "cs_test_x", "url": hostile })).await;
        let app = common::app_with(with_stripe(&base)).await;
        let (status, body) = send(&app, "POST", "/api/checkout", &[], None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{hostile}");
        assert!(!body.to_string().contains("evil.example"));
    }
    // A session without any URL is refused too.
    let (base, _) = fake_stripe(StatusCode::OK, json!({ "id": "cs_test_x" })).await;
    let app = common::app_with(with_stripe(&base)).await;
    assert_eq!(send(&app, "POST", "/api/checkout", &[], None).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn checkout_is_rate_limited_like_ticket_creation() {
    let (base, seen) = fake_stripe(StatusCode::OK, session("cs_test_r")).await;
    let app = common::app_with(Config { rate_limit_create_per_minute: 2, ..with_stripe(&base) }).await;

    for _ in 0..2 {
        assert_eq!(send(&app, "POST", "/api/checkout", &[], None).await.0, StatusCode::OK);
    }
    let (status, headers, body) = send_full(&app, "POST", "/api/checkout", &[], None).await;
    assert_eq!((status, &body["code"]), (StatusCode::TOO_MANY_REQUESTS, &json!("rate_limited")));
    assert!(headers.contains_key("retry-after"));
    assert_eq!(seen.requests().len(), 2, "the refused request never reached Stripe");
}

#[tokio::test]
async fn a_started_checkout_is_audited_without_the_key() {
    let (base, _) = fake_stripe(StatusCode::OK, session("cs_test_audit")).await;
    let app = common::app_with(Config { trust_proxy: true, ..with_stripe(&base) }).await;
    let headers = [("x-forwarded-for", "203.0.113.50")];
    assert_eq!(send(&app, "POST", "/api/checkout", &headers, None).await.0, StatusCode::OK);

    let mut entry = None;
    for _ in 0..50 {
        let (_, log) = send(&app, "GET", "/api/admin/audit", &[("x-admin-token", ADMIN_TOKEN)], None).await;
        entry = log.as_array().unwrap().iter().find(|e| e["kind"] == "checkout_started").cloned();
        if entry.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let entry = entry.expect("a checkout_started entry");
    assert_eq!(entry["client_ip"], "203.0.113.50");
    assert_eq!(entry["detail"], "Stripe session cs_test_audit");
    assert!(!entry.to_string().contains(&test_key()));
}
