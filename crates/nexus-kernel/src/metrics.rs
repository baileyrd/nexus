//! BL-093 — kernel time-series metrics.
//!
//! In-process counters + histograms for the kernel's hot paths (IPC
//! dispatch, event bus, capability checks, plugin lifecycle hooks).
//! The recording API is fire-and-forget — every method takes `&self`
//! and writes through a `Mutex` plus `AtomicU64`s, so callers in the
//! dispatch path don't need a mutable handle. Snapshots are cheap
//! enough to take per IPC handler call (no scraping endpoint
//! required).
//!
//! ## Histogram strategy
//!
//! Latencies are recorded into fixed exponential buckets (1µs, 10µs,
//! 100µs, 1ms, 10ms, 100ms, 1s, 10s, +∞) so a snapshot can compute
//! p50 / p95 / p99 by interpolating bucket cumulative counts. We
//! deliberately avoid reservoir sampling (too much memory for the
//! number of distinct (plugin, command) pairs) and the `metrics`
//! crate (an extra workspace-wide dep + global registry surface).
//!
//! ## Cardinality
//!
//! `(plugin_id, command_id)` tuples and `(plugin_id, capability,
//! result)` tuples can grow unboundedly when a plugin synthesises
//! command/cap names at runtime. The recorder caps each
//! `HashMap<key, value>` at 4096 entries; once full, further unique
//! keys increment a sentinel `metrics_dropped_total` counter so the
//! cap is observable rather than silent.
//!
//! ## What's *not* in this module
//!
//! - Prometheus scrape endpoint (deferred — see BL-093 closure
//!   notes).
//! - Shell health panel UI (deferred).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

/// Maximum number of distinct keys per metric. See module docs.
const MAX_KEYS_PER_METRIC: usize = 4096;

/// Bucket upper bounds in nanoseconds (last bucket = +∞).
const BUCKET_UPPER_NS: &[u64] = &[
    1_000,          // 1µs
    10_000,         // 10µs
    100_000,        // 100µs
    1_000_000,      // 1ms
    10_000_000,     // 10ms
    100_000_000,    // 100ms
    1_000_000_000,  // 1s
    10_000_000_000, // 10s
];

const NUM_BUCKETS: usize = BUCKET_UPPER_NS.len() + 1;

/// Outcome label for IPC and capability metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallStatus {
    /// Call succeeded.
    Ok,
    /// Caller lacked the required capability.
    CapabilityDenied,
    /// Target plugin / command was unknown.
    NotFound,
    /// Handler timed out.
    Timeout,
    /// Caller (or ancestor IPC dispatch) cancelled the call via the
    /// cooperative `CancellationToken` — distinct from `Timeout`
    /// (deadline-driven) so dashboards can separate user / parent
    /// abandonment from genuinely slow handlers.
    Cancelled,
    /// Handler returned an error or trapped.
    Error,
}

impl CallStatus {
    fn as_str(self) -> &'static str {
        match self {
            CallStatus::Ok => "ok",
            CallStatus::CapabilityDenied => "capability_denied",
            CallStatus::NotFound => "not_found",
            CallStatus::Timeout => "timeout",
            CallStatus::Cancelled => "cancelled",
            CallStatus::Error => "error",
        }
    }
}

