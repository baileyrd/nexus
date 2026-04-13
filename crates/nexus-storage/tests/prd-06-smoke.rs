//! PRD 06 smoke tests — block refs, callouts, tasks, alias resolution.

use nexus_storage::{parse_markdown, FileFilter, StorageEngine, TaskFilter};

fn engine() -> (tempfile::TempDir, StorageEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = StorageEngine::init(dir.path()).unwrap();
    (dir, engine)
}

#[test]
fn block_ref_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/refs.md", b"# Intro ^intro\n\nBody text ^body\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let blocks = engine.query_blocks(files[0].id).unwrap();

    let intro = blocks.iter().find(|b| b.block_ref_id == Some("intro".to_string()));
    assert!(intro.is_some(), "heading should have block_ref_id 'intro'");

    let body = blocks.iter().find(|b| b.block_ref_id == Some("body".to_string()));
    assert!(body.is_some(), "paragraph should have block_ref_id 'body'");
}

#[test]
fn callout_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/callouts.md", b"> [!warning] Watch out\n> Be careful here\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let blocks = engine.query_blocks(files[0].id).unwrap();

    let callout = blocks.iter().find(|b| b.callout_type == Some("warning".to_string()));
    assert!(callout.is_some(), "should have a warning callout block");
    assert_eq!(callout.unwrap().block_type, "callout");
}

