//! Single sign-on, against a fake identity provider on a local port. The fake checks what a real
//! one checks (client secret, redirect address, the PKCE verifier against the challenge it saw)
//! and records what it was sent. Nothing here reaches the internet.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use common::{ADMIN_TOKEN, app_with, config, send, send_full};
use nullbox_api::Config;
use nullbox_api::config::SsoConfig;
use nullbox_api::sso::pkce_challenge;
use reqwest::Url;
use serde_json::{Value, json};

const SITE: &str = "https://team-09.example";
const CLIENT_SECRET: &str = "csecret-value-0123456789";
const ACCESS_TOKEN: &str = "fake-access-token-9876543210";

/// The fields of one form posted to the token endpoint.
type Form = Vec<(String, String)>;

#[derive(Clone, Default)]
struct Provider {
    /// The `code_challenge` the browser was sent to, as the provider would have seen it.
    challenge: Arc<Mutex<Option<String>>>,
    /// Every form posted to the token endpoint.
    exchanges: Arc<Mutex<Vec<Form>>>,
    refuse_codes: bool,
}

fn decode(raw: &str) -> String {
    percent_encoding::percent_decode_str(&raw.replace('+', " ")).decode_utf8_lossy().into_owned()
}

fn form_pairs(body: &str) -> Vec<(String, String)> {
    body.split('&').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (decode(k), decode(v))).collect()
}

fn field<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

async fn token_endpoint(State(provider): State<Provider>, body: String) -> (StatusCode, Json<Value>) {
    let form = form_pairs(&body);
    provider.exchanges.lock().unwrap().push(form.clone());
    let expected_challenge = provider.challenge.lock().unwrap().clone();
    let verifier_matches = field(&form, "code_verifier")
        .zip(expected_challenge)
        .is_some_and(|(verifier, challenge)| pkce_challenge(verifier) == challenge);
    let valid = field(&form, "grant_type") == Some("authorization_code")
        && field(&form, "code") == Some("good-code")
        && field(&form, "client_id") == Some("cid")
        && field(&form, "client_secret") == Some(CLIENT_SECRET)
        && field(&form, "redirect_uri") == Some(&format!("{SITE}/api/auth/sso/callback"))
        && verifier_matches;
    if provider.refuse_codes || !valid {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "bad_verification_code" })));
    }
    (StatusCode::OK, Json(json!({ "access_token": ACCESS_TOKEN, "token_type": "bearer" })))
}

async fn userinfo_endpoint(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let authorised =
        headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {ACCESS_TOKEN}"));
    if !authorised || !headers.contains_key("user-agent") {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "message": "Bad credentials" })));
    }
    (StatusCode::OK, Json(json!({ "id": 4711, "login": "hanako-gh", "name": "Hanako Tanaka" })))
}

async fn start_provider(provider: Provider) -> String {
    let router = Router::new()
        .route("/token", post(token_endpoint))
        .route("/userinfo", get(userinfo_endpoint))
        .with_state(provider);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    base
}

fn sso_config(base: &str) -> SsoConfig {
    SsoConfig {
        provider: "fakeidp".into(),
        name: "FakeIdP".into(),
        auth_url: format!("{base}/authorize"),
        token_url: format!("{base}/token"),
        userinfo_url: format!("{base}/userinfo"),
        scope: "read:user".into(),
        client_id: "cid".into(),
        client_secret: CLIENT_SECRET.into(),
        subject_field: "id".into(),
        name_fields: vec!["name".into(), "login".into()],
    }
}

async fn sso_app(provider: Provider) -> (Router, String) {
    let base = start_provider(provider).await;
    let config = Config { public_web_url: SITE.into(), sso: Some(sso_config(&base)), trust_proxy: true, ..config() };
    (app_with(config).await, base)
}

/// What the browser holds after clicking "Sign in with FakeIdP".
struct Started {
    location: Url,
    state: String,
    cookie_header: String,
    set_cookie: String,
}

