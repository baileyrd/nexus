//! End-to-end test for the BL-074 editor wiring: dispatch through
//! `EditorCorePlugin` (the same path the kernel takes in production),
//! verify a CRDT op envelope fires on the bus, verify the persistence
//! file lands on disk on close.

use std::sync::Arc;

use nexus_bootstrap::crdt_publisher::CrdtPublisher;
use nexus_crdt::{crdt_state_path, ops_topic, OpEnvelope};
use nexus_editor::core_plugin::{
    EditorCorePlugin, EditorSnapshot, HANDLER_APPLY_TRANSACTION, HANDLER_CLOSE, HANDLER_OPEN,
};
use nexus_editor::{Operation, Transaction, TransactionMetadata};
use nexus_kernel::{EventBus, EventFilter, NexusEvent};
use nexus_plugins::CorePlugin;

fn write_md(forge: &std::path::Path, relpath: &str, content: &str) {
    let abs = forge.join(relpath);
    std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
    std::fs::write(abs, content).unwrap();
}

#[tokio::test]
async fn end_to_end_publishes_op_and_persists_state() {
    let dir = tempfile::tempdir().unwrap();
    let forge = dir.path().to_path_buf();
    write_md(&forge, "notes.md", "Hello");

    let bus = Arc::new(EventBus::new(64));
    let mut sub = bus.subscribe(EventFilter::CustomExact(ops_topic("notes.md")));

    let mut plugin = EditorCorePlugin::with_event_bus(forge.clone(), Arc::clone(&bus));
    let publisher = Arc::new(CrdtPublisher::new(forge.clone(), Arc::clone(&bus)));
    plugin.set_op_observer(publisher.clone());
    plugin.on_init().unwrap();

    // Open the file. This triggers on_session_opened in the publisher.
    let snap: EditorSnapshot = serde_json::from_value(
        plugin
            .dispatch(HANDLER_OPEN, &serde_json::json!({ "relpath": "notes.md" }))
            .unwrap(),
    )
    .unwrap();
    let para_id = snap.tree.root_blocks[0];
    let content_len = snap.tree.blocks[&para_id].content.len();

    // Apply a single transaction with one InsertText. The publisher
    // should fire a com.nexus.editor.ops.notes.md event.
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: para_id,
            pos: content_len,
            text: " world".into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    plugin
        .dispatch(
            HANDLER_APPLY_TRANSACTION,
            &serde_json::json!({
                "relpath": "notes.md",
                "transaction": serde_json::to_value(&tx).unwrap(),
            }),
        )
        .unwrap();

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
        .await
        .expect("event must arrive within 2s")
        .expect("non-error");
    let payload = match &event.event {
        NexusEvent::Custom {
            type_id, payload, ..
        } => {
            assert_eq!(type_id, "com.nexus.editor.ops.notes.md");
            payload.clone()
        }
        other => panic!("expected Custom event, got {other:?}"),
    };
    let envelope = OpEnvelope::from_json(&payload).expect("envelope decodes");
    assert_eq!(envelope.op.id.site, publisher.site());
    // Phase 2: the wire op carries an RGA translation per char.
    assert_eq!(envelope.op.rga_ops.len(), " world".chars().count());

    // Close the file. The publisher should write the persistence file.
    plugin
        .dispatch(HANDLER_CLOSE, &serde_json::json!({ "relpath": "notes.md" }))
        .unwrap();

    let state_path = forge.join(crdt_state_path("notes.md"));
    assert!(
        state_path.exists(),
        "expected persistence file at {}",
        state_path.display()
    );
    let bytes = std::fs::read(&state_path).unwrap();
    let envelope: nexus_crdt::PersistedCrdt =
        serde_json::from_slice(&bytes).expect("on-disk envelope decodes");
    assert_eq!(envelope.version, nexus_crdt::PERSISTED_VERSION);
    assert_eq!(envelope.state.log.len(), 1, "log captures one op");
    assert_eq!(envelope.state.site, publisher.site());
}
