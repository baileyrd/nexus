//! End-to-end sync test: two independent memory stores (two "nodes") converge
//! through a real `nexus-memory-hub` over HTTP.

use std::sync::Arc;

use nexus_memory::core_plugin::{MemoryCorePlugin, HANDLER_ADD, HANDLER_LIST, HANDLER_SYNC};
use nexus_memory::db::MemoryDb;
use nexus_memory_hub::{AppState, HubStore};
use nexus_plugins::CorePlugin;
use serde_json::{json, Value};

const SECRET: &str = "convergence-secret";

/// Spin up an in-memory hub on an ephemeral port; return its base URL.
async fn spawn_hub() -> String {
    let state = AppState {
        store: HubStore::open_in_memory().expect("hub store"),
        secret: Arc::new(SECRET.to_string()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        nexus_memory_hub::serve(listener, state).await.expect("serve");
    });
    format!("http://{addr}")
}

/// Drive a plugin's async `sync` handler to completion.
async fn run_sync(plugin: &mut MemoryCorePlugin, hub: &str, node: &str) -> Value {
    let fut = plugin
        .dispatch_async(
            HANDLER_SYNC,
            &json!({ "hub_url": hub, "secret": SECRET, "node_id": node }),
        )
        .expect("sync must be async");
    fut.await.expect("sync ok")
}

#[tokio::test]
async fn two_nodes_converge_through_the_hub() {
    let hub = spawn_hub().await;

    // Node A and Node B each have their own store.
    let mut a = MemoryCorePlugin::with_db(MemoryDb::open_in_memory().unwrap());
    let mut b = MemoryCorePlugin::with_db(MemoryDb::open_in_memory().unwrap());

    // A stores a memory; B stores a different one.
    a.dispatch(HANDLER_ADD, &json!({ "content": "from node A", "category": "ops" })).unwrap();
    b.dispatch(HANDLER_ADD, &json!({ "content": "from node B", "category": "prefs" })).unwrap();

    // A pushes its memory (pulls nothing yet).
    let a1 = run_sync(&mut a, &hub, "node-a").await;
    assert_eq!(a1["pushed"], 1);
    assert_eq!(a1["pulled"], 0);

    // B pushes its own and pulls A's.
    let b1 = run_sync(&mut b, &hub, "node-b").await;
    assert_eq!(b1["pushed"], 1);
    assert_eq!(b1["pulled"], 1);

    // B now has both memories.
    let b_list = b.dispatch(HANDLER_LIST, &json!({ "limit": 50 })).unwrap();
    let b_contents: Vec<&str> = b_list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(b_contents.contains(&"from node A"), "B should have A's memory: {b_contents:?}");
    assert!(b_contents.contains(&"from node B"));

    // A syncs again and pulls B's memory → both nodes converged.
    let a2 = run_sync(&mut a, &hub, "node-a").await;
    assert_eq!(a2["pulled"], 1);
    let a_list = a.dispatch(HANDLER_LIST, &json!({ "limit": 50 })).unwrap();
    let a_contents: Vec<&str> = a_list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(a_contents.contains(&"from node A"));
    assert!(a_contents.contains(&"from node B"), "A should have B's memory: {a_contents:?}");

    // Idempotent: a third sync with no local changes pushes/pulls nothing new.
    let a3 = run_sync(&mut a, &hub, "node-a").await;
    assert_eq!(a3["pushed"], 0);
    assert_eq!(a3["pulled"], 0);
}
