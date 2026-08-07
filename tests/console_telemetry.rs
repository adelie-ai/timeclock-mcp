//! Process-level acceptance criteria for timeclock-mcp's telemetry
//! (mcp-core#40): what a default-feature build resolves, and what actually
//! reaches stdout when the server runs for real.
//!
//! A console-text test cannot see a span-field leak (mcp-core#40 lesson 7):
//! it proves protocol purity and that no *event* carries content, nothing
//! about span fields. The in-process capturing-layer tests in
//! `src/storage.rs` and `src/service.rs` cover span fields directly; this
//! file is the other half of "write both content tests".

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

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
/// WARN/DEBUG pair from `src/storage.rs` in the same run. The stdio
/// transport frames JSON-RPC on stdout, so one stray log line there
/// corrupts the protocol stream.
#[test]
fn stdout_carries_only_jsonrpc_at_trace_log_level() {
    let exe = env!("CARGO_BIN_EXE_timeclock-mcp");
    let data_dir = tempfile::tempdir().expect("tempdir for TIMECLOCK_DATA_DIR");
    std::fs::write(data_dir.path().join("_projects.jsonl"), "not valid json\n")
        .expect("seed a corrupt projects registry line");

    let requests = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        // Triggers storage::read_projects, which must skip the corrupt line
        // above (logging its WARN/DEBUG pair) instead of failing the call.
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "timeclock_project_list", "arguments": {}},
        }),
        // A protocol-level error (unknown tool) -- a different response
        // shape, on the same stdout stream.
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "nonexistent_tool", "arguments": {}},
        }),
    ];

    let mut child = Command::new(exe)
        .args(["serve", "--transport", "stdio"])
        .env("RUST_LOG", "trace")
        .env("TIMECLOCK_DATA_DIR", data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("timeclock-mcp must start");

    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for request in &requests {
            writeln!(stdin, "{request}").expect("timeclock-mcp must accept its input");
        }
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("timeclock-mcp must finish");
    assert!(
        output.status.success(),
        "timeclock-mcp must exit cleanly, otherwise an empty stdout proves nothing: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

    let mut replies = 0;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("every stdout line must be JSON-RPC, but {line:?} is not: {e}")
        });
        assert_eq!(
            value.get("jsonrpc").and_then(Value::as_str),
            Some("2.0"),
            "every stdout line must carry the JSON-RPC envelope: {line:?}"
        );
        replies += 1;
    }
    assert_eq!(replies, 4, "timeclock-mcp must answer all four requests");

    assert!(
        stderr.contains("WARN") && stderr.contains("DEBUG"),
        "the corrupt-line WARN/DEBUG pair must have actually fired for this test to prove \
         anything; stderr was: {stderr:?}"
    );
}