async fn start(app: &Router, provider: &Provider) -> Started {
    let (status, headers, _) =
        send_full(app, "GET", "/api/auth/sso/login", &[("x-forwarded-for", "203.0.113.7")], None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = Url::parse(headers["location"].to_str().unwrap()).unwrap();
    let param =
        |name: &str| location.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned()).unwrap_or_default();
    *provider.challenge.lock().unwrap() = Some(param("code_challenge"));
    let set_cookie = headers["set-cookie"].to_str().unwrap().to_owned();
    let state = param("state");
    let cookie_header = set_cookie.split(';').next().unwrap().to_owned();
    Started { location, state, cookie_header, set_cookie }
}

async fn callback(app: &Router, query: &str, cookie: Option<&str>) -> (StatusCode, HeaderMap) {
    let mut headers = vec![("x-forwarded-for", "203.0.113.7")];
    if let Some(cookie) = cookie {
        headers.push(("cookie", cookie));
    }
    let (status, response_headers, _) =
        send_full(app, "GET", &format!("/api/auth/sso/callback?{query}"), &headers, None).await;
    (status, response_headers)
}

fn location(headers: &HeaderMap) -> String {
    headers["location"].to_str().unwrap().to_owned()
}

fn session_token(location: &str) -> String {
    location
        .strip_prefix(&format!("{SITE}/sso/done#token="))
        .unwrap_or_else(|| panic!("no token in {location}"))
        .to_owned()
}

// --- starting a sign-in ---------------------------------------------------------------

