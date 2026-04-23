//! PRD 06 file format smoke tests — canvas, config, bases.

use nexus_storage::{FileFilter, StorageEngine};

fn engine() -> (tempfile::TempDir, StorageEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(dir.path()).unwrap();
    (dir, engine)
}

// ── Canvas ───────────────────────────────────────────────────────────────────

#[test]
fn canvas_write_and_read_round_trip() {
    let (_dir, engine) = engine();
    let json = r#"{
        "nodes": [
            {"id": "n1", "type": "text", "text": "Hello", "x": 0, "y": 0, "width": 300, "height": 200},
            {"id": "n2", "type": "file", "file": "notes/design.md", "x": 400, "y": 0, "width": 250, "height": 300}
        ],
        "edges": [
            {"id": "e1", "from": "n1", "to": "n2", "type": "dashed", "label": "references"}
        ]
    }"#;
    engine.write_file("project.canvas", json.as_bytes()).unwrap();

    let canvas = engine.read_canvas("project.canvas").unwrap();
    assert_eq!(canvas.nodes.len(), 2);
    assert_eq!(canvas.edges.len(), 1);
    assert_eq!(canvas.nodes[0].id, "n1");
    assert_eq!(canvas.edges[0].from_node, "n1");
    assert_eq!(canvas.edges[0].to_node, "n2");
}

#[test]
fn canvas_indexed_as_canvas_type() {
    let (_dir, engine) = engine();
    let json = r#"{"nodes":[],"edges":[]}"#;
    engine.write_file("empty.canvas", json.as_bytes()).unwrap();

    let files = engine
        .query_files(&FileFilter::default())
        .unwrap();
    let canvas_file = files.iter().find(|f| f.path == "empty.canvas");
    assert!(canvas_file.is_some(), "canvas should be indexed");
    assert_eq!(canvas_file.unwrap().file_type, "canvas");
}

#[test]
fn canvas_rewrite_replaces_data() {
    let (_dir, engine) = engine();

    let json1 = r#"{"nodes":[{"id":"n1","type":"text","text":"A","x":0,"y":0,"width":100,"height":100}],"edges":[]}"#;
    engine.write_file("rewrite.canvas", json1.as_bytes()).unwrap();
    assert_eq!(engine.read_canvas("rewrite.canvas").unwrap().nodes.len(), 1);

    let json2 = r#"{"nodes":[
        {"id":"n1","type":"text","text":"A","x":0,"y":0,"width":100,"height":100},
        {"id":"n2","type":"text","text":"B","x":200,"y":0,"width":100,"height":100}
    ],"edges":[]}"#;
    engine.write_file("rewrite.canvas", json2.as_bytes()).unwrap();
    assert_eq!(engine.read_canvas("rewrite.canvas").unwrap().nodes.len(), 2);
}

#[test]
fn canvas_delete_removes_from_index() {
    let (_dir, engine) = engine();
    let json = r#"{"nodes":[],"edges":[]}"#;
    engine.write_file("del.canvas", json.as_bytes()).unwrap();
    assert!(engine.file_exists("del.canvas").unwrap());

    engine.delete_file("del.canvas").unwrap();
    assert!(!engine.file_exists("del.canvas").unwrap());
}

#[test]
fn canvas_file_node_creates_graph_link() {
    let (_dir, engine) = engine();
    // Create a target note first.
    engine
        .write_file("notes/target.md", b"# Target\n\nContent.\n")
        .unwrap();

    // Create canvas with a file node pointing to it.
    let json = r#"{
        "nodes": [{"id":"n1","type":"file","file":"notes/target.md","x":0,"y":0,"width":200,"height":200}],
        "edges": []
    }"#;
    engine.write_file("overview.canvas", json.as_bytes()).unwrap();

    // The canvas should have an outgoing link in the graph.
    let outgoing = engine.outgoing_links("overview.canvas").unwrap();
    assert!(
        outgoing.iter().any(|l| l.target_path == "notes/target.md"),
        "canvas file node should create a graph edge to the target note, got: {outgoing:?}"
    );
}

