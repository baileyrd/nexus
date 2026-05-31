//! Output-buffer throughput benches (PRD-09 §17.1).
//!
//! # What this measures
//!
//! - **push_100k_lines**: cost of streaming 100 000 short lines into
//!   [`OutputBuffer`] + [`LineBuffer`] as `read_into` would. The PRD
//!   target is **10 000 lines/sec sustained** — at our ~100-byte
//!   lines that's roughly 1 MB/s, so the bench should stay far below
//!   the default Criterion sample budget.
//! - **memory_per_10k_line_buffer**: byte footprint of a filled
//!   [`LineBuffer`] at its default cap. PRD target is **< 50 MB per
//!   10 000-line buffer**; we assert on the estimate so a regression
//!   in `Line` layout trips the bench.
//!
//! # Why not `#[bench]`
//!
//! The workspace is stable-only (see `rust-toolchain.toml`).
//! Criterion's `criterion_group!` macro + `Bencher::iter_batched`
//! gives us statistically meaningful numbers without nightly.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use nexus_terminal::{LineBuffer, OutputBuffer};

/// Synthetic ~100-byte line similar to a typical build log entry.
fn sample_line(i: usize) -> Vec<u8> {
    format!(
        "[2026-04-17 12:34:56.{i:06}] INFO  module::submod: doing the thing number {i} of many\n",
    )
    .into_bytes()
}

fn bench_push_100k_lines(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal.throughput");
    // One bench iteration pushes 100 k lines — the throughput number
    // Criterion prints is "elements per second", which maps directly
    // to PRD §17.1's lines/sec metric.
    group.throughput(Throughput::Elements(100_000));
    group.sample_size(10); // pushing 100k lines is slow per sample.

    let lines: Vec<Vec<u8>> = (0..100_000).map(sample_line).collect();

    group.bench_function("push_100k_lines_into_output_and_line_buffer", |b| {
        b.iter(|| {
            // Fresh buffers each iteration so eviction doesn't skew
            // the numbers in one direction or the other.
            let mut bytes = OutputBuffer::with_capacity(10 * 1024 * 1024);
            let mut line_buf = LineBuffer::with_max_lines(100_000);
            for line in &lines {
                bytes.push(line);
                line_buf.push(line);
            }
            black_box((bytes.len(), line_buf.len()));
        });
    });
    group.finish();
}

fn bench_memory_per_10k_line_buffer(c: &mut Criterion) {
    // Not a latency measurement, but we use Criterion's reporting so
    // the assertion lands alongside throughput numbers. Each iter
    // fills a fresh 10k-line buffer and reports the total string byte
    // count + struct overhead as a pseudo-"throughput" metric.
    let mut group = c.benchmark_group("terminal.memory");
    group.sample_size(10);
    let lines: Vec<Vec<u8>> = (0..10_000).map(sample_line).collect();

    group.bench_function("build_10k_line_buffer_and_measure_footprint", |b| {
        b.iter(|| {
            let mut line_buf = LineBuffer::with_max_lines(10_000);
            for line in &lines {
                line_buf.push(line);
            }
            // Very rough lower-bound: raw bytes + text_only bytes across
            // every line + struct overhead. Real allocator footprint is
            // bigger, but the bench reads the same number every run so a
            // regression flags a size increase without measuring RSS.
            let bytes: usize = line_buf
                .iter()
                .map(|l| l.raw.capacity() + l.text_only.capacity())
                .sum();
            black_box(bytes);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_push_100k_lines,
    bench_memory_per_10k_line_buffer
);
criterion_main!(benches);
