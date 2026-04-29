//! HarnessV1 turn lifecycle + JSONL store smoke tests.

use std::time::Instant;
use std::sync::{Mutex, OnceLock};

use hyper_stigmergy::harness::{HarnessEvent, HarnessRuntime, HarnessState, HarnessStore};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvSet<'a> {
    key: &'a str,
}

impl<'a> EnvSet<'a> {
    fn new(key: &'a str, value: &str) -> Self {
        std::env::set_var(key, value);
        Self { key }
    }
}

impl Drop for EnvSet<'_> {
    fn drop(&mut self) {
        std::env::remove_var(self.key);
    }
}

#[test]
fn store_appends_jsonl_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("events.jsonl");
    let store = HarnessStore::new(log.clone(), None).expect("store");
    let ev = HarnessEvent::transition(
        "trace-a",
        "agent-1",
        "unit",
        "task-9",
        3,
        HarnessState::Queued,
        HarnessState::Running,
        None,
        None,
        None,
    );
    store.append_event(&ev).expect("append");
    let txt = std::fs::read_to_string(&log).expect("read");
    assert!(txt.contains("trace-a"));
    assert!(txt.contains("task-9"));
    assert!(txt.contains("running"));
    let line_count = txt.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(line_count, 1);
}

#[test]
fn noop_runtime_turn_cycle_no_io() {
    let mut rt = HarnessRuntime::noop();
    let t0 = Instant::now();
    rt.turn_begin("tid", 0);
    rt.turn_end("tid", 0, t0, None);
}

#[test]
fn from_env_logs_two_lines_per_turn() {
    let _guard = env_lock().lock().expect("env lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("harness.jsonl");
    let _log_guard = EnvSet::new("HSM_HARNESS_LOG", log.to_str().unwrap());
    let _trace_guard = EnvSet::new("HSM_HARNESS_TRACE_ID", "trace-unit");
    let mut rt = HarnessRuntime::from_env("test-runner").expect("from_env");
    let t0 = Instant::now();
    rt.turn_begin("t1", 0);
    rt.turn_end("t1", 0, t0, None);
    let txt = std::fs::read_to_string(&log).expect("read log");
    let n = txt.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(n, 2);
    assert!(txt.contains("queued"));
    assert!(txt.contains("running"));
    assert!(txt.contains("completed"));
}

#[test]
fn from_env_logs_failed_turn() {
    let _guard = env_lock().lock().expect("env lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("harness.jsonl");
    let _log_guard = EnvSet::new("HSM_HARNESS_LOG", log.to_str().unwrap());
    let _trace_guard = EnvSet::new("HSM_HARNESS_TRACE_ID", "trace-err");
    let mut rt = HarnessRuntime::from_env("test-runner").expect("from_env");
    let t0 = Instant::now();
    rt.turn_begin("t1", 0);
    rt.turn_end("t1", 0, t0, Some("upstream timeout"));
    let txt = std::fs::read_to_string(&log).expect("read log");
    assert!(txt.contains("failed"));
    assert_txt_contains_outcome_error(&txt);
}

fn assert_txt_contains_outcome_error(txt: &str) {
    assert!(
        txt.contains("\"outcome\":\"error\"") || txt.contains(r#""outcome": "error""#),
        "expected outcome error in {}",
        txt
    );
}