#[test]
fn canvas_obsidian_compatible_json() {
    // Verify we can parse a realistic Obsidian .canvas file.
    let json = r##"{
        "nodes": [
            {"id":"abc123","type":"file","file":"README.md","x":-200,"y":-100,"width":400,"height":300},
            {"id":"def456","type":"text","text":"Architecture overview","x":300,"y":-100,"width":400,"height":300,"color":"#FF6B6B"},
            {"id":"ghi789","type":"link","url":"https://docs.rust-lang.org","x":-200,"y":300,"width":400,"height":200},
            {"id":"jkl012","type":"group","label":"Core Components","x":-250,"y":-150,"width":1000,"height":700}
        ],
        "edges": [
            {"id":"edge-1","from":"abc123","to":"def456","label":"documents"},
            {"id":"edge-2","from":"def456","to":"ghi789","type":"dotted","color":"#4A90E2"}
        ]
    }"##;
    let canvas = nexus_storage::parse_canvas(json).unwrap();
    assert_eq!(canvas.nodes.len(), 4);
    assert_eq!(canvas.edges.len(), 2);

    // Verify round-trip.
    let serialized = nexus_storage::serialize_canvas(&canvas).unwrap();
    let reparsed = nexus_storage::parse_canvas(&serialized).unwrap();
    assert_eq!(reparsed.nodes.len(), 4);
    assert_eq!(reparsed.edges.len(), 2);
}

// ── Config ───────────────────────────────────────────────────────────────────

#[test]
fn config_defaults_on_fresh_forge() {
    let (dir, _engine) = engine();
    let cfg = nexus_storage::config::load_app_config(dir.path()).unwrap();
    assert_eq!(cfg.core.name, "MyForge");
    assert_eq!(cfg.editor.font_size, 14);
    assert!(cfg.editor.auto_save);

    let ws = nexus_storage::config::load_workspace_state(dir.path()).unwrap();
    assert!(ws.active_file.is_none());
    assert_eq!(ws.theme, "dark");
}

#[test]
fn config_persist_and_reload() {
    let (dir, _engine) = engine();

    // Save custom app config.
    let mut cfg = nexus_storage::config::AppConfig::default();
    cfg.core.name = "TestForge".to_string();
    cfg.editor.font_size = 20;
    nexus_storage::config::save_app_config(dir.path(), &cfg).unwrap();

    // Save custom workspace state.
    let mut ws = nexus_storage::config::WorkspaceState::default();
    ws.active_file = Some("notes/index.md".to_string());
    ws.theme = "light".to_string();
    nexus_storage::config::save_workspace_state(dir.path(), &ws).unwrap();

    // Reload and verify.
    let cfg2 = nexus_storage::config::load_app_config(dir.path()).unwrap();
    assert_eq!(cfg2.core.name, "TestForge");
    assert_eq!(cfg2.editor.font_size, 20);

    let ws2 = nexus_storage::config::load_workspace_state(dir.path()).unwrap();
    assert_eq!(ws2.active_file, Some("notes/index.md".to_string()));
    assert_eq!(ws2.theme, "light");
}

#[test]
fn config_ai_and_mcp_round_trip() {
    let (dir, _engine) = engine();

    let mut ai = nexus_storage::config::AiConfig::default();
    ai.model = "claude-opus-4-6".to_string();
    ai.temperature = 0.3;
    nexus_storage::config::save_ai_config(dir.path(), &ai).unwrap();

    let mut mcp = nexus_storage::config::McpConfig::default();
    mcp.allowed_tools = vec!["search".to_string(), "read_file".to_string()];
    nexus_storage::config::save_mcp_config(dir.path(), &mcp).unwrap();

    let ai2 = nexus_storage::config::load_ai_config(dir.path()).unwrap();
    assert_eq!(ai2.model, "claude-opus-4-6");
    assert!((ai2.temperature - 0.3).abs() < f64::EPSILON);

    let mcp2 = nexus_storage::config::load_mcp_config(dir.path()).unwrap();
    assert_eq!(mcp2.allowed_tools.len(), 2);
}

// ── Bases ────────────────────────────────────────────────────────────────────

#[test]
fn bases_create_and_query() {
    let (dir, engine) = engine();

    let schema_json = r#"{"version":"1.0","fields":{
        "id":{"type":"uuid","primary":true},
        "title":{"type":"text","required":true},
        "status":{"type":"select","options":["todo","done"]}
    }}"#;
    let schema: nexus_storage::bases::BaseSchema = serde_json::from_str(schema_json).unwrap();
    let base_dir = dir.path().join("Tasks.bases");
    let mut base = nexus_storage::bases::init_base(&base_dir, "Tasks", &schema).unwrap();

    // Add records.
    base.records.push(nexus_storage::bases::BaseRecord {
        id: "r1".to_string(),
        deleted_at: None,
        fields: serde_json::json!({"title": "Buy milk", "status": "todo"})
            .as_object()
            .unwrap()
            .clone(),
    });
    base.records.push(nexus_storage::bases::BaseRecord {
        id: "r2".to_string(),
        deleted_at: None,
        fields: serde_json::json!({"title": "Write tests", "status": "done"})
            .as_object()
            .unwrap()
            .clone(),
    });
    nexus_storage::bases::save_base(&base_dir, &base).unwrap();

    // Index and query.
    engine.index_base("Tasks.bases", &base).unwrap();
    let summaries = engine.list_bases().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].name, "Tasks");
    assert_eq!(summaries[0].record_count, 2);
}

