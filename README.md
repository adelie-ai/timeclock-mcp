# timeclock-mcp

A small, local-first MCP server for tracking billable work sessions, grouped by
project, kept in on-disk JSONL files with no external account or auth. It lets
an LLM agent (or a human through a client) clock in and out of named projects,
query sessions by time window, and export results as JSON or CSV.

It is not a billing system. It is a time-tracking data source.

## Tools

All tools use underscore-separated names with no dots.

| Tool | Purpose |
|---|---|
| `timeclock_project_list` | List all known projects |
| `timeclock_project_upsert` | Create or update a project |
| `timeclock_project_delete` | Delete a project (refuses if it has sessions, unless `delete_entries=true`) |
| `timeclock_clock_in` | Start a new session for a project |
| `timeclock_clock_out` | End the active session for a project |
| `timeclock_session_get_active` | List currently active sessions |
| `timeclock_session_query` | Report tracked time over a date range, as JSON or CSV |
| `timeclock_session_add_note` | Append a timestamped note to a session |
| `timeclock_session_delete` | Permanently delete a session |
| `timeclock_session_correct` | Amend a session's time or tags |

See `docs/spec.md` for the full data model and design rationale.

## Quick start

```bash
cargo build --release
./target/release/timeclock-mcp serve   # stdio transport (default)
```

Data lives under `$XDG_DATA_HOME/desktop-assistant/timeclock` (or
`~/.local/share/desktop-assistant/timeclock` when `XDG_DATA_HOME` is unset),
overridable with the `TIMECLOCK_DATA_DIR` environment variable.

## Logging

timeclock-mcp gets its logging, tracing and metrics from `mcp-core`, which
installs them through
[adelie-telemetry](https://github.com/adelie-ai/adelie-telemetry). This
section covers what is specific to this server; `mcp-core`'s own README has
the full contract.

### Where it goes, and how much

**stderr, always.** This server speaks stdio, and the transport frames
JSON-RPC on stdout, so a log line there would corrupt the protocol -- this
holds even at `RUST_LOG=trace`.

`RUST_LOG` sets the filter. Unset means `info`.

```sh
RUST_LOG=debug timeclock-mcp serve
```

### The level contract, and why it matters here

| Level | Carries |
|---|---|
| INFO | ids, counts, durations, tool names. **Never content.** |
| DEBUG | tool arguments, and a corrupt storage line's full path and parse error. |

This server records billable time against clients and projects, so a project
name, a session note, and a note-annotation's text are content: they never
reach a span field or an INFO line, at any tool handler. `storage::
read_projects` and `storage::read_sessions` skip a line they cannot parse
instead of failing the whole read: the affected store (`projects` or
`sessions`) and a bounded reason are logged at WARN as identifiers, the same
class of value as a tool name. The line's full path is not: it resolves
through the operator's home directory by default, and the parse error's text
can quote a snippet of the line's own malformed JSON. Both move to DEBUG
instead. `timeclock.storage_failures`, labelled by that same bounded
`reason`, counts these regardless of the log level -- see Metrics below.

### Metrics

`mcp-core`'s dispatch layer already records a tool-call counter and a latency
histogram, by tool name and outcome, for every call this server handles; see
`mcp-core`'s README for the full list. This server adds one metric of its
own:

| Metric | Labels | Meaning |
|---|---|---|
| `timeclock.storage_failures` | `reason` | A storage read that skipped a corrupt line, by why. |

### Exporting to a collector

Off by default. Turn it on with the `otel` feature:

```sh
cargo build --features otel
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./target/debug/timeclock-mcp serve
```

With the feature off, no opentelemetry crate is resolved at all -- `cargo
tree` on a default build shows none. With it on, traces, metrics and log
records export over the standard `OTEL_EXPORTER_OTLP_*` / `OTEL_RESOURCE_*`
environment variables; there are no server-specific flags or variables. See
`mcp-core`'s README for the full variable list and
[adelie-telemetry](https://github.com/adelie-ai/adelie-telemetry)'s for
transport and TLS details.

With no collector configured, the metrics registry still accumulates and
still writes a periodic summary to stderr, so a plain `cargo install` build
reports real numbers without any extra setup.
