//! Line-buffer search benches (PRD-09 §17.1).
//!
//! # What this measures
//!
//! - **search_literal_100k**: substring scan across a 100 000-line
//!   buffer. PRD target is **< 500 ms**.
//! - **search_regex_100k**: regex-lite scan across the same buffer.
//!   Same target — regex is expected to be slower than substring but
//!   still well under the threshold for common patterns.
//! - **build_100k_line_buffer**: one-shot cost of ingesting 100 k
//!   lines; complements `buffers::push_100k_lines` by exercising the
//!   structured-line path in isolation (no byte ring).

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use nexus_terminal::LineBuffer;

fn sample_line(i: usize) -> Vec<u8> {
    format!("log entry {i}: step {step} of {total} — status ok\n", step = i % 100, total = 100)
        .into_bytes()
}

fn build_buffer(n: usize) -> LineBuffer {
    let mut b = LineBuffer::with_max_lines(n);
    for i in 0..n {
        b.push(&sample_line(i));
    }
    b
}

fn bench_build_100k_line_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal.lines");
    group.throughput(Throughput::Elements(100_000));
    group.sample_size(10);
    group.bench_function("build_100k_line_buffer", |b| {
        b.iter(|| {
            let buf = build_buffer(100_000);
            black_box(buf.len());
        });
    });
    group.finish();
}

fn bench_search_literal_100k(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal.search");
    group.sample_size(20);
    // Pre-build once; the search is the hot loop.
    let buf = build_buffer(100_000);
    group.bench_function("literal_substring_100k", |b| {
        b.iter(|| {
            // Substring that matches every 100th line.
            let hits = buf.find("step 50");
            black_box(hits.len());
        });
    });
    group.finish();
}

fn bench_search_regex_100k(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal.search");
    group.sample_size(20);
    let buf = build_buffer(100_000);
    group.bench_function("regex_100k", |b| {
        b.iter(|| {
            // Anchored regex — a realistic "lines starting with 'log entry'
            // whose id is three digits" shape.
            let hits = buf.find_regex(r"^log entry \d{3}:").expect("valid regex");
            black_box(hits.len());
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_build_100k_line_buffer,
    bench_search_literal_100k,
    bench_search_regex_100k,
);
criterion_main!(benches);