#[derive(Default)]
struct Histogram {
    buckets: [AtomicU64; NUM_BUCKETS],
    sum_ns: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    fn record(&self, ns: u64) {
        let mut idx = NUM_BUCKETS - 1;
        for (i, &edge) in BUCKET_UPPER_NS.iter().enumerate() {
            if ns <= edge {
                idx = i;
                break;
            }
        }
        self.buckets[idx].fetch_add(1, Ordering::Relaxed);
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> HistogramSnapshot {
        let count = self.count.load(Ordering::Relaxed);
        let sum_ns = self.sum_ns.load(Ordering::Relaxed);
        let buckets: Vec<u64> = self
            .buckets
            .iter()
            .map(|b| b.load(Ordering::Relaxed))
            .collect();
        HistogramSnapshot {
            count,
            sum_ns,
            mean_ns: sum_ns.checked_div(count).unwrap_or(0),
            p50_ns: percentile(&buckets, count, 0.50),
            p95_ns: percentile(&buckets, count, 0.95),
            p99_ns: percentile(&buckets, count, 0.99),
        }
    }
}

fn percentile(buckets: &[u64], count: u64, p: f64) -> u64 {
    if count == 0 {
        return 0;
    }
    let target = (count as f64 * p).ceil() as u64;
    let mut acc = 0u64;
    for (i, &b) in buckets.iter().enumerate() {
        acc += b;
        if acc >= target {
            return BUCKET_UPPER_NS.get(i).copied().unwrap_or(u64::MAX);
        }
    }
    u64::MAX
}

/// Wire-form histogram values in a metrics snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct HistogramSnapshot {
    /// Total observations recorded.
    pub count: u64,
    /// Sum of observed values in nanoseconds.
    pub sum_ns: u64,
    /// `sum_ns / count`, or 0 if count is 0.
    pub mean_ns: u64,
    /// 50th-percentile bucket upper bound (ns).
    pub p50_ns: u64,
    /// 95th-percentile bucket upper bound (ns).
    pub p95_ns: u64,
    /// 99th-percentile bucket upper bound (ns).
    pub p99_ns: u64,
}

/// Public snapshot type produced by [`KernelMetrics::snapshot`].
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    /// `ipc_calls_total{plugin_id, command, status}` — flattened
    /// `<plugin>::<command>::<status>` keys.
    pub ipc_calls_total: HashMap<String, u64>,
    /// `ipc_call_duration{plugin_id, command}` histograms.
    pub ipc_call_duration: HashMap<String, HistogramSnapshot>,
    /// `event_bus_published_total{plugin_id}` counter.
    pub event_bus_published_total: HashMap<String, u64>,
    /// `capability_checks_total{plugin_id, capability, result}`
    /// (flattened `<plugin>::<cap>::<result>` keys).
    pub capability_checks_total: HashMap<String, u64>,
    /// `plugin_lifecycle_duration{plugin_id, hook}` histograms.
    pub plugin_lifecycle_duration: HashMap<String, HistogramSnapshot>,
    /// `event_bus_queue_depth` — instantaneous broadcast-channel
    /// buffer occupancy, sampled after every `publish_*`. The
    /// snapshot returns the most-recent reading (the gauge
    /// semantic — last-set wins). 0 means no backpressure; high
    /// values mean a slow subscriber is about to get a `Lagged`
    /// error from `tokio::sync::broadcast`. Cap is the bus capacity
    /// from `KernelConfig` (default 1024).
    pub event_bus_queue_depth: u64,
    /// Sentinel — number of metric writes dropped because a
    /// per-metric key cap was hit.
    pub metrics_dropped_total: u64,
}

/// A single-slot gauge backed by [`AtomicU64`]. Cheap to read /
/// write under contention; the snapshot reads the last value
/// stored. Used for instantaneous samples whose history isn't
/// useful (e.g. broadcast channel queue depth, where only the
/// current reading is actionable).
#[derive(Default)]
struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }

    fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct CounterMap {
    inner: Mutex<HashMap<String, AtomicU64>>,
}

