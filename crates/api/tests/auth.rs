//! Accounts and sessions: what HOT and a judge will try. Nothing here reaches the internet:
//! the breach service is a fake on a local port.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use common::{ADMIN_TOKEN, PASSWORD, app, app_with, config, create_ticket, send, worker};
use nullbox_api::Config;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

fn sha1_hex(password: &str) -> String {
    Sha1::digest(password.as_bytes()).iter().map(|byte| format!("{byte:02X}")).collect()
}

/// What the fake breach service shares between requests: the prefixes it was asked for, and
/// the prefix and suffix of the one password it knows as leaked.
type BreachState = (Arc<Mutex<Vec<String>>>, String, String);

/// A fake Have I Been Pwned that knows `leaked` as a breached password. Returns its base URL
/// and every range prefix it was asked for.
async fn fake_breach_service(leaked: &str) -> (String, Arc<Mutex<Vec<String>>>) {
    let (prefix, suffix) = sha1_hex(leaked).split_at(5).to_owned_pair();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let state = (asked.clone(), prefix, suffix);
    let app = Router::new()
        .route(
            "/range/{prefix}",
            get(|State((asked, known_prefix, suffix)): State<BreachState>, Path(prefix): Path<String>| async move {
                asked.lock().unwrap().push(prefix.clone());
                // Padding lines (count 0) are what the real service adds when asked to.
                let mut body = String::from("0000000000000000000000000000000000A:0\r\n");
                if prefix == known_prefix {
                    body.push_str(&format!("{suffix}:3861\r\n"));
                }
                body
            }),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, asked)
}

trait SplitPair {
    fn to_owned_pair(self) -> (String, String);
}

impl SplitPair for (&str, &str) {
    fn to_owned_pair(self) -> (String, String) {
        (self.0.to_owned(), self.1.to_owned())
    }
}

async fn signup(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    send(app, "POST", "/api/auth/signup", &[], Some(json!({ "username": username, "password": password }))).await
}

async fn login(app: &Router, username: &str, password: &str) -> (StatusCode, Value) {
    send(app, "POST", "/api/auth/login", &[], Some(json!({ "username": username, "password": password }))).await
}

// --- "members-only page stays closed" -----------------------------------------------

#[tokio::test]
async fn the_worker_api_is_closed_to_everyone_without_a_session() {
    let app = app().await;
    let (id, customer_token) = create_ticket(&app, "Private").await;
    let real_worker = worker(&app, "yamada").await;
    // A session that has ended is as good as none.
    let ended = worker(&app, "tanaka").await;
    let logged_out = send(&app, "POST", "/api/auth/logout", &[ended.header()], None).await.0;
    assert_eq!(logged_out, StatusCode::NO_CONTENT);

    let bearer = |token: &str| format!("Bearer {token}");
    let made_up = bearer("made-up-token-0123456789abcdefghijklmnopq");
    let customer = bearer(&customer_token);
    let admin = bearer(ADMIN_TOKEN);
    let ended_session = bearer(&ended.token);
    let attempts: Vec<(&str, Vec<(&str, &str)>)> = vec![
        ("no credentials", vec![]),
        // The retired header: whoever sends a name gets nothing.
        ("X-Actor-Name only", vec![("x-actor-name", "%E5%B1%B1%E7%94%B0")]),
        ("X-Actor-Name of a real worker", vec![("x-actor-name", "yamada")]),
        ("a made-up token", vec![("authorization", made_up.as_str())]),
        ("a customer's ticket token", vec![("authorization", customer.as_str())]),
        ("the admin token", vec![("authorization", admin.as_str())]),
        ("a session that was logged out", vec![("authorization", ended_session.as_str())]),
        ("another scheme", vec![("authorization", "Basic eWFtYWRhOnBhc3N3b3Jk")]),
        ("an empty bearer", vec![("authorization", "Bearer ")]),
    ];
    let routes: [(&str, String, Option<Value>); 7] = [
        ("GET", "/api/tickets".into(), None),
        ("GET", format!("/api/tickets/{id}"), None),
        ("POST", format!("/api/tickets/{id}/messages"), Some(json!({ "body": "hi" }))),
        ("PATCH", format!("/api/tickets/{id}"), Some(json!({ "status": "closed" }))),
        ("POST", format!("/api/tickets/{id}/yeet"), None),
        ("GET", "/api/stats/backlog".into(), None),
        ("GET", "/api/me".into(), None),
    ];
    for (label, headers) in &attempts {
        for (method, uri, body) in &routes {
            let (status, error) = send(&app, method, uri, headers, body.clone()).await;
            assert_eq!(
                (status, &error["code"]),
                (StatusCode::UNAUTHORIZED, &json!("missing_actor")),
                "{method} {uri} with {label}"
            );
            assert!(!error.to_string().contains("Private"), "{method} {uri} with {label} leaked data");
        }
    }

    // The ticket was never touched by any of that, and a real session works.
    let (status, thread) = send(&app, "GET", &format!("/api/tickets/{id}"), &[real_worker.header()], None).await;
    assert_eq!((status, &thread["ticket"]["status"]), (StatusCode::OK, &json!("open")));
}

