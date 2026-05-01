//! Regression tests for issue #78 — unbounded parsing in `nexus-formats`.
//!
//! 1. YAML frontmatter parser was unbounded; a billion-laughs-shaped
//!    YAML input could exhaust memory before parsing terminated.
//! 2. Canvas JSON parser had no input size limit and no node/edge
//!    count limit; serde walked gigabytes of intermediate state on
//!    a single malicious file.

use nexus_formats::canvas::{
    parse as parse_canvas, parse_with_path as parse_canvas_with_path, MAX_CANVAS_BYTES,
    MAX_CANVAS_ELEMENTS,
};
use nexus_formats::markdown::frontmatter::{extract, MAX_FRONTMATTER_BYTES};
use nexus_formats::CanvasError;
use nexus_formats::MarkdownError;

#[test]
fn frontmatter_rejects_oversize_yaml() {
    // Frontmatter block one byte over the cap. Content is benign
    // (a single string field) — the cap fires on size alone, before
    // serde_yml ever sees it.
    let inner = "title: ".to_string() + &"a".repeat(MAX_FRONTMATTER_BYTES + 1);
    let doc = format!("---\n{inner}\n---\nbody");
    let err = extract(&doc).expect_err("must reject oversize frontmatter");
    match err {
        MarkdownError::FrontmatterParse { reason, .. } => {
            assert!(
                reason.contains("max is") && reason.contains("bytes"),
                "expected size-cap rejection; got: {reason}"
            );
        }
        other => panic!("expected FrontmatterParse, got: {other:?}"),
    }
}

#[test]
fn frontmatter_accepts_normal_size() {
    // Realistic frontmatter — well under the cap.
    let doc = "---\ntitle: Hello\ntags: [a, b]\n---\nbody\n";
    let (fm, body) = extract(doc).expect("normal frontmatter must parse");
    assert_eq!(fm.title.as_deref(), Some("Hello"));
    assert!(body.starts_with("body"));
}

#[test]
fn canvas_rejects_oversize_input() {
    // Build an oversize JSON payload — junk past the byte cap. The
    // size gate fires before serde sees it, so the contents needn't
    // be valid canvas JSON.
    let mut json = String::with_capacity(MAX_CANVAS_BYTES + 16);
    json.push('{');
    json.push_str(&"a".repeat(MAX_CANVAS_BYTES));
    json.push('}');
    let err = parse_canvas(&json).expect_err("must reject oversize input");
    match err {
        CanvasError::InvalidJson { reason, .. } => {
            assert!(
                reason.contains("max is") && reason.contains("bytes"),
                "expected size-cap rejection; got: {reason}"
            );
        }
        other => panic!("expected InvalidJson, got: {other:?}"),
    }
}

#[test]
fn canvas_rejects_excessive_element_count() {
    // Build a small-bytes-but-many-elements canvas: 100_001 nodes.
    // Each node is the minimum shape; total bytes stay well under
    // MAX_CANVAS_BYTES but the element count exceeds the limit.
    let mut nodes = String::from("[");
    let count = MAX_CANVAS_ELEMENTS + 1;
    for i in 0..count {
        if i > 0 {
            nodes.push(',');
        }
        // Smallest valid node shape per the canvas schema: id + type
        // + position + size. Type "text" is widely supported.
        nodes.push_str(&format!(
            r#"{{"id":"n{i}","type":"text","x":0,"y":0,"width":10,"height":10,"text":""}}"#
        ));
    }
    nodes.push(']');
    let json = format!(r#"{{"nodes":{nodes},"edges":[]}}"#);
    // Sanity-check that we're under the byte cap so we're really
    // exercising the element-count gate.
    assert!(
        json.len() <= MAX_CANVAS_BYTES,
        "test fixture itself exceeded byte cap; element-count gate would never fire"
    );
    let err = parse_canvas_with_path(&json, "test.canvas")
        .expect_err("must reject excessive element count");
    match err {
        CanvasError::InvalidJson { reason, .. } => {
            assert!(
                reason.contains("nodes+edges") && reason.contains("max is"),
                "expected element-count rejection; got: {reason}"
            );
        }
        other => panic!("expected InvalidJson, got: {other:?}"),
    }
}

#[test]
fn canvas_accepts_realistic_input() {
    // Smoke: a small valid canvas still parses fine.
    let json = r#"{
        "nodes": [
            {"id":"a","type":"text","x":0,"y":0,"width":100,"height":50,"text":"hello"}
        ],
        "edges": []
    }"#;
    parse_canvas(json).expect("realistic canvas must parse");
}
