use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, Mutex},
};

use crate::{config::ObservabilityConfig, domain::snapshot::PatchSnapshot};
use tracing_subscriber::{
    EnvFilter,
    fmt::{format::Writer, time::FormatTime},
};

struct JiffClock;
impl FormatTime for JiffClock {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(writer, "{}", jiff::Timestamp::now())
    }
}

pub fn init_logging(config: &ObservabilityConfig) -> anyhow::Result<()> {
    let filter = EnvFilter::try_new(&config.log_level)?;
    let writer = match &config.log_file {
        Some(path) => tracing_subscriber::fmt::writer::BoxMakeWriter::new(Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        )),
        None => tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::stdout),
    };
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_timer(JiffClock)
        .with_writer(writer)
        .with_ansi(false);
    if config.log_format == "json" {
        builder.json().try_init().map_err(|e| anyhow::anyhow!(e))?;
    } else {
        builder
            .pretty()
            .try_init()
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum HttpRoute {
    Health,
    Ready,
    Version,
    Installed,
    Pending,
    Summary,
    Export,
    Baseline,
    Metrics,
    Other,
}
impl HttpRoute {
    const ALL: [Self; 10] = [
        Self::Health,
        Self::Ready,
        Self::Version,
        Self::Installed,
        Self::Pending,
        Self::Summary,
        Self::Export,
        Self::Baseline,
        Self::Metrics,
        Self::Other,
    ];
    pub fn from_path(path: &str) -> Self {
        match path {
            "/health" => Self::Health,
            "/ready" => Self::Ready,
            "/version" => Self::Version,
            "/patches" => Self::Installed,
            "/patches/pending" => Self::Pending,
            "/patches/summary" => Self::Summary,
            "/patches/export" => Self::Export,
            "/patches/baseline" => Self::Baseline,
            "/metrics" => Self::Metrics,
            _ => Self::Other,
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::Health => "/health",
            Self::Ready => "/ready",
            Self::Version => "/version",
            Self::Installed => "/patches",
            Self::Pending => "/patches/pending",
            Self::Summary => "/patches/summary",
            Self::Export => "/patches/export",
            Self::Baseline => "/patches/baseline",
            Self::Metrics => "/metrics",
            Self::Other => "unmatched",
        }
    }
}
const STATUS_COUNT: usize = 900;
const BUCKETS: [(f64, &str); 9] = [
    (0.01, "0.01"),
    (0.1, "0.1"),
    (0.5, "0.5"),
    (1.0, "1"),
    (5.0, "5"),
    (20.0, "20"),
    (60.0, "60"),
    (180.0, "180"),
    (f64::INFINITY, "+Inf"),
];
#[derive(Clone, Default)]
struct CollectionStats {
    escaped: String,
    success: u64,
    failure: u64,
    count: u64,
    sum: f64,
    buckets: [u64; 9],
}
struct Registry {
    collectors: Mutex<BTreeMap<String, CollectionStats>>,
    requests: Box<[std::sync::atomic::AtomicU64]>,
    seen_statuses: Box<[std::sync::atomic::AtomicU64]>,
}
#[derive(Clone)]
pub struct Metrics {
    inner: Arc<Registry>,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            inner: Arc::new(Registry {
                collectors: Mutex::new(BTreeMap::new()),
                requests: (0..HttpRoute::ALL.len() * STATUS_COUNT)
                    .map(|_| std::sync::atomic::AtomicU64::new(0))
                    .collect(),
                seen_statuses: (0..HttpRoute::ALL.len() * STATUS_COUNT.div_ceil(64))
                    .map(|_| std::sync::atomic::AtomicU64::new(0))
                    .collect(),
            }),
        }
    }
}
fn label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn family(text: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(text, "# HELP {name} {help}\n# TYPE {name} {kind}");
}
impl Metrics {
    pub fn record_collection(&self, name: &str, elapsed: f64, success: bool) {
        let mut collectors = self
            .inner
            .collectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = collectors
            .entry(name.into())
            .or_insert_with(|| CollectionStats {
                escaped: label(name),
                ..CollectionStats::default()
            });
        if success {
            value.success += 1;
        } else {
            value.failure += 1;
        }
        value.count += 1;
        value.sum += elapsed;
        for (index, (limit, _)) in BUCKETS.iter().enumerate() {
            if elapsed <= *limit {
                value.buckets[index] += 1;
            }
        }
    }
    /// Bounded route/status counters require no allocation or registry lock on requests.
    pub fn record_request(&self, route: HttpRoute, status: u16) {
        if let Some(status) = status
            .checked_sub(100)
            .filter(|status| usize::from(*status) < STATUS_COUNT)
        {
            self.inner.requests[route as usize * STATUS_COUNT + usize::from(status)]
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let word = &self.inner.seen_statuses
                [route as usize * STATUS_COUNT.div_ceil(64) + usize::from(status) / 64];
            let mask = 1 << (usize::from(status) % 64);
            if word.load(std::sync::atomic::Ordering::Relaxed) & mask == 0 {
                word.fetch_or(mask, std::sync::atomic::Ordering::Release);
            }
        }
    }
    pub fn render(
        &self,
        snapshot: &PatchSnapshot,
        stale_after_secs: u64,
        now: jiff::Timestamp,
    ) -> String {
        // Formatting is outside the lock; collector histogram values remain one coherent sample.
        let collectors = self
            .inner
            .collectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut text = String::with_capacity(4096);
        family(
            &mut text,
            "patchpulse_collect_success_total",
            "counter",
            "Successful collector invocations.",
        );
        for values in collectors.values() {
            let _ = writeln!(
                text,
                "patchpulse_collect_success_total{{collector=\"{}\"}} {}",
                values.escaped, values.success
            );
        }
        family(
            &mut text,
            "patchpulse_collect_failure_total",
            "counter",
            "Failed collector invocations.",
        );
        for values in collectors.values() {
            let _ = writeln!(
                text,
                "patchpulse_collect_failure_total{{collector=\"{}\"}} {}",
                values.escaped, values.failure
            );
        }
        family(
            &mut text,
            "patchpulse_collect_duration_seconds",
            "histogram",
            "Collector invocation duration.",
        );
        for values in collectors.values() {
            for (index, (_, limit)) in BUCKETS.iter().enumerate() {
                let _ = writeln!(
                    text,
                    "patchpulse_collect_duration_seconds_bucket{{collector=\"{}\",le=\"{limit}\"}} {}",
                    values.escaped, values.buckets[index]
                );
            }
            let _ = writeln!(
                text,
                "patchpulse_collect_duration_seconds_sum{{collector=\"{}\"}} {}\npatchpulse_collect_duration_seconds_count{{collector=\"{}\"}} {}",
                values.escaped, values.sum, values.escaped, values.count
            );
        }
        family(
            &mut text,
            "patchpulse_http_requests_total",
            "counter",
            "HTTP responses by route and status.",
        );
        for route in HttpRoute::ALL {
            let counters = &self.inner.requests
                [route as usize * STATUS_COUNT..(route as usize + 1) * STATUS_COUNT];
            let width = STATUS_COUNT.div_ceil(64);
            for (word_index, seen) in self.inner.seen_statuses
                [route as usize * width..(route as usize + 1) * width]
                .iter()
                .enumerate()
            {
                let mut bits = seen.load(std::sync::atomic::Ordering::Acquire);
                while bits != 0 {
                    let bit = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    let offset = word_index * 64 + bit;
                    let count = counters[offset].load(std::sync::atomic::Ordering::Relaxed);
                    let _ = writeln!(
                        text,
                        "patchpulse_http_requests_total{{path=\"{}\",status=\"{}\"}} {count}",
                        route.path(),
                        offset + 100
                    );
                }
            }
        }
        for (name, help, value) in [
            (
                "patchpulse_snapshot_age_seconds",
                "Oldest available backend snapshot age; -1 before collection.",
                snapshot.age_seconds(now).unwrap_or(-1),
            ),
            (
                "patchpulse_installed_patches",
                "Installed patch count.",
                snapshot.installed.len() as i64,
            ),
            (
                "patchpulse_pending_patches",
                "Pending patch count.",
                snapshot.pending.len() as i64,
            ),
            (
                "patchpulse_stale",
                "Snapshot incomplete, failed, or beyond age threshold.",
                i64::from(snapshot.is_stale(now, stale_after_secs)),
            ),
            (
                "patchpulse_reboot_required",
                "System or update reports a required reboot.",
                i64::from(snapshot.reboot_required),
            ),
        ] {
            family(&mut text, name, "gauge", help);
            let _ = writeln!(text, "{name} {value}");
        }
        text
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_is_cumulative_and_labels_are_escaped() {
        let metrics = Metrics::default();
        metrics.record_collection("test\"\\\n", 0.3, true);
        metrics.record_collection("test\"\\\n", 4.0, false);
        let text = metrics.render(&PatchSnapshot::default(), 10, jiff::Timestamp::now());
        assert!(text.contains("collector=\"test\\\"\\\\\\n\""));
        assert!(text.contains("le=\"0.5\"} 1"));
        assert!(text.contains("le=\"+Inf\"} 2"));
        assert!(text.contains("patchpulse_stale 1"));
        assert!(text.contains("patchpulse_snapshot_age_seconds -1"));
    }
    #[test]
    fn http_counters_are_bounded_and_preserve_exact_statuses() {
        let metrics = Metrics::default();
        for status in [200, 404, 999] {
            metrics.record_request(HttpRoute::from_path("/unknown/client-input"), status);
        }
        metrics.record_request(HttpRoute::Health, 99);
        let text = metrics.render(&PatchSnapshot::default(), 10, jiff::Timestamp::now());
        for status in [200, 404, 999] {
            assert!(text.contains(&format!("path=\"unmatched\",status=\"{status}\"}} 1")));
        }
        assert!(!text.contains("client-input"));
        assert!(!text.contains("status=\"99\""));
    }
}
