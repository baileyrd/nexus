//! BL-092 — kernel event bus criterion benchmarks.
//!
//! These establish performance baselines for the event bus paths
//! that every plugin lifecycle and IPC path eventually crosses.
//! Benchmarks print throughput; SLO assertions are intentionally
//! omitted at this stage (see BACKLOG_COMPLETED notes for BL-092).

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use nexus_kernel::{EventBus, EventFilter};
use serde_json::json;

const BUS_CAPACITY: usize = 4096;

fn bench_publish_no_subscribers(c: &mut Criterion) {
    let bus = EventBus::new(BUS_CAPACITY);
    c.bench_function("event_bus/publish_no_subscribers", |b| {
        b.iter(|| {
            let _ = bus.publish_plugin(
                "com.bench.publisher",
                "com.bench.publisher.event",
                json!({"k": 1}),
            );
        });
    });
}

fn bench_publish_one_subscriber(c: &mut Criterion) {
    let bus = EventBus::new(BUS_CAPACITY);
    let _sub = bus.subscribe(EventFilter::All);
    c.bench_function("event_bus/publish_one_subscriber", |b| {
        b.iter(|| {
            let _ = bus.publish_plugin(
                "com.bench.publisher",
                "com.bench.publisher.event",
                json!({"k": 1}),
            );
        });
    });
}

fn bench_publish_ten_subscribers(c: &mut Criterion) {
    let bus = EventBus::new(BUS_CAPACITY);
    let mut subs = Vec::with_capacity(10);
    for _ in 0..10 {
        subs.push(bus.subscribe(EventFilter::All));
    }
    c.bench_function("event_bus/publish_ten_subscribers", |b| {
        b.iter(|| {
            let _ = bus.publish_plugin(
                "com.bench.publisher",
                "com.bench.publisher.event",
                json!({"k": 1}),
            );
        });
    });
    drop(subs);
}

fn bench_subscribe_filter_match(c: &mut Criterion) {
    let bus = EventBus::new(BUS_CAPACITY);
    let _sub = bus.subscribe(EventFilter::CustomPrefix("com.bench.".to_string()));
    c.bench_function("event_bus/subscribe_filter_match", |b| {
        b.iter(|| {
            let _ = bus.publish_plugin(
                "com.bench.publisher",
                "com.bench.publisher.event",
                black_box(json!({})),
            );
        });
    });
}

fn bench_subscribe_filter_no_match(c: &mut Criterion) {
    let bus = EventBus::new(BUS_CAPACITY);
    let _sub = bus.subscribe(EventFilter::CustomPrefix("com.never.".to_string()));
    c.bench_function("event_bus/subscribe_filter_no_match", |b| {
        b.iter(|| {
            let _ = bus.publish_plugin(
                "com.bench.publisher",
                "com.bench.publisher.event",
                black_box(json!({})),
            );
        });
    });
}

criterion_group!(
    benches,
    bench_publish_no_subscribers,
    bench_publish_one_subscriber,
    bench_publish_ten_subscribers,
    bench_subscribe_filter_match,
    bench_subscribe_filter_no_match,
);
criterion_main!(benches);
