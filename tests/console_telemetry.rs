//! Process-level acceptance criteria for timeclock-mcp's telemetry
//! (mcp-core#40): what a default-feature build resolves, and what actually
//! reaches stdout and stderr when the server runs for real.
//!
//! A console-text test cannot see a span-field leak (mcp-core#40 lesson 7):
//! it proves protocol purity and that no *event* carries content, nothing
//! about span fields. The in-process capturing-layer tests in
//! `src/storage.rs` and `src/service.rs` cover span fields directly; this
//! file is the other half of "write both content tests".
//!
//! `sentinel_never_reaches_stdout_or_info_stderr` is table-driven over the
//! whole tool list rather than one tool (mcp-core#40 lesson 8: a
//! single-tool version of this kind of test caught a leak on the one
//! operation it exercised and missed it on two others). This file has no
//! access to `src/service.rs`'s own table -- this crate has no `lib`
//! target, so a `tests/` file cannot import from `src/` -- so the coverage
//! check here reads the *real* registered tool set back from a live
//! `tools/list` call instead of trusting a second hand-written list to stay
//! in sync with the first.

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// The value planted in every content-shaped argument below: a project id,
/// a project name, a session note, a note-annotation text, a tag, or a
/// session id -- all content under the level contract (mcp-core#40 D10).
const SENTINEL: &str = "MARKER-timeclock-console-sentinel-3a9f0e";

/// One entry per registered tool: its name, and an argument set with
/// [`SENTINEL`] planted in every string-shaped field the tool accepts.
/// `timeclock_project_list` takes no arguments, so its entry is empty; it
/// stays in the table so the coverage check still requires it to be listed.
fn leak_test_cases() -> Vec<(&'static str, Value)> {
    vec![
        ("timeclock_project_list", json!({})),
        (
            "timeclock_project_upsert",
            json!({ "project_id": SENTINEL, "name": SENTINEL }),
        ),
        (
            "timeclock_project_delete",
            json!({ "project_id": SENTINEL, "delete_entries": false }),
        ),
        (
            "timeclock_clock_in",
            json!({ "project_id": SENTINEL, "note": SENTINEL, "tags": [SENTINEL] }),
        ),
        (
            "timeclock_clock_out",
            json!({ "project_id": SENTINEL, "note": SENTINEL }),
        ),
        (
            "timeclock_session_get_active",
            json!({ "project_id": SENTINEL }),
        ),
        (
            "timeclock_session_query",
            json!({
                "start": "2026-01-01T00:00:00Z",
                "end": "2026-01-02T00:00:00Z",
                "project_ids": [SENTINEL],
                "format": "json",
            }),
        ),
        (
            "timeclock_session_add_note",
            json!({ "session_id": SENTINEL, "text": SENTINEL }),
        ),
        (
            "timeclock_session_delete",
            json!({ "session_id": SENTINEL }),
        ),
        (
            "timeclock_session_correct",
            json!({ "session_id": SENTINEL, "tags": [SENTINEL] }),
        ),
    ]
}

/// Spawn timeclock-mcp under `RUST_LOG=trace` against `data_dir`, feed it
/// `requests` over stdin, and return (stdout, stderr) once it exits.
fn run_requests(data_dir: &std::path::Path, requests: &[Value]) -> (String, String) {
    let exe = env!("CARGO_BIN_EXE_timeclock-mcp");
    let mut child = Command::new(exe)
        .args(["serve", "--transport", "stdio"])
        .env("RUST_LOG", "trace")
        .env("TIMECLOCK_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("timeclock-mcp must start");

    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("timeclock-mcp must accept its input");
        }
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("timeclock-mcp must finish");
    assert!(
        output.status.success(),
        "timeclock-mcp must exit cleanly, otherwise an empty output proves nothing: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    (
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

/// Every stdout line, parsed and checked to carry the JSON-RPC envelope.
/// Panics naming the offending line if one does not parse or lacks it.
fn assert_stdout_is_pure_jsonrpc(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
                panic!("every stdout line must be JSON-RPC, but {line:?} is not: {e}")
            });
            assert_eq!(
                value.get("jsonrpc").and_then(Value::as_str),
                Some("2.0"),
                "every stdout line must carry the JSON-RPC envelope: {line:?}"
            );
            value
        })
        .collect()
}

/// AC (epic AC2): a default-feature build resolves no `opentelemetry*`
/// crate. The `otel` feature is the only thing that adds one.
#[test]
fn default_build_pulls_no_opentelemetry() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    let output = Command::new(cargo)
        .args(["tree", "--edges", "normal", "--prefix", "none", "--locked"])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("cargo tree must run");

    assert!(
        output.status.success(),
        "cargo tree failed, so this criterion is unproven: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_lowercase().starts_with("opentelemetry"))
        .collect();

    assert!(
        found.is_empty(),
        "a default-feature build must resolve no opentelemetry crate, but it resolved: {found:?}"
    );
}

