//! HTTP tests: the whole router against in-memory SQLite, via `oneshot`.
//! Copy a test, change the requests, done.

mod common;

use axum::http::StatusCode;
use common::{ADMIN_TOKEN, app, send, worker};
use serde_json::json;

#[tokio::test]
async fn customer_and_worker_round_trip() {
    let app = app().await;
    let w = worker(&app, "山田太郎").await;

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
    let (_, inbox) = send(&app, "GET", "/api/tickets", &[w.header()], None).await;
    assert_eq!(inbox.as_array().unwrap().len(), 1);
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/tickets/{id}/messages"),
        &[w.header()],
        Some(json!({ "body": "it's always DNS", "internal": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, yeeted) = send(&app, "POST", &format!("/api/tickets/{id}/yeet"), &[w.header()], None).await;
    assert_eq!((status, &yeeted["status"]), (StatusCode::OK, &json!("resolved")));

    let (_, stats) = send(&app, "GET", "/api/stats/backlog", &[w.header()], None).await;
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

    // The worker view does include the note, written by 山田太郎.
    let (_, thread) = send(&app, "GET", &format!("/api/tickets/{id}"), &[w.header()], None).await;
    let note = thread["messages"].as_array().unwrap().iter().find(|m| m["internal"] == true).unwrap();
    assert_eq!(note["author_name"], "山田太郎");
}

#[tokio::test]
async fn errors_share_one_shape() {
    let app = app().await;
    let w = worker(&app, "yamada").await;

    let (status, error) = send(&app, "GET", "/api/tickets", &[], None).await;
    assert_eq!((status, &error["code"]), (StatusCode::UNAUTHORIZED, &json!("missing_actor")));

    let (status, error) =
        send(&app, "POST", "/api/tickets", &[], Some(json!({ "subject": " ", "body": "x", "severity": "p4" }))).await;
    assert_eq!((status, &error["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("validation_failed")));

    let (status, error) = send(&app, "GET", "/api/tickets/not-a-uuid", &[w.header()], None).await;
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

    let admin = [("x-admin-token", ADMIN_TOKEN)];
    let (status, tickets) = send(&app, "GET", "/api/admin/export", &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tickets[0]["customer_email"], "a@example.com");
}