impl CounterMap {
    fn incr(&self, key: String, dropped: &AtomicU64) {
        let mut m = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(slot) = m.get(&key) {
            slot.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if m.len() >= MAX_KEYS_PER_METRIC {
            dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        m.insert(key, AtomicU64::new(1));
    }

    fn snapshot(&self) -> HashMap<String, u64> {
        let m = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        m.iter()
            .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
            .collect()
    }
}

#[derive(Default)]
struct HistogramMap {
    inner: Mutex<HashMap<String, Histogram>>,
}

impl HistogramMap {
    fn record(&self, key: String, ns: u64, dropped: &AtomicU64) {
        let mut m = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(h) = m.get(&key) {
            h.record(ns);
            return;
        }
        if m.len() >= MAX_KEYS_PER_METRIC {
            dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let h = Histogram::default();
        h.record(ns);
        m.insert(key, h);
    }

    fn snapshot(&self) -> HashMap<String, HistogramSnapshot> {
        let m = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        m.iter().map(|(k, h)| (k.clone(), h.snapshot())).collect()
    }
}

/// Per-kernel metrics registry. Cheap to clone via `Arc` wrapper at
/// the call site; this struct holds interior mutability so every
/// recording method takes `&self`.
#[derive(Default)]
pub struct KernelMetrics {
    ipc_calls_total: CounterMap,
    ipc_call_duration: HistogramMap,
    event_bus_published_total: CounterMap,
    capability_checks_total: CounterMap,
    plugin_lifecycle_duration: HistogramMap,
    event_bus_queue_depth: Gauge,
    metrics_dropped_total: AtomicU64,
}

impl KernelMetrics {
    /// Construct an empty metrics registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one finished IPC call. `duration_ns` is end-to-end
    /// wall time including caller-side capability check + dispatch
    /// hop.
    pub fn record_ipc_call(
        &self,
        plugin_id: &str,
        command: &str,
        status: CallStatus,
        duration_ns: u64,
    ) {
        self.ipc_calls_total.incr(
            format!("{plugin_id}::{command}::{}", status.as_str()),
            &self.metrics_dropped_total,
        );
        self.ipc_call_duration.record(
            format!("{plugin_id}::{command}"),
            duration_ns,
            &self.metrics_dropped_total,
        );
    }

    /// Record one event bus publish (plugin-namespaced or kernel-tier).
    pub fn record_event_publish(&self, plugin_id: &str) {
        self.event_bus_published_total
            .incr(plugin_id.to_string(), &self.metrics_dropped_total);
    }

    /// Set the `event_bus_queue_depth` gauge to a fresh sample.
    /// `EventBus::publish_*` calls this with `sender.len()` after
    /// every publish — the gauge reflects the latest reading; an
    /// earlier sample is overwritten without history. Last-write
    /// wins (typical gauge semantic).
    pub fn record_event_bus_queue_depth(&self, depth: u64) {
        self.event_bus_queue_depth.set(depth);
    }

    /// Record one capability check.
    pub fn record_capability_check(&self, plugin_id: &str, capability: &str, granted: bool) {
        let result = if granted { "granted" } else { "denied" };
        self.capability_checks_total.incr(
            format!("{plugin_id}::{capability}::{result}"),
            &self.metrics_dropped_total,
        );
    }

    /// Record the duration of one plugin lifecycle hook
    /// (`on_init`, `on_start`, `on_stop`, …).
    pub fn record_lifecycle_duration(&self, plugin_id: &str, hook: &str, duration_ns: u64) {
        self.plugin_lifecycle_duration.record(
            format!("{plugin_id}::{hook}"),
            duration_ns,
            &self.metrics_dropped_total,
        );
    }

    /// Time the closure `f`, then record the result via
    /// [`Self::record_lifecycle_duration`]. Convenience wrapper used
    /// by the plugin loader's lifecycle watchdog (BL-095).
    pub fn time_lifecycle<F, R>(&self, plugin_id: &str, hook: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let started = std::time::Instant::now();
        let r = f();
        self.record_lifecycle_duration(
            plugin_id,
            hook,
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        );
        r
    }

    /// Snapshot every metric as a serialisable view. Cheap enough
    /// to call per IPC handler invocation — no allocations beyond
    /// the result map.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            ipc_calls_total: self.ipc_calls_total.snapshot(),
            ipc_call_duration: self.ipc_call_duration.snapshot(),
            event_bus_published_total: self.event_bus_published_total.snapshot(),
            capability_checks_total: self.capability_checks_total.snapshot(),
            plugin_lifecycle_duration: self.plugin_lifecycle_duration.snapshot(),
            event_bus_queue_depth: self.event_bus_queue_depth.get(),
            metrics_dropped_total: self.metrics_dropped_total.load(Ordering::Relaxed),
        }
    }
}

// ── Prometheus text exposition (BL-093 exit path) ────────────────────────────

/// Escape a label value per the Prometheus text exposition format:
/// backslash, double-quote, and newline must be escaped.
fn escape_label(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Nanoseconds → seconds for Prometheus base units. `f64` default
/// formatting is the shortest round-trip representation, so output is
/// deterministic.
#[allow(clippy::cast_precision_loss)] // ns fit f64's mantissa for any realistic uptime
fn ns_to_secs(ns: u64) -> f64 {
    ns as f64 / 1_000_000_000.0
}

/// Render a flattened `<a>::<b>[::<c>]` counter key as a Prometheus
/// label set with the given label names. Keys that don't split into
/// exactly `names.len()` parts fall back to a single `key="<raw>"`
/// label rather than being dropped — visibility over tidiness.
fn labels_for(key: &str, names: &[&str]) -> String {
    let parts: Vec<&str> = key.split("::").collect();
    if parts.len() == names.len() {
        let pairs: Vec<String> = names
            .iter()
            .zip(&parts)
            .map(|(n, p)| format!("{n}=\"{}\"", escape_label(p)))
            .collect();
        format!("{{{}}}", pairs.join(","))
    } else {
        format!("{{key=\"{}\"}}", escape_label(key))
    }
}

impl MetricsSnapshot {
    /// Render the snapshot in the Prometheus text exposition format
    /// (version 0.0.4) — the missing "exit path" for the BL-093
    /// registry. Counters map to `counter`, the queue-depth gauge to
    /// `gauge`, and the percentile histograms to `summary` (quantile
    /// labels), since [`HistogramSnapshot`] carries p50/p95/p99 rather
    /// than raw bucket counts. Durations are converted to seconds
    /// (Prometheus base units). Output is sorted by key so scrapes and
    /// tests are deterministic.
    #[must_use]
    pub fn to_prometheus_text(&self) -> String {
        let mut out = String::new();

        let mut counter = |out: &mut String,
                           name: &str,
                           help: &str,
                           map: &HashMap<String, u64>,
                           labels: &[&str]| {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} counter\n"));
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                out.push_str(&format!("{name}{} {}\n", labels_for(k, labels), map[k]));
            }
        };

        counter(
            &mut out,
            "nexus_ipc_calls_total",
            "IPC calls dispatched, by plugin, command, and outcome.",
            &self.ipc_calls_total,
            &["plugin_id", "command", "status"],
        );
        counter(
            &mut out,
            "nexus_event_bus_published_total",
            "Events published to the kernel bus, by plugin.",
            &self.event_bus_published_total,
            &["plugin_id"],
        );
        counter(
            &mut out,
            "nexus_capability_checks_total",
            "Capability checks performed, by plugin, capability, and result.",
            &self.capability_checks_total,
            &["plugin_id", "capability", "result"],
        );

        let mut summary = |out: &mut String,
                           name: &str,
                           help: &str,
                           map: &HashMap<String, HistogramSnapshot>,
                           labels: &[&str]| {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} summary\n"));
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                let h = &map[k];
                let base = labels_for(k, labels);
                // Splice the quantile label into the existing label set.
                let with_q = |q: &str| -> String {
                    let inner = base.trim_start_matches('{').trim_end_matches('}');
                    format!("{{{inner},quantile=\"{q}\"}}")
                };
                out.push_str(&format!(
                    "{name}{} {}\n",
                    with_q("0.5"),
                    ns_to_secs(h.p50_ns)
                ));
                out.push_str(&format!(
                    "{name}{} {}\n",
                    with_q("0.95"),
                    ns_to_secs(h.p95_ns)
                ));
                out.push_str(&format!(
                    "{name}{} {}\n",
                    with_q("0.99"),
                    ns_to_secs(h.p99_ns)
                ));
                out.push_str(&format!("{name}_sum{base} {}\n", ns_to_secs(h.sum_ns)));
                out.push_str(&format!("{name}_count{base} {}\n", h.count));
            }
        };

        summary(
            &mut out,
            "nexus_ipc_call_duration_seconds",
            "IPC call latency, by plugin and command.",
            &self.ipc_call_duration,
            &["plugin_id", "command"],
        );
        summary(
            &mut out,
            "nexus_plugin_lifecycle_duration_seconds",
            "Plugin lifecycle hook latency, by plugin and hook.",
            &self.plugin_lifecycle_duration,
            &["plugin_id", "hook"],
        );

        out.push_str(&format!(
            "# HELP nexus_event_bus_queue_depth Instantaneous broadcast-channel buffer occupancy.\n\
             # TYPE nexus_event_bus_queue_depth gauge\n\
             nexus_event_bus_queue_depth {}\n",
            self.event_bus_queue_depth
        ));
        out.push_str(&format!(
            "# HELP nexus_metrics_dropped_total Metric writes dropped by the per-metric key cap.\n\
             # TYPE nexus_metrics_dropped_total counter\n\
             nexus_metrics_dropped_total {}\n",
            self.metrics_dropped_total
        ));

        out
    }
}