/// AC (mcp-core#40, non-negotiable #3): with `RUST_LOG=trace`, every line
/// timeclock-mcp writes to stdout parses as JSON-RPC, and the logs land on
/// stderr instead -- even while a real corrupt storage line fires the
/// WARN/DEBUG pair from `src/storage.rs` in the same run, and every
/// registered tool is called once. The stdio transport frames JSON-RPC on
/// stdout, so one stray log line there corrupts the protocol stream.
#[test]
fn stdout_carries_only_jsonrpc_at_trace_log_level() {
    let data_dir = tempfile::tempdir().expect("tempdir for TIMECLOCK_DATA_DIR");
    std::fs::write(data_dir.path().join("_projects.jsonl"), "not valid json\n")
        .expect("seed a corrupt projects registry line");

    let cases = leak_test_cases();
    let mut requests = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    ];
    for (i, (name, args)) in cases.iter().enumerate() {
        requests.push(json!({
            "jsonrpc": "2.0",
            "id": 2 + i as u64,
            "method": "tools/call",
            "params": {"name": name, "arguments": args},
        }));
    }
    // A protocol-level error (unknown tool) -- a different response shape,
    // on the same stdout stream.
    let unknown_tool_id = requests.len() as u64;
    requests.push(json!({
        "jsonrpc": "2.0",
        "id": unknown_tool_id,
        "method": "tools/call",
        "params": {"name": "nonexistent_tool", "arguments": {}},
    }));

    let (stdout, stderr) = run_requests(data_dir.path(), &requests);

    let replies = assert_stdout_is_pure_jsonrpc(&stdout);
    assert_eq!(
        replies.len(),
        requests.len(),
        "timeclock-mcp must answer every request it was sent"
    );

    assert!(
        stderr.contains("WARN") && stderr.contains("DEBUG"),
        "the corrupt-line WARN/DEBUG pair must have actually fired for this test to prove \
         anything; stderr was: {stderr:?}"
    );
}

/// AC (mcp-core#40 D10, epic AC7, lesson 8): calling every registered tool
/// once with a sentinel planted in each of its content-shaped arguments
/// must never put that sentinel on stdout outside a legitimate echo, nor
/// into any INFO-or-louder stderr line.
///
/// Self-guarding: the table is checked against the server's own live
/// `tools/list` response, so a tool added without a row here fails this
/// test on the mismatch rather than going unchecked.
#[test]
fn sentinel_never_reaches_info_or_louder_stderr() {
    let data_dir = tempfile::tempdir().expect("tempdir for TIMECLOCK_DATA_DIR");
    let cases = leak_test_cases();

    let mut requests = vec![
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    ];
    for (i, (name, args)) in cases.iter().enumerate() {
        requests.push(json!({
            "jsonrpc": "2.0",
            "id": 2 + i as u64,
            "method": "tools/call",
            "params": {"name": name, "arguments": args},
        }));
    }

    let (stdout, stderr) = run_requests(data_dir.path(), &requests);
    let replies = assert_stdout_is_pure_jsonrpc(&stdout);

    let tools_list_reply = replies
        .iter()
        .find(|r| r.get("id").and_then(Value::as_u64) == Some(1))
        .expect("tools/list must reply");
    let registered: BTreeSet<&str> = tools_list_reply["result"]["tools"]
        .as_array()
        .expect("tools/list result.tools must be an array")
        .iter()
        .map(|t| t["name"].as_str().expect("each tool has a name"))
        .collect();
    let tested: BTreeSet<&str> = cases.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        registered, tested,
        "this test's table must cover exactly the registered tool set (mcp-core#40 \
         lesson 8): a tool present in one set but not the other is untested or stale"
    );

    // A project_upsert/clock_in echo the sentinel back as the created
    // project's/session's own name or note, legitimately -- that is
    // returned data, not a log leak. So this checks stderr only, not
    // stdout: stdout's purity is `stdout_carries_only_jsonrpc_at_trace_log_
    // level`'s job, and content in a *result* is expected, unlike content
    // in a *log line*.
    for line in stderr.lines() {
        let is_info_or_louder =
            line.contains(" INFO ") || line.contains(" WARN ") || line.contains(" ERROR ");
        if !is_info_or_louder {
            continue;
        }
        assert!(
            !line.contains(SENTINEL),
            "the sentinel reached an INFO-or-louder stderr line: {line:?}"
        );
    }
}
