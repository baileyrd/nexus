//! BL-122: typing-latency perf scaffold for `apply_transaction`.
//!
//! This integration test exercises `EditorCorePlugin::dispatch` with
//! a text-only `InsertText` op across three document sizes and prints
//! a stable JSON line per scenario, prefixed `PERF_RESULT::`, that
//! `experiments/perf/run.ts` parses into the report it commits to
//! `experiments/perf/baselines/`.
//!
//! Gated behind `NEXUS_PERF=1` so default `cargo test` runs are
//! unaffected (a normal `cargo test -p nexus-editor` returns
//! immediately). The microbench drives the kernel directly — no
//! Tauri, no IPC dispatcher — so the numbers isolate the editor crate
//! and let BL-123 land a "flat curve" win that's visible against the
//! current N-linear baseline.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nexus_editor::core_plugin::{
    EditorCorePlugin, EditorSnapshot, HANDLER_APPLY_TRANSACTION, HANDLER_OPEN,
};
use nexus_editor::{Operation, Transaction, TransactionMetadata};
use nexus_plugins::CorePlugin;
use serde_json::json;
use tempfile::TempDir;

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    /// Number of paragraph blocks in the source doc.
    block_count: usize,
    /// Iterations to time (after warmup).
    iterations: usize,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "editor.apply_transaction.small",
        block_count: 10,
        iterations: 500,
    },
    Scenario {
        name: "editor.apply_transaction.medium",
        block_count: 100,
        iterations: 300,
    },
    Scenario {
        name: "editor.apply_transaction.large",
        block_count: 5000,
        iterations: 50,
    },
];

#[test]
fn perf_apply_transaction() {
    if env::var("NEXUS_PERF").ok().as_deref() != Some("1") {
        // Default cargo test runs return immediately. Set NEXUS_PERF=1
        // to opt in.
        return;
    }

    for sc in SCENARIOS {
        let line = run_scenario(*sc);
        println!("PERF_RESULT::{line}");
    }
}

fn run_scenario(sc: Scenario) -> String {
    let (_tmp, root) = setup_forge();
    let relpath = "notes/bench.md";
    write_note(&root, relpath, &build_doc(sc.block_count));

    let mut plugin = EditorCorePlugin::new(root.clone());
    plugin.on_init().unwrap();
    let open_snap: EditorSnapshot = serde_json::from_value(
        plugin
            .dispatch(HANDLER_OPEN, &json!({ "relpath": relpath }))
            .unwrap(),
    )
    .unwrap();

    // Target the last paragraph so the insert lands deep in the block
    // list — keeps the post-mutation snapshot serialize honest.
    let target = *open_snap
        .tree
        .root_blocks
        .last()
        .expect("benchmark doc must have at least one block");
    let content_len = open_snap.tree.blocks[&target].content.len();

    // Warmup: a handful of inserts to let the allocator settle and the
    // undo tree grow past its initial Vec reallocations. The iteration
    // count is independent of `iterations` so a tiny scenario still
    // gets a couple warmups.
    let warmup = sc.iterations.clamp(3, 20);
    for i in 0..warmup {
        apply_one(&mut plugin, relpath, target, content_len + i, "x");
    }

    let mut samples_us: Vec<f64> = Vec::with_capacity(sc.iterations);
    let start = Instant::now();
    for i in 0..sc.iterations {
        let t0 = Instant::now();
        apply_one(&mut plugin, relpath, target, content_len + warmup + i, "y");
        samples_us.push(t0.elapsed().as_secs_f64() * 1_000_000.0);
    }
    let total_ms = start.elapsed().as_secs_f64() * 1_000.0;

    samples_us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = percentile(&samples_us, 0.5);
    let p95 = percentile(&samples_us, 0.95);
    let p99 = percentile(&samples_us, 0.99);
    let mean = samples_us.iter().copied().sum::<f64>() / samples_us.len() as f64;

    json!({
        "name": sc.name,
        "iterations": sc.iterations,
        "block_count": sc.block_count,
        "p50us": round2(p50),
        "p95us": round2(p95),
        "p99us": round2(p99),
        "meanus": round2(mean),
        "totalMs": round2(total_ms),
    })
    .to_string()
}

fn apply_one(
    plugin: &mut EditorCorePlugin,
    relpath: &str,
    block: uuid::Uuid,
    pos: usize,
    ch: &str,
) {
    let tx = Transaction::new(
        vec![Operation::InsertText {
            block_id: block,
            pos,
            text: ch.into(),
            pre_annotations: Vec::new(),
        }],
        TransactionMetadata::default(),
    );
    let resp = plugin
        .dispatch(
            HANDLER_APPLY_TRANSACTION,
            &json!({
                "relpath": relpath,
                "transaction": serde_json::to_value(&tx).unwrap(),
            }),
        )
        .expect("apply_transaction must succeed");
    // Force the JSON value to be touched so the optimizer can't elide
    // any of the work — mirrors what an IPC consumer would do.
    let _ = resp.to_string().len();
}

fn percentile(sorted_asc: &[f64], p: f64) -> f64 {
    if sorted_asc.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_asc.len() as f64) * p) as usize;
    let idx = idx.min(sorted_asc.len() - 1);
    sorted_asc[idx]
}

fn round2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

fn setup_forge() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    fs::create_dir_all(root.join(".forge")).unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    (tmp, root)
}

fn write_note(root: &Path, relpath: &str, body: &str) {
    let abs = root.join(relpath);
    if let Some(p) = abs.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(abs, body).unwrap();
}

/// Build a synthetic markdown document with `n` paragraph blocks. We
/// keep each block uniform and short — the cost we want to expose is
/// per-block, not per-byte; BL-123 shrinks the response from O(N
/// blocks) to O(1) for text-only ops, so the curve flattens once that
/// lands.
fn build_doc(n: usize) -> String {
    let mut s = String::with_capacity(n * 24);
    for i in 0..n {
        s.push_str(&format!("Paragraph number {i}.\n\n"));
    }
    s
}