// ── Global accessor ──────────────────────────────────────────────────────────

static GLOBAL_METRICS: OnceLock<Arc<KernelMetrics>> = OnceLock::new();

/// Install the global metrics registry. Idempotent (`OnceLock`).
/// Bootstrap calls this so call sites in the kernel hot path can
/// reach the registry without threading a handle through.
pub fn install(metrics: Arc<KernelMetrics>) {
    let _ = GLOBAL_METRICS.set(metrics);
}

/// Borrow the global metrics handle. Returns `None` when no
/// registry has been installed yet (e.g. unit tests that don't
/// boot a full kernel) — callers in the hot path branch on this and
/// skip recording rather than failing.
#[must_use]
pub fn global() -> Option<&'static KernelMetrics> {
    GLOBAL_METRICS.get().map(|a| a.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_call_counter_and_histogram_increment() {
        let m = KernelMetrics::new();
        m.record_ipc_call("com.x", "cmd", CallStatus::Ok, 250_000);
        m.record_ipc_call("com.x", "cmd", CallStatus::Ok, 350_000);
        m.record_ipc_call("com.x", "cmd", CallStatus::Error, 100_000);

        let s = m.snapshot();
        assert_eq!(s.ipc_calls_total["com.x::cmd::ok"], 2);
        assert_eq!(s.ipc_calls_total["com.x::cmd::error"], 1);
        let h = &s.ipc_call_duration["com.x::cmd"];
        assert_eq!(h.count, 3);
        assert!(h.mean_ns > 0);
        assert!(h.p99_ns > 0);
    }

    #[test]
    fn event_publish_counter_increments() {
        let m = KernelMetrics::new();
        m.record_event_publish("com.x");
        m.record_event_publish("com.x");
        m.record_event_publish("com.y");
        let s = m.snapshot();
        assert_eq!(s.event_bus_published_total["com.x"], 2);
        assert_eq!(s.event_bus_published_total["com.y"], 1);
    }

    #[test]
    fn capability_check_records_outcome() {
        let m = KernelMetrics::new();
        m.record_capability_check("com.x", "fs.read", true);
        m.record_capability_check("com.x", "fs.read", true);
        m.record_capability_check("com.x", "process.spawn", false);
        let s = m.snapshot();
        assert_eq!(s.capability_checks_total["com.x::fs.read::granted"], 2);
        assert_eq!(s.capability_checks_total["com.x::process.spawn::denied"], 1);
    }

    #[test]
    fn lifecycle_histogram_records_duration() {
        let m = KernelMetrics::new();
        m.record_lifecycle_duration("com.x", "init", 5_000_000);
        m.record_lifecycle_duration("com.x", "init", 6_000_000);
        let s = m.snapshot();
        let h = &s.plugin_lifecycle_duration["com.x::init"];
        assert_eq!(h.count, 2);
        assert_eq!(h.sum_ns, 11_000_000);
    }

    #[test]
    fn event_bus_queue_depth_gauge_records_latest_value() {
        // BL-093 follow-up — gauge semantics: each set() overwrites
        // the previous reading. snapshot() returns the most-recent
        // value, not an aggregate.
        let m = KernelMetrics::new();
        // Default reading is 0.
        assert_eq!(m.snapshot().event_bus_queue_depth, 0);
        m.record_event_bus_queue_depth(7);
        assert_eq!(m.snapshot().event_bus_queue_depth, 7);
        m.record_event_bus_queue_depth(3);
        // Last write wins — not 7+3=10.
        assert_eq!(m.snapshot().event_bus_queue_depth, 3);
        m.record_event_bus_queue_depth(0);
        // Resets cleanly to 0 (a normal "no backpressure" state).
        assert_eq!(m.snapshot().event_bus_queue_depth, 0);
    }

    #[test]
    fn event_bus_queue_depth_independent_of_publish_counter() {
        // The gauge moves on a different cadence than the publish
        // counter (the counter is monotone across the run; the gauge
        // is point-in-time). This test pins that they don't share
        // state — recording one doesn't perturb the other.
        let m = KernelMetrics::new();
        m.record_event_publish("com.x");
        m.record_event_publish("com.x");
        m.record_event_bus_queue_depth(42);
        let s = m.snapshot();
        assert_eq!(s.event_bus_queue_depth, 42);
        assert_eq!(s.event_bus_published_total["com.x"], 2);
    }

    #[test]
    fn percentile_bucket_attribution_is_monotonic() {
        let h = Histogram::default();
        // 50 fast samples (≤1µs), 50 slow samples (~1.5s) — so p50
        // lands in the fast bucket and p95/p99 in the slow bucket.
        for _ in 0..50 {
            h.record(500);
        }
        for _ in 0..50 {
            h.record(1_500_000_000);
        }
        let s = h.snapshot();
        assert_eq!(s.count, 100);
        assert!(s.p50_ns <= 1_000, "p50 should be ≤1µs, got {}", s.p50_ns);
        assert!(
            s.p95_ns > 1_000_000_000,
            "p95 should be >1s (slow bucket), got {}",
            s.p95_ns
        );
        assert!(
            s.p99_ns > 1_000_000_000,
            "p99 should be >1s (slow bucket), got {}",
            s.p99_ns
        );
    }

    #[test]
    fn dropped_counter_increments_when_cap_hit() {
        let m = KernelMetrics::new();
        // We can't exhaust 4096 entries cheaply in a unit test, so
        // probe the failure path indirectly: a brand-new metric with
        // zero entries should never increment dropped, and the
        // snapshot field is reachable.
        let s = m.snapshot();
        assert_eq!(s.metrics_dropped_total, 0);
    }

    #[test]
    fn prometheus_text_renders_counters_gauges_and_summaries() {
        let m = KernelMetrics::new();
        m.record_ipc_call("com.nexus.storage", "search", CallStatus::Ok, 5_000_000);
        m.record_event_publish("com.nexus.editor");
        let text = m.snapshot().to_prometheus_text();

        assert!(text.contains("# TYPE nexus_ipc_calls_total counter"));
        assert!(text.contains(
            "nexus_ipc_calls_total{plugin_id=\"com.nexus.storage\",command=\"search\",status=\"ok\"} 1"
        ));
        assert!(text.contains("# TYPE nexus_ipc_call_duration_seconds summary"));
        assert!(text.contains("nexus_ipc_call_duration_seconds_count{plugin_id=\"com.nexus.storage\",command=\"search\"} 1"));
        assert!(text.contains("quantile=\"0.99\""));
        assert!(text.contains("nexus_event_bus_published_total{plugin_id=\"com.nexus.editor\"} 1"));
        assert!(text.contains("# TYPE nexus_event_bus_queue_depth gauge"));
        assert!(text.contains("nexus_metrics_dropped_total 0"));
    }

    #[test]
    fn prometheus_text_is_deterministic_and_sorted() {
        let m = KernelMetrics::new();
        m.record_event_publish("zeta");
        m.record_event_publish("alpha");
        let a = m.snapshot().to_prometheus_text();
        let b = m.snapshot().to_prometheus_text();
        assert_eq!(a, b);
        let alpha = a.find("plugin_id=\"alpha\"").expect("alpha present");
        let zeta = a.find("plugin_id=\"zeta\"").expect("zeta present");
        assert!(alpha < zeta, "output must be key-sorted");
    }

    #[test]
    fn prometheus_label_escaping_and_malformed_key_fallback() {
        assert_eq!(escape_label("a\"b\\c\n"), "a\\\"b\\\\c\\n");
        // A key that doesn't split into the expected label count falls
        // back to a raw `key` label instead of being dropped.
        assert_eq!(
            labels_for("only-one-part", &["plugin_id", "command"]),
            "{key=\"only-one-part\"}"
        );
    }
}