#[tokio::test]
async fn the_customer_side_stays_open_without_a_login() {
    let app = app().await;
    let (id, token) = create_ticket(&app, "Hello").await;
    let (status, _) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", &token)], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(send(&app, "GET", "/api/auth/config", &[], None).await.0, StatusCode::OK);
}

// --- username and password -----------------------------------------------------------

#[tokio::test]
async fn sign_up_log_in_reply_under_your_own_name_and_log_out() {
    let app = app().await;
    let (id, customer_token) = create_ticket(&app, "Where is my parcel").await;

    let (status, session) = signup(&app, "Yamada", PASSWORD).await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    assert_eq!(session["name"], "Yamada");
    assert!(session["token"].as_str().unwrap().len() >= 40);
    assert!(session["expires_at"].is_string());
    let first = format!("Bearer {}", session["token"].as_str().unwrap());

    let (status, me) = send(&app, "GET", "/api/me", &[("authorization", &first)], None).await;
    assert_eq!((status, &me["name"]), (StatusCode::OK, &json!("Yamada")));

    // The reply carries the account's name, whatever a client claims in a header.
    let reply = Some(json!({ "body": "It is on its way." }));
    let headers = [("authorization", first.as_str()), ("x-actor-name", "Someone Else")];
    let (status, message) = send(&app, "POST", &format!("/api/tickets/{id}/messages"), &headers, reply).await;
    assert_eq!((status, &message["author_name"]), (StatusCode::CREATED, &json!("Yamada")));
    let (_, thread) =
        send(&app, "GET", &format!("/api/public/tickets/{id}"), &[("x-ticket-token", &customer_token)], None).await;
    assert_eq!(thread["messages"][0]["author_name"], "Yamada");

    // Logging out locks that session out for good.
    assert_eq!(
        send(&app, "POST", "/api/auth/logout", &[("authorization", &first)], None).await.0,
        StatusCode::NO_CONTENT
    );
    for _ in 0..2 {
        let (status, _) = send(&app, "GET", "/api/tickets", &[("authorization", &first)], None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        send(&app, "POST", "/api/auth/logout", &[("authorization", &first)], None).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Logging in again (any capitalisation of the name) gives a new, working session.
    let (status, again) = login(&app, "yamada", PASSWORD).await;
    assert_eq!((status, &again["name"]), (StatusCode::OK, &json!("Yamada")));
    assert_ne!(again["token"], session["token"]);
    let second = format!("Bearer {}", again["token"].as_str().unwrap());
    assert_eq!(send(&app, "GET", "/api/tickets", &[("authorization", &second)], None).await.0, StatusCode::OK);
    // ...and the old one is still dead.
    assert_eq!(send(&app, "GET", "/api/tickets", &[("authorization", &first)], None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logging_out_only_ends_that_session() {
    let app = app().await;
    signup(&app, "yamada", PASSWORD).await;
    let phone = login(&app, "yamada", PASSWORD).await.1;
    let laptop = login(&app, "yamada", PASSWORD).await.1;
    let (phone, laptop) = (
        format!("Bearer {}", phone["token"].as_str().unwrap()),
        format!("Bearer {}", laptop["token"].as_str().unwrap()),
    );
    send(&app, "POST", "/api/auth/logout", &[("authorization", &phone)], None).await;
    assert_eq!(send(&app, "GET", "/api/me", &[("authorization", &phone)], None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "GET", "/api/me", &[("authorization", &laptop)], None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_user_look_exactly_the_same() {
    let app = app().await;
    signup(&app, "yamada", PASSWORD).await;

    let wrong = login(&app, "yamada", "not the password at all").await;
    let unknown = login(&app, "nobody-here", "not the password at all").await;
    let empty = login(&app, "yamada", "").await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong, unknown, "the answer must not reveal which usernames exist");
    assert_eq!((empty.0, &empty.1["code"]), (StatusCode::UNAUTHORIZED, &json!("invalid_credentials")));
    assert!(!wrong.1.to_string().contains("not the password"));
}

#[tokio::test]
async fn short_common_and_leaked_passwords_are_refused_at_sign_up() {
    let leaked = "Summer-holiday-2019!";
    let (breaches, asked) = fake_breach_service(leaked).await;
    let app = app_with(Config { hibp_api_base: breaches, ..config() }).await;

    let cases = [
        ("too short", "Tr0ub4d!", "password_too_short"),
        ("common", "password123456", "password_too_weak"),
        ("the username", "yamada-yamada", "password_too_weak"),
        ("found in a breach", leaked, "password_leaked"),
    ];
    for (label, password, code) in cases {
        let (status, error) = signup(&app, "yamada", password).await;
        assert_eq!((status, &error["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!(code)), "{label}");
        assert!(!error.to_string().contains(password), "{label}: the password is echoed back");
    }
    // None of those created an account.
    assert_eq!(login(&app, "yamada", leaked).await.0, StatusCode::UNAUTHORIZED);

    // A strong password goes through, and the breach service only ever saw five hex characters.
    assert_eq!(signup(&app, "yamada", PASSWORD).await.0, StatusCode::CREATED);
    let asked = asked.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(asked.iter().all(|prefix| prefix.len() == 5 && prefix.chars().all(|c| c.is_ascii_hexdigit())), "{asked:?}");
    assert!(!asked.contains(&sha1_hex(PASSWORD)));
}

#[tokio::test]
async fn an_unreachable_breach_service_does_not_block_sign_up() {
    // `config()` points the breach check at a closed port.
    let app = app().await;
    assert_eq!(signup(&app, "yamada", PASSWORD).await.0, StatusCode::CREATED);
    // The local rules still apply without it.
    assert_eq!(signup(&app, "tanaka", "password123456").await.1["code"], "password_too_weak");
}

#[tokio::test]
async fn usernames_are_validated_and_unique() {
    let app = app().await;
    assert_eq!(signup(&app, "Yamada", PASSWORD).await.0, StatusCode::CREATED);
    let (status, error) = signup(&app, "YAMADA", PASSWORD).await;
    assert_eq!((status, &error["code"]), (StatusCode::CONFLICT, &json!("username_taken")));

    for bad in ["", "ab", &"x".repeat(33), "a b c", "<script>", "yamada@example.com"] {
        let (status, error) = signup(&app, bad, PASSWORD).await;
        assert_eq!(
            (status, &error["code"]),
            (StatusCode::UNPROCESSABLE_ENTITY, &json!("validation_failed")),
            "{bad:?}"
        );
    }
    // Names in any script are fine.
    assert_eq!(signup(&app, "山田太郎", PASSWORD).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn an_invite_code_closes_sign_up_when_one_is_set() {
    let app = app_with(Config { signup_invite_code: Some("let-us-in-2026".into()), ..config() }).await;
    let (_, auth_config) = send(&app, "GET", "/api/auth/config", &[], None).await;
    assert_eq!(auth_config["invite_required"], true);

    for body in [json!({}), json!({ "invite_code": "wrong-code-here" }), json!({ "invite_code": "" })] {
        let mut request = json!({ "username": "yamada", "password": PASSWORD });
        request.as_object_mut().unwrap().extend(body.as_object().unwrap().clone());
        let (status, error) = send(&app, "POST", "/api/auth/signup", &[], Some(request)).await;
        assert_eq!((status, &error["code"]), (StatusCode::FORBIDDEN, &json!("forbidden")));
    }
    let request = json!({ "username": "yamada", "password": PASSWORD, "invite_code": "let-us-in-2026" });
    assert_eq!(send(&app, "POST", "/api/auth/signup", &[], Some(request)).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn guessing_passwords_is_cut_off() {
    let app = app_with(Config { rate_limit_auth_per_minute: 4, ..config() }).await;
    assert_eq!(signup(&app, "yamada", PASSWORD).await.0, StatusCode::CREATED);
    for guess in ["guess-one-here", "guess-two-here", "guess-three-here"] {
        assert_eq!(login(&app, "yamada", guess).await.0, StatusCode::UNAUTHORIZED);
    }
    // The fifth attempt is refused even though the password is right.
    let (status, error) = login(&app, "yamada", PASSWORD).await;
    assert_eq!((status, &error["code"]), (StatusCode::TOO_MANY_REQUESTS, &json!("rate_limited")));
}

#[tokio::test]
async fn account_activity_is_audited_without_secrets() {
    let app = app_with(Config { trust_proxy: true, ..config() }).await;
    let from = [("x-forwarded-for", "198.51.100.9")];
    let body = |password: &str| Some(json!({ "username": "yamada", "password": password }));
    let (_, session) = send(&app, "POST", "/api/auth/signup", &from, body(PASSWORD)).await;
    send(&app, "POST", "/api/auth/login", &from, body("a wrong password 1234")).await;
    let again = send(&app, "POST", "/api/auth/login", &from, body(PASSWORD)).await.1;
    let bearer = format!("Bearer {}", again["token"].as_str().unwrap());
    send(&app, "POST", "/api/auth/logout", &[("authorization", &bearer)], None).await;

    let mut log = Vec::new();
    for _ in 0..50 {
        let (_, entries) = send(&app, "GET", "/api/admin/audit", &[("x-admin-token", ADMIN_TOKEN)], None).await;
        log = entries.as_array().unwrap().clone();
        if log.iter().any(|entry| entry["kind"] == "logout") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let kinds: Vec<&str> = log.iter().map(|entry| entry["kind"].as_str().unwrap()).collect();
    for kind in ["account_created", "login_failed", "login_succeeded", "logout"] {
        assert!(kinds.contains(&kind), "{kind} missing from {kinds:?}");
    }
    let failed = log.iter().find(|entry| entry["kind"] == "login_failed").unwrap();
    assert_eq!((&failed["client_ip"], &failed["detail"]), (&json!("198.51.100.9"), &json!("username: yamada")));

    // Neither passwords nor session tokens are ever written down.
    let dump = Value::Array(log).to_string();
    for secret in
        [PASSWORD, "a wrong password 1234", session["token"].as_str().unwrap(), again["token"].as_str().unwrap()]
    {
        assert!(!dump.contains(secret), "the audit log contains a secret");
    }
}