#[test]
fn bases_validation_rejects_bad_records() {
    let schema_json = r#"{"version":"1.0","fields":{
        "id":{"type":"uuid","primary":true},
        "title":{"type":"text","required":true}
    }}"#;
    let schema: nexus_storage::bases::BaseSchema = serde_json::from_str(schema_json).unwrap();

    // Valid record.
    let good = nexus_storage::bases::BaseRecord {
        id: "r1".to_string(),
        deleted_at: None,
        fields: serde_json::json!({"title": "Valid"}).as_object().unwrap().clone(),
    };
    assert!(nexus_storage::bases::validate_record(&schema, &good).is_ok());

    // Missing required field.
    let bad = nexus_storage::bases::BaseRecord {
        id: "r2".to_string(),
        deleted_at: None,
        fields: serde_json::Map::new(),
    };
    assert!(nexus_storage::bases::validate_record(&schema, &bad).is_err());
}

#[test]
fn bases_views_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let base_dir = dir.path().join("ViewTest.bases");

    let schema: nexus_storage::bases::BaseSchema = serde_json::from_str(
        r#"{"version":"1.0","fields":{"id":{"type":"uuid"},"title":{"type":"text"},"status":{"type":"select","options":["todo","done"]}}}"#,
    ).unwrap();

    let mut base = nexus_storage::bases::init_base(&base_dir, "ViewTest", &schema).unwrap();
    base.views.push(nexus_storage::bases::BaseView {
        name: "All Tasks".to_string(),
        view_type: nexus_storage::bases::ViewType::Table,
        fields: vec!["title".to_string(), "status".to_string()],
        sort: vec![nexus_storage::bases::SortRule {
            field: "title".to_string(),
            direction: "asc".to_string(),
        }],
        filter: vec![],
        group_field: None,
        date_field: None,
        end_field: None,
    });
    base.views.push(nexus_storage::bases::BaseView {
        name: "By Status".to_string(),
        view_type: nexus_storage::bases::ViewType::Kanban,
        fields: vec!["title".to_string()],
        sort: vec![],
        filter: vec![],
        group_field: Some("status".to_string()),
        date_field: None,
        end_field: None,
    });
    nexus_storage::bases::save_base(&base_dir, &base).unwrap();

    let loaded = nexus_storage::bases::load_base(&base_dir).unwrap();
    assert_eq!(loaded.views.len(), 2);
    let table_view = loaded.views.iter().find(|v| v.name == "All Tasks").unwrap();
    assert_eq!(table_view.view_type, nexus_storage::bases::ViewType::Table);
    assert_eq!(table_view.fields.len(), 2);
    let kanban_view = loaded.views.iter().find(|v| v.name == "By Status").unwrap();
    assert_eq!(kanban_view.view_type, nexus_storage::bases::ViewType::Kanban);
    assert_eq!(kanban_view.group_field.as_deref(), Some("status"));
}

#[test]
fn bases_full_lifecycle() {
    let (dir, engine) = engine();
    let base_dir = dir.path().join("Projects.bases");
    let schema: nexus_storage::bases::BaseSchema = serde_json::from_str(
        r#"{"version":"1.0","fields":{"id":{"type":"uuid"},"name":{"type":"text","required":true},"status":{"type":"select","options":["active","done"]}}}"#,
    ).unwrap();

    // Create.
    let base = nexus_storage::bases::init_base(&base_dir, "Projects", &schema).unwrap();
    engine.index_base("Projects.bases", &base).unwrap();

    // Add records by loading, mutating, saving.
    let mut base = nexus_storage::bases::load_base(&base_dir).unwrap();
    base.records.push(nexus_storage::bases::BaseRecord {
        id: "p1".to_string(),
        deleted_at: None,
        fields: serde_json::json!({"name": "Nexus", "status": "active"}).as_object().unwrap().clone(),
    });
    nexus_storage::bases::save_base(&base_dir, &base).unwrap();
    engine.index_base("Projects.bases", &base).unwrap();

    // Verify.
    let summaries = engine.list_bases().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].record_count, 1);

    // Reload from disk.
    let reloaded = nexus_storage::bases::load_base(&base_dir).unwrap();
    assert_eq!(reloaded.records.len(), 1);
    assert_eq!(reloaded.records[0].id, "p1");
}
