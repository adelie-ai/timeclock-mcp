# AGENT.md — timeclock-mcp

Guidance for AI agents and automated tools working in this repository.

## What this project is

`timeclock-mcp` is a small, local-first **MCP server** written in Rust for tracking billable work sessions. It lets an LLM agent (or a human via a client) clock in and out of named projects, query sessions by time window, and export results as JSON or CSV for use in external tools like Excel.

It is **not** a billing system. It is a time-tracking data source.

## Repository layout

```
Cargo.toml          — package manifest (edition 2024, strict lints)
src/
  main.rs           — CLI entrypoint and server runner
docs/
  spec.md           — authoritative design spec (data model, tools, decisions)
```

## Build & run

```bash
# build
cargo build --release

# run
./target/release/timeclock-mcp serve   # stdio transport (default)

# check + test
cargo check
cargo test
cargo clippy
```

Lints are set to `deny` for both `warnings` and `clippy::all`. The build must be clean.

## Key design decisions (see docs/spec.md for full rationale)

| Topic | Decision |
|---|---|
| Storage | JSONL, one file per project: `~/.local/share/desktop-assistant/timeclock/{project_id}.jsonl` |
| Append-only | Corrections append a new record with the same `session_id`; last record wins |
| Active sessions | At most one active session **per project** (not a global lock) |
| Timestamps | Always RFC3339 UTC. No local timezone conversion in the server |
| Transport | `stdio` required; `websocket` optional |

## MCP tools exposed

| Tool | Purpose |
|---|---|
| `timeclock_project_list` | List all known projects |
| `timeclock_project_upsert` | Create or rename a project |
| `timeclock_project_delete` | Delete a project; refuses if sessions exist unless `delete_entries=true` |
| `timeclock_clock_in` | Start a session for a project |
| `timeclock_clock_out` | End the active session for a project |
| `timeclock_session_get_active` | Return active sessions (optionally filtered by project) |
| `timeclock_session_query` | Query sessions by time window; supports JSON or CSV output |
| `timeclock_session_add_note` | Append a timestamped note to any session (active or closed) |
| `timeclock_session_correct` | Amend time fields or tags on an existing session |
| `timeclock_session_delete` | Permanently delete a session by ID |

Full input/output schemas are in [docs/spec.md](docs/spec.md).

## Storage format

Each project's sessions live in `~/.local/share/desktop-assistant/timeclock/{project_id}.jsonl`. Every line is a self-contained JSON session object:

The path follows XDG conventions: `$XDG_DATA_HOME/desktop-assistant/timeclock/` (falling back to `~/.local/share/desktop-assistant/timeclock/`). Override with the `TIMECLOCK_DATA_DIR` env var.

```json
{"session_id":"<uuid>","project_id":"acme","time_in":"2026-02-19T14:00:00Z","time_out":"2026-02-19T16:30:00Z","note":"initial design","tags":[],"duration_seconds":9000}
```

Reading a project's sessions means reading all lines and keeping the last record seen for each `session_id` (to handle corrections).

## Adding a new tool

1. Implement the handler logic as a focused module (or function) in `src/`.
2. Register it in the server's tool dispatch.
3. Document it in [docs/spec.md](docs/spec.md) under **MCP surface area**.
4. Add unit tests covering happy path and relevant error cases.

## Dependencies (notable)

- `axum` + `tokio` — async runtime and HTTP/WebSocket transport
- `clap` — CLI argument parsing
- `serde` / `serde_json` — serialization
- `uuid` — session ID generation
- `thiserror` — structured error types

## Coding conventions

- Rust edition **2024**.
- `[lints.rust] warnings = "deny"` and `[lints.clippy] all = "deny"` — no warnings allowed.
- All timestamps produced or stored by the server must be UTC.
- Prefer explicit error types over `unwrap`/`expect` in non-test code.
- Keep tool handlers thin; push logic into dedicated functions that are easy to unit-test.
