#![allow(dead_code)] // Not every item here is reached from every test file.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use tempfile::TempDir;
use tracing::Level;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

/// Global mutex to serialize tests that touch the filesystem/env.
///
/// Also serializes every test that reads or records into the process-global
/// telemetry metrics registry (`mcp_core::telemetry::metrics::global()`):
/// every such test needs a [`TestEnv`] for its own `TIMECLOCK_DATA_DIR`
/// isolation anyway, and the guard below is held for the test's whole body,
/// so two tests can never race on the same counter or histogram series
/// (mcp-core#40 lesson 6: "the metrics registry is process-global").
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that:
/// 1. Acquires the global test mutex (serializing all storage tests).
/// 2. Creates a fresh temporary directory for data.
/// 3. Points `TIMECLOCK_DATA_DIR` at it.
/// 4. Restores the environment on drop.
pub struct TestEnv {
    _dir: TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl TestEnv {
    pub fn new() -> Self {
        Self::with_subdir(None)
    }

    /// Like [`Self::new`], but nests the data directory under `segment`
    /// inside the temp root, so the effective `TIMECLOCK_DATA_DIR` path
    /// contains `segment` as a path component.
    ///
    /// Used by tests that must prove a value derived from the data
    /// directory's path -- the full path a storage error's `Display`
    /// embeds -- does or does not reach a particular log level. `segment`
    /// stands in for the operator's home directory that `data_dir()`
    /// resolves through outside tests.
    pub fn with_path_segment(segment: &str) -> Self {
        Self::with_subdir(Some(segment))
    }

    fn with_subdir(segment: Option<&str>) -> Self {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = TempDir::new().expect("failed to create temp dir");
        let data_dir = match segment {
            Some(s) => {
                let d = base.path().join(s);
                std::fs::create_dir_all(&d).expect("failed to create sentinel-bearing data dir");
                d
            }
            None => base.path().to_path_buf(),
        };
        // Safety: single-threaded at this point due to mutex.
        unsafe {
            std::env::set_var("TIMECLOCK_DATA_DIR", &data_dir);
        }
        Self {
            _dir: base,
            _guard: guard,
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("TIMECLOCK_DATA_DIR");
        }
    }
}

// ---------------------------------------------------------------------------
// A capturing `tracing` layer.
//
// The telemetry acceptance criteria (mcp-core#40) are about what this
// server's own spans and events actually carry, so a test has to read them
// back rather than assert a constant against itself. Adapted from
// mcp-core's own `tests/support/mod.rs`, which each consumer copies rather
// than shares.
// ---------------------------------------------------------------------------

/// One span, as the subscriber saw it. A span whose fields are recorded
/// after creation appears a second time, carrying only what was recorded
/// then.
#[derive(Clone, Debug)]
pub struct RecordedSpan {
    /// The span's name.
    pub name: &'static str,
    /// Field name to its rendered value.
    pub fields: BTreeMap<String, String>,
}

/// One event, as the subscriber saw it.
#[derive(Clone, Debug)]
pub struct RecordedEvent {
    /// The level the event was emitted at.
    pub level: Level,
    /// Field name to its rendered value. The message is the `message` field.
    pub fields: BTreeMap<String, String>,
}

/// Everything one captured run produced.
#[derive(Clone, Debug, Default)]
pub struct Recorded {
    /// Spans, in the order they opened.
    pub spans: Vec<RecordedSpan>,
    /// Events, in the order they were emitted.
    pub events: Vec<RecordedEvent>,
}

impl Recorded {
    /// A short rendering for an assertion message.
    pub fn span_summary(&self) -> Vec<String> {
        self.spans
            .iter()
            .map(|span| format!("{}{:?}", span.name, span.fields))
            .collect()
    }

    /// A short rendering for an assertion message.
    pub fn event_summary(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|event| format!("{}{:?}", event.level, event.fields))
            .collect()
    }
}

/// Run `body` (synchronous) with a capturing subscriber installed on this
/// thread, and return what it emitted.
pub fn capture<F: FnOnce()>(body: F) -> Recorded {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, body);
    capture.take()
}

/// Run `body` (an async closure, given its own current-thread runtime) with
/// a capturing subscriber installed, and return what it emitted.
///
/// The runtime is built and driven inside `with_default`'s scope rather than
/// via `#[tokio::test]` on the caller, so a test using this stays a plain
/// `#[test]` and never nests one Tokio runtime inside another.
pub fn capture_async<F, Fut>(body: F) -> Recorded
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime");
        runtime.block_on(body());
    });
    capture.take()
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Recorded>>);

impl Capture {
    fn take(self) -> Recorded {
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .clone()
    }
}

impl<S> Layer<S> for Capture
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan {
                name: attrs.metadata().name(),
                fields,
            });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let name = ctx.span(id).map_or("<closed>", |span| span.name());
        let mut fields = BTreeMap::new();
        values.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan { name, fields });
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .events
            .push(RecordedEvent {
                level: *event.metadata().level(),
                fields,
            });
    }
}

struct Collector<'a>(&'a mut BTreeMap<String, String>);

impl Visit for Collector<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

/// The lifetime total of one counter series, or zero when it has never been
/// recorded. The metrics registry is process-wide, so every assertion built
/// on this is a delta relative to a "before" call.
pub fn counter_total(name: &str, labels: &[mcp_core::telemetry::metrics::Label]) -> u64 {
    mcp_core::telemetry::metrics::global()
        .snapshot()
        .counters
        .iter()
        .find(|counter| counter.name == name && same_labels(&counter.labels, labels))
        .map_or(0, |counter| counter.total)
}

fn same_labels(
    recorded: &[mcp_core::telemetry::metrics::Label],
    wanted: &[mcp_core::telemetry::metrics::Label],
) -> bool {
    recorded.len() == wanted.len()
        && wanted.iter().all(|want| {
            recorded
                .iter()
                .any(|have| have.key() == want.key() && have.value() == want.value())
        })
}