#[test]
fn link_fragment_round_trip() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/links.md", b"See [[other#^ref1]] and [[other#Heading]]\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let links = engine.query_links(files[0].id).unwrap();

    assert_eq!(links.len(), 2);
    let ref_link = links.iter().find(|l| l.fragment == Some("^ref1".to_string()));
    assert!(ref_link.is_some(), "should have fragment ^ref1");

    let heading_link = links.iter().find(|l| l.fragment == Some("Heading".to_string()));
    assert!(heading_link.is_some(), "should have fragment Heading");
}

#[test]
fn task_extraction_and_query() {
    let (_dir, engine) = engine();
    engine
        .write_file(
            "notes/tasks.md",
            b"# Tasks\n\n- [ ] Buy milk\n- [x] Write tests\n- [ ] Deploy\n",
        )
        .unwrap();

    let all = engine.query_tasks(&TaskFilter::default()).unwrap();
    assert_eq!(all.len(), 3);

    let pending = engine
        .query_tasks(&TaskFilter {
            completed: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pending.len(), 2);

    let done = engine
        .query_tasks(&TaskFilter {
            completed: Some(true),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].content, "Write tests");
}

#[test]
fn task_toggle_writes_back_to_file() {
    let (_dir, engine) = engine();
    engine
        .write_file("notes/toggle.md", b"- [ ] Pending task\n")
        .unwrap();

    let tasks = engine.query_tasks(&TaskFilter::default()).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(!tasks[0].completed);

    let toggled = engine.toggle_task(tasks[0].id).unwrap();
    assert!(toggled.completed);

    // Verify file on disk was updated
    let content = engine.read_file("notes/toggle.md").unwrap();
    let text = String::from_utf8_lossy(&content);
    assert!(
        text.contains("- [x]"),
        "file should have checked checkbox, got: {text}"
    );
}

#[test]
fn alias_link_resolution() {
    let (_dir, engine) = engine();

    // File with aliases
    engine
        .write_file(
            "notes/real-name.md",
            b"---\naliases:\n  - Alt Name\n  - Another Alias\n---\n# Real Name\n",
        )
        .unwrap();

    // File linking via alias
    engine
        .write_file("notes/linker.md", b"See [[Alt Name]]\n")
        .unwrap();

    let files = engine.query_files(&FileFilter::default()).unwrap();
    let linker = files.iter().find(|f| f.path == "notes/linker.md").unwrap();
    let links = engine.query_links(linker.id).unwrap();

    assert_eq!(links.len(), 1);
    assert!(links[0].is_resolved, "link via alias should be resolved");
    assert!(links[0].target_file_id.is_some(), "target_file_id should be set");
}

#[test]
fn combined_markdown_features() {
    let md = concat!(
        "---\ntags:\n  - test\n---\n",
        "# Title ^title\n\n",
        "> [!note] Important\n> Remember this\n\n",
        "- [ ] First task\n- [x] Done task\n\n",
        "See [[other#^ref1]]\n\n",
        "Some text #inline-tag\n",
    );
    let pf = parse_markdown(md).unwrap();

    // Tags: 1 frontmatter + 1 inline
    assert!(pf.tags.iter().any(|t| t.name == "test" && t.source == "frontmatter"));
    assert!(pf.tags.iter().any(|t| t.name == "inline-tag" && t.source == "inline"));

    // Block ref on heading
    let title = pf.blocks.iter().find(|b| b.block_type == "heading").unwrap();
    assert_eq!(title.block_ref_id, Some("title".to_string()));

    // Callout
    let callout = pf.blocks.iter().find(|b| b.block_type == "callout").unwrap();
    assert_eq!(callout.callout_type, Some("note".to_string()));

    // Tasks
    assert_eq!(pf.tasks.len(), 2);

    // Link with fragment
    let link = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
    assert_eq!(link.fragment, Some("^ref1".to_string()));
}

#[test]
fn search_with_tag_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/rust.md", b"---\ntags:\n  - rust\n---\n# Rust Guide\n\nAsync programming in Rust.\n")
        .unwrap();
    engine
        .write_file("notes/python.md", b"---\ntags:\n  - python\n---\n# Python Guide\n\nAsync programming in Python.\n")
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let all = engine.search("programming", 10).unwrap();
    assert!(all.len() >= 2, "expected at least 2 results, got {}", all.len());

    let scoped = engine.search("tag:rust programming", 10).unwrap();
    assert_eq!(scoped.len(), 1, "expected 1 result for tag:rust, got {}", scoped.len());
    assert_eq!(scoped[0].file_path, "notes/rust.md");
}

#[test]
fn search_with_path_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file("notes/a.md", b"# Notes\n\nImportant content here.\n")
        .unwrap();
    engine
        .write_file("docs/b.md", b"# Docs\n\nImportant content here.\n")
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("path:notes/ important", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/a.md");
}

#[test]
fn search_with_prop_scope() {
    let (_dir, engine) = engine();

    engine
        .write_file(
            "notes/done.md",
            b"---\nstatus: done\n---\n# Done Task\n\nCompleted work here.\n",
        )
        .unwrap();
    engine
        .write_file(
            "notes/wip.md",
            b"---\nstatus: wip\n---\n# WIP Task\n\nCompleted work here.\n",
        )
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("prop:status:done work", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/done.md");
}

#[test]
fn search_with_combined_scopes() {
    let (_dir, engine) = engine();

    engine
        .write_file(
            "notes/match.md",
            b"---\ntags:\n  - rust\n---\n# Match\n\nAsync programming patterns.\n",
        )
        .unwrap();
    engine
        .write_file(
            "docs/nomatch.md",
            b"---\ntags:\n  - rust\n---\n# No Match\n\nAsync programming patterns.\n",
        )
        .unwrap();

    engine.rebuild_search_index().unwrap();

    let scoped = engine.search("tag:rust path:notes/ programming", 10).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].file_path, "notes/match.md");
}

#[test]
fn daily_note_template_is_indexed() {
    let (_dir, engine) = engine();

    let content =
        b"---\ndate: 2026-04-13\ntags: [daily]\n---\n# April 13, 2026\n\n## Tasks\n\n## Notes\n";
    engine
        .write_file("notes/daily/2026-04-13.md", content)
        .unwrap();

    // Verify the daily tag is indexed
    let tags = engine.query_tags("daily").unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].file_path, "notes/daily/2026-04-13.md");

    // Verify file exists
    assert!(engine.file_exists("notes/daily/2026-04-13.md").unwrap());
}
