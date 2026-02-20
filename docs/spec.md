# timeclock-mcp — Spec (WIP)

## Goal
Provide a small, local-first **timeclock** MCP server for tracking billable work sessions.

Core capabilities:
- Track **project**
- Record **time in** / **time out** (i.e., sessions)
- Query sessions by **time period** to support billing/invoicing

Non-goals (for initial version):
- Payroll/taxes
- Automatic idle detection
- Complex approvals/workflows

## Data model

### Project
A logical label used to group time sessions.

Minimum fields:
- `project_id` (string; stable identifier, may equal name initially)
- `name` (string)

### Time session
Represents a contiguous work interval.

Minimum fields:
- `session_id` (string/uuid)
- `project_id` (string)
- `time_in` (RFC3339 timestamp, always UTC)
- `time_out` (RFC3339 timestamp, always UTC; nullable until clock-out)
- `note` (optional string)
- `tags` (optional list of strings)

Derived fields (computed):
- `duration_seconds` = `time_out - time_in` when `time_out` exists

### Validation rules
- A session must have a `time_in`.
- `time_out` must be >= `time_in`.
- At most one **active** session per project at a time.

## Storage
Local persistence.

**Decision: JSONL**, one file per project.
- Path: `~/.local/share/desktop-assistant/timeclock/{project_id}.jsonl`
  (follows XDG: `$XDG_DATA_HOME/desktop-assistant/timeclock/`; override with `TIMECLOCK_DATA_DIR` env var)
- Project registry: `_projects.jsonl` in the same directory
- Each line is a JSON-encoded session record.
- Append-only writes; corrections are handled by appending a replacement record
  with the same `session_id` (last record for a given `session_id` wins).

## MCP surface area
This server should expose a small tool set.

### Tools

#### `timeclock_project_list`
List known projects.

Input:
- none

Output:
- `projects: [{ project_id, name }]`

#### `timeclock_project_upsert`
Create/update a project.

Input:
- `project_id` (optional; if omitted, derive from `name`)
- `name`

Output:
- `project: { project_id, name }`

#### `timeclock_project_delete`
Delete a project from the registry.

Input:
- `project_id` (required)
- `delete_entries` (optional bool; default `false`)

Behaviour:
- If the project has any recorded sessions and `delete_entries` is `false`, the operation is **refused** (molly guard). The error message names the session count and instructs the caller to pass `delete_entries=true`.
- If `delete_entries=true`, the session file is removed before deleting the project record.

Output:
- `{ deleted_project, sessions_deleted, session_count }`

#### `timeclock_clock_in`
Start a new session.

Input:
- `project_id`
- `time_in` (optional; default: now)
- `note` (optional)
- `tags` (optional)

Output:
- `session: { ... }`

Errors:
- if there is already an active session for the given project

#### `timeclock_clock_out`
End the active session for a project.

Input:
- `project_id`
- `time_out` (optional; default: now)
- `note` (optional; replaces existing note if provided)

Output:
- `session: { ... }`

Errors:
- if there is no active session for the given project

#### `timeclock_session_get_active`
Return all currently active sessions, optionally filtered to a single project.

Input:
- `project_id` (optional; if omitted, returns all active sessions across all projects)

Output:
- `sessions: [{ ... }]`

#### `timeclock_session_query`
Query sessions for a time period across one, many, or all projects.

Input:
- `start` (RFC3339, UTC)
- `end` (RFC3339, UTC)
- `project_ids` (optional list of strings; if omitted or empty, queries **all** projects)
- `format` (optional; `json` (default) or `csv`)
- `output_file` (optional; if provided, write results to this path instead of returning inline)

Output:
- `sessions: [{ session_id, project_id, time_in, time_out, notes, tags, duration_seconds }]` (when format is `json`)
- CSV text with columns `session_id, project_id, time_in, time_out, duration_seconds, notes, tags` (when format is `csv`)

#### `timeclock_session_add_note`
Append a timestamped note to an existing session. Works on both active and closed sessions.

Input:
- `session_id` (required)
- `text` (required)

Output:
- `session: { ... }` (updated session with new note appended)

Errors:
- if `session_id` does not exist
- if `text` is empty

#### `timeclock_session_correct`
Correct fields on an existing session (supports amending past entries).

Input:
- `session_id` (required)
- `time_in` (optional; RFC3339, UTC)
- `time_out` (optional; RFC3339, UTC)
- `note` (optional; **appended** as a new timestamped note entry)
- `tags` (optional; replaces tag list)

Output:
- `session: { ... }` (updated session)

Errors:
- if `session_id` does not exist
- if resulting `time_out` < `time_in`

#### `timeclock_session_delete`
Permanently delete a session by ID. Use `timeclock_session_correct` instead if you only need to amend fields.

Input:
- `session_id` (required)

Output:
- `{ deleted_session, project_id }`

Errors:
- if `session_id` does not exist

### Resources (optional)
Potential resources to expose later:
- `timeclock://sessions` (read-only listing)
- `timeclock://projects`

## Reporting
This server is a **data source**, not a billing system. The intent is to export session data into external tools (e.g., a spreadsheet) for invoicing or analysis.

- Use `timeclock.session.query` with `format: csv` and `output_file` to produce a file ready to load into Excel or similar.
- Billing calculations (rates, rounding, totals) are left to the consuming tool.

## Implementation notes (Rust)
- Rust edition: **2024**
- Follow the existing MCP services pattern in `~/projects/*-mcp`:
  - CLI with `serve` subcommand
  - transport mode: `stdio` (required), `websocket` (optional)
  - axum + tokio runtime
- Prefer strict linting:
  - `[lints.rust] warnings = "deny"`
  - `[lints.clippy] all = "deny"`

## Decisions
1. **Storage**: JSONL, one file per project (`~/.local/share/desktop-assistant/timeclock/{project_id}.jsonl`).
2. **Active sessions**: One active session per project (not a global lock).
3. **Corrections**: Supported via `timeclock.session.correct`; implemented as an appended replacement record in the JSONL file.
4. **Time zones**: Always UTC. No local rendering in the server; clients may format for display.