#[tokio::test]
async fn the_login_screen_is_told_about_the_provider_only_when_one_is_configured() {
    let (app, _) = sso_app(Provider::default()).await;
    let (_, auth_config) = send(&app, "GET", "/api/auth/config", &[], None).await;
    assert_eq!(auth_config["sso"], json!({ "name": "FakeIdP", "login_url": "/api/auth/sso/login" }));

    let plain = common::app().await;
    assert_eq!(send(&plain, "GET", "/api/auth/config", &[], None).await.1["sso"], Value::Null);
    assert_eq!(send(&plain, "GET", "/api/auth/sso/login", &[], None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(send(&plain, "GET", "/api/auth/sso/callback?code=x&state=y", &[], None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn starting_sends_the_browser_to_the_provider_with_pkce_and_a_binding_cookie() {
    let provider = Provider::default();
    let (app, base) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;

    let query = |name: &str| started.location.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    assert_eq!(
        format!("{}://{}{}", started.location.scheme(), started.location.authority(), started.location.path()),
        format!("{base}/authorize")
    );
    assert_eq!(query("response_type").as_deref(), Some("code"));
    assert_eq!(query("client_id").as_deref(), Some("cid"));
    assert_eq!(query("redirect_uri").as_deref(), Some(&*format!("{SITE}/api/auth/sso/callback")));
    assert_eq!(query("scope").as_deref(), Some("read:user"));
    assert_eq!(query("code_challenge_method").as_deref(), Some("S256"));
    assert_eq!(query("code_challenge").unwrap().len(), 43, "a SHA-256 challenge, base64url");
    assert!(started.state.len() >= 40, "an unguessable state");
    // The secret never goes through the browser.
    assert!(!started.location.as_str().contains(CLIENT_SECRET));

    // The cookie ties this sign-in to this browser and is out of reach of scripts and plain HTTP.
    let cookie = &started.set_cookie;
    assert!(cookie.starts_with(&format!("nullbox_sso={}", started.state)), "{cookie}");
    for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/api/auth/sso", "Max-Age=600"] {
        assert!(cookie.contains(attribute), "{attribute} missing from {cookie}");
    }
}

// --- finishing it -----------------------------------------------------------------------

#[tokio::test]
async fn signing_in_gives_a_session_under_the_providers_name_and_signing_out_ends_it() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let (id, customer_token) = common::create_ticket(&app, "Where is my parcel").await;

    let started = start(&app, &provider).await;
    let (status, headers) =
        callback(&app, &format!("code=good-code&state={}", started.state), Some(&started.cookie_header)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let landing = location(&headers);
    // The session token travels in the fragment, which browsers never send to a server.
    assert!(landing.starts_with(&format!("{SITE}/sso/done#token=")) && !landing.contains('?'), "{landing}");
    assert!(headers["set-cookie"].to_str().unwrap().contains("Max-Age=0"), "the cookie is cleared");
    let bearer = format!("Bearer {}", session_token(&landing));

    let (status, me) = send(&app, "GET", "/api/me", &[("authorization", &bearer)], None).await;
    assert_eq!((status, &me["name"]), (StatusCode::OK, &json!("Hanako Tanaka")));

    // The provider was asked exactly what a real one checks: the code, the secret, the redirect
    // address and the PKCE verifier. The verifier itself never went through the browser.
    let exchanges = provider.exchanges.lock().unwrap().clone();
    assert_eq!(exchanges.len(), 1);
    let verifier = field(&exchanges[0], "code_verifier").unwrap();
    assert!(!started.location.as_str().contains(verifier));
    assert_ne!(verifier, pkce_challenge(verifier));

    // A reply is signed with that account's name.
    let reply = Some(json!({ "body": "It is on its way." }));
    let (status, message) =
        send(&app, "POST", &format!("/api/tickets/{id}/messages"), &[("authorization", &bearer)], reply).await;
    assert_eq!((status, &message["author_name"]), (StatusCode::CREATED, &json!("Hanako Tanaka")));
    let (_, thread) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", &customer_token)], None).await;
    assert_eq!(thread["messages"][0]["author_name"], "Hanako Tanaka");

    // Logging out locks the session out again.
    assert_eq!(
        send(&app, "POST", "/api/auth/logout", &[("authorization", &bearer)], None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(send(&app, "GET", "/api/me", &[("authorization", &bearer)], None).await.0, StatusCode::UNAUTHORIZED);

    // Signing in again finds the same account.
    let started = start(&app, &provider).await;
    let (_, headers) =
        callback(&app, &format!("code=good-code&state={}", started.state), Some(&started.cookie_header)).await;
    let bearer = format!("Bearer {}", session_token(&location(&headers)));
    assert_eq!(send(&app, "GET", "/api/me", &[("authorization", &bearer)], None).await.1["name"], "Hanako Tanaka");
}

#[tokio::test]
async fn a_callback_from_another_browser_is_refused_before_anything_is_exchanged() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    let query = format!("code=good-code&state={}", started.state);

    // An attacker's link opened in a victim's browser: no cookie, or somebody else's.
    for cookie in [None, Some("nullbox_sso=not-the-state"), Some("other=1"), Some("nullbox_sso=")] {
        let (status, headers) = callback(&app, &query, cookie).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{cookie:?}");
        assert_eq!(location(&headers), format!("{SITE}/sso/done#error=expired"), "{cookie:?}");
    }
    assert!(provider.exchanges.lock().unwrap().is_empty(), "the code must not even be traded");
    // The refused tries did not burn the state: the real browser can still finish.
    let (_, headers) = callback(&app, &query, Some(&started.cookie_header)).await;
    assert!(location(&headers).contains("#token="));
}

#[tokio::test]
async fn a_state_works_once() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    let query = format!("code=good-code&state={}", started.state);

    let (_, first) = callback(&app, &query, Some(&started.cookie_header)).await;
    assert!(location(&first).contains("#token="));
    // Replaying the very same callback (back button, a copied link) gets nothing.
    let (_, replay) = callback(&app, &query, Some(&started.cookie_header)).await;
    assert_eq!(location(&replay), format!("{SITE}/sso/done#error=expired"));
    assert_eq!(provider.exchanges.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_state_is_refused() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let (_, headers) = callback(&app, "code=good-code&state=made-up-state", Some("nullbox_sso=made-up-state")).await;
    assert_eq!(location(&headers), format!("{SITE}/sso/done#error=expired"));
    assert!(provider.exchanges.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_provider_that_refuses_the_code_signs_nobody_in() {
    let provider = Provider { refuse_codes: true, ..Provider::default() };
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    let (_, headers) =
        callback(&app, &format!("code=good-code&state={}", started.state), Some(&started.cookie_header)).await;
    assert_eq!(location(&headers), format!("{SITE}/sso/done#error=failed"));

    // A wrong code against a provider that does check is refused the same way.
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    let (_, headers) =
        callback(&app, &format!("code=stolen-code&state={}", started.state), Some(&started.cookie_header)).await;
    assert_eq!(location(&headers), format!("{SITE}/sso/done#error=failed"));
    assert_eq!(send(&app, "GET", "/api/tickets", &[], None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn saying_no_at_the_provider_or_a_broken_callback_ends_cleanly() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    let (status, headers) = callback(&app, "error=access_denied&state=x", Some(&started.cookie_header)).await;
    assert_eq!(
        (status, location(&headers).as_str()),
        (StatusCode::SEE_OTHER, &*format!("{SITE}/sso/done#error=denied"))
    );
    for query in ["", "code=only-a-code", "state=only-a-state"] {
        let (_, headers) = callback(&app, query, Some(&started.cookie_header)).await;
        assert_eq!(location(&headers), format!("{SITE}/sso/done#error=failed"), "{query:?}");
    }
    assert!(provider.exchanges.lock().unwrap().is_empty());
}

// --- around it ----------------------------------------------------------------------------

#[tokio::test]
async fn sign_in_activity_is_audited_without_secrets() {
    let provider = Provider::default();
    let (app, _) = sso_app(provider.clone()).await;
    let started = start(&app, &provider).await;
    callback(&app, "error=access_denied&state=x", None).await;
    let (_, headers) =
        callback(&app, &format!("code=good-code&state={}", started.state), Some(&started.cookie_header)).await;
    let session = session_token(&location(&headers));

    let mut log = Vec::new();
    for _ in 0..50 {
        let (_, entries) = send(&app, "GET", "/api/admin/audit", &[("x-admin-token", ADMIN_TOKEN)], None).await;
        log = entries.as_array().unwrap().clone();
        if log.iter().any(|entry| entry["kind"] == "sso_login_succeeded") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let kinds: Vec<&str> = log.iter().map(|entry| entry["kind"].as_str().unwrap()).collect();
    for kind in ["sso_login_started", "sso_login_failed", "sso_login_succeeded"] {
        assert!(kinds.contains(&kind), "{kind} missing from {kinds:?}");
    }
    let done = log.iter().find(|entry| entry["kind"] == "sso_login_succeeded").unwrap();
    assert_eq!((&done["actor"], &done["client_ip"]), (&json!("Hanako Tanaka"), &json!("203.0.113.7")));

    let dump = Value::Array(log).to_string();
    for secret in [CLIENT_SECRET, ACCESS_TOKEN, session.as_str(), started.state.as_str()] {
        assert!(!dump.contains(secret), "the audit log contains a secret");
    }
}

#[tokio::test]
async fn the_sign_in_routes_share_the_login_rate_limit() {
    let provider = Provider::default();
    let base = start_provider(provider).await;
    let config =
        Config { public_web_url: SITE.into(), sso: Some(sso_config(&base)), rate_limit_auth_per_minute: 3, ..config() };
    let app = app_with(config).await;
    for _ in 0..3 {
        assert_eq!(send(&app, "GET", "/api/auth/sso/login", &[], None).await.0, StatusCode::SEE_OTHER);
    }
    let (status, error) = send(&app, "GET", "/api/auth/sso/callback?code=x&state=y", &[], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::TOO_MANY_REQUESTS, &json!("rate_limited")));
}

#[tokio::test]
async fn the_inbox_stays_closed_without_a_session_even_with_sso_on() {
    let (app, _) = sso_app(Provider::default()).await;
    assert_eq!(send(&app, "GET", "/api/tickets", &[], None).await.0, StatusCode::UNAUTHORIZED);
    // A made-up session, and the provider's own access token, open nothing either.
    for bearer in ["Bearer made-up-session-token-0123456789", &format!("Bearer {ACCESS_TOKEN}")] {
        assert_eq!(
            send(&app, "GET", "/api/tickets", &[("authorization", bearer)], None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}
