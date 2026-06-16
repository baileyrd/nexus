//! End-to-end HTTP test: bind the real axum server on an ephemeral port and
//! drive it with `reqwest`, covering routing, bearer auth, and the push/pull
//! JSON surface.

use std::sync::Arc;

use nexus_memory_hub::{router, AppState, HubStore};
use serde_json::{json, Value};

/// Bind the hub on an OS-assigned port and return its base URL + the secret.
async fn spawn_hub() -> (String, &'static str) {
    let secret = "test-secret";
    let state = AppState {
        store: HubStore::open_in_memory().expect("store"),
        secret: Arc::new(secret.to_string()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.expect("serve");
    });
    (format!("http://{addr}"), secret)
}

#[tokio::test]
async fn health_push_pull_and_auth_over_http() {
    let (base, secret) = spawn_hub().await;
    let client = reqwest::Client::new();

    // /health is unauthenticated.
    let health = client.get(format!("{base}/health")).send().await.expect("health");
    assert_eq!(health.status(), 200);
    let body: Value = health.json().await.expect("health json");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["role"], "hub");

    // Push without a bearer token → 401.
    let unauth = client
        .post(format!("{base}/sync/push"))
        .json(&json!({ "node_id": "a", "records": [] }))
        .send()
        .await
        .expect("unauth push");
    assert_eq!(unauth.status(), 401);

    // Authed push of one record.
    let push = client
        .post(format!("{base}/sync/push"))
        .bearer_auth(secret)
        .json(&json!({
            "node_id": "node-a",
            "records": [
                { "id": "m1", "updated_at": "2026-01-01T00:00:00+00:00", "content": "hello hub" }
            ]
        }))
        .send()
        .await
        .expect("push");
    assert_eq!(push.status(), 200);
    let pr: Value = push.json().await.expect("push json");
    assert_eq!(pr["accepted"], 1);
    assert_eq!(pr["processed_ids"][0], "m1");

    // Authed pull from a different node sees the record.
    let pull = client
        .get(format!("{base}/sync/pull?exclude_node=node-b"))
        .bearer_auth(secret)
        .send()
        .await
        .expect("pull");
    assert_eq!(pull.status(), 200);
    let pl: Value = pull.json().await.expect("pull json");
    assert_eq!(pl["count"], 1);
    assert_eq!(pl["records"][0]["content"], "hello hub");

    // The pushing node excludes its own writes.
    let own = client
        .get(format!("{base}/sync/pull?exclude_node=node-a"))
        .bearer_auth(secret)
        .send()
        .await
        .expect("pull own");
    let own_body: Value = own.json().await.expect("pull own json");
    assert_eq!(own_body["count"], 0);
}
