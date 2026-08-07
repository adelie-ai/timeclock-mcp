//! [`McpService`] implementation — bridges mcp-core dispatch to the timeclock
//! domain operations (storage + operations/*).

use mcp_core::{CallError, McpService, ToolDef, ToolReply, async_trait};
use serde_json::{Value, json};

use crate::error::McpError;
use crate::operations::{
    clock_in, clock_out, project_delete, project_list, project_upsert, session_add_note,
    session_correct, session_delete, session_get_active, session_query,
};

/// The stateless timeclock service. All state lives on disk (JSONL storage).
pub struct TimeclockService;

#[async_trait]
impl McpService for TimeclockService {
    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new(
                "timeclock_project_list",
                "List all known projects.",
                json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
            ToolDef::new(
                "timeclock_project_delete",
                "Delete a project from the registry. Refuses by default if any sessions exist \
                 (molly guard); set delete_entries=true to also remove all session data.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "ID of the project to delete."
                        },
                        "delete_entries": {
                            "type": "boolean",
                            "description": "If true, also delete all session data for this project. Default: false."
                        }
                    },
                    "required": ["project_id"]
                }),
            ),
            ToolDef::new(
                "timeclock_project_upsert",
                "Create or update a project. If project_id is omitted it is derived from name.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "Stable identifier for the project. Optional; derived from name if omitted."
                        },
                        "name": {
                            "type": "string",
                            "description": "Human-readable project name."
                        }
                    },
                    "required": ["name"]
                }),
            ),
            ToolDef::new(
                "timeclock_clock_in",
                "Start a new time session for a project. Errors if the project already has an active session.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "Project to clock in to."
                        },
                        "time_in": {
                            "type": "string",
                            "description": "RFC3339 UTC start time. Defaults to now."
                        },
                        "note": {
                            "type": "string",
                            "description": "Optional initial note for the session."
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional list of tags."
                        }
                    },
                    "required": ["project_id"]
                }),
            ),
            ToolDef::new(
                "timeclock_clock_out",
                "End the active session for a project. Errors if no active session exists.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "Project to clock out of."
                        },
                        "time_out": {
                            "type": "string",
                            "description": "RFC3339 UTC end time. Defaults to now."
                        },
                        "note": {
                            "type": "string",
                            "description": "Optional closing note; appended to the session's note list."
                        }
                    },
                    "required": ["project_id"]
                }),
            ),
            ToolDef::new(
                "timeclock_session_get_active",
                "Return all currently active sessions, optionally filtered to one project.",
                json!({
                    "type": "object",
                    "properties": {
                        "project_id": {
                            "type": "string",
                            "description": "If provided, return only the active session for this project."
                        }
                    }
                }),
            ),
            ToolDef::new(
                "timeclock_session_query",
                "Report on tracked time: list work sessions with their durations over a date range, \
                 for one, many, or all projects - the tool for timesheets, hours summaries, and \
                 \"how much time did I spend\" questions. Returns JSON or CSV and can optionally \
                 write the results to a file.",
                json!({
                    "type": "object",
                    "properties": {
                        "start": {
                            "type": "string",
                            "description": "RFC3339 UTC window start (inclusive)."
                        },
                        "end": {
                            "type": "string",
                            "description": "RFC3339 UTC window end (inclusive)."
                        },
                        "project_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Projects to include. If omitted or empty, all projects are queried."
                        },
                        "format": {
                            "type": "string",
                            "enum": ["json", "csv"],
                            "description": "Output format. Default: json."
                        },
                        "output_file": {
                            "type": "string",
                            "description": "If provided, write results to this file path instead of returning inline."
                        }
                    },
                    "required": ["start", "end"]
                }),
            ),
            ToolDef::new(
                "timeclock_session_add_note",
                "Append a timestamped note to a session. Works on both active and closed sessions. \
                 Use this to add comments at any time.",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "The session to annotate."
                        },
                        "text": {
                            "type": "string",
                            "description": "Note text to append."
                        }
                    },
                    "required": ["session_id", "text"]
                }),
            ),
            ToolDef::new(
                "timeclock_session_delete",
                "Permanently delete a session by session_id. Use timeclock_session_correct instead \
                 if you only want to amend fields.",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "The session to delete."
                        }
                    },
                    "required": ["session_id"]
                }),
            ),
            ToolDef::new(
                "timeclock_session_correct",
                "Correct fields on an existing session. Amends the record by appending a \
                 replacement (last-record-wins). To append a note use timeclock_session_add_note.",
                json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "The session to correct."
                        },
                        "time_in": {
                            "type": "string",
                            "description": "New RFC3339 UTC start time."
                        },
                        "time_out": {
                            "type": "string",
                            "description": "New RFC3339 UTC end time."
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Replacement tag list."
                        }
                    },
                    "required": ["session_id"]
                }),
            ),
        ]
    }

    async fn call_tool(&self, name: &str, args: &Value) -> Result<ToolReply, CallError> {
        let result = dispatch(name, args);
        match result {
            Ok(v) => Ok(ToolReply::json(&v)?),
            Err(e) => {
                // Distinguish protocol-level invalid-params from domain errors.
                use crate::error::TimeclockError;
                match &e {
                    TimeclockError::Mcp(McpError::InvalidToolParameters(_)) => {
                        Err(CallError::invalid_params(e.to_string()))
                    }
                    TimeclockError::Mcp(McpError::ToolNotFound(_)) => {
                        Err(CallError::tool(e.to_string()))
                    }
                    _ => Err(CallError::tool(e.to_string())),
                }
            }
        }
    }
}

/// Pure synchronous dispatch to domain operations.
fn dispatch(name: &str, args: &Value) -> crate::error::Result<Value> {
    match name {
        "timeclock_project_list" => project_list::run(),
        "timeclock_project_upsert" => {
            let project_id = args.get("project_id").and_then(|v| v.as_str());
            let name_str = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidToolParameters("name is required".to_string()))?;
            project_upsert::run(project_id, name_str)
        }
        "timeclock_clock_in" => {
            let project_id = args
                .get("project_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("project_id is required".to_string())
                })?;
            let time_in = args.get("time_in").and_then(|v| v.as_str());
            let note = args.get("note").and_then(|v| v.as_str());
            let tags: Vec<String> = args
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            clock_in::run(project_id, time_in, note, tags)
        }
        "timeclock_clock_out" => {
            let project_id = args
                .get("project_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("project_id is required".to_string())
                })?;
            let time_out = args.get("time_out").and_then(|v| v.as_str());
            let note = args.get("note").and_then(|v| v.as_str());
            clock_out::run(project_id, time_out, note)
        }
        "timeclock_session_get_active" => {
            let project_id = args.get("project_id").and_then(|v| v.as_str());
            session_get_active::run(project_id)
        }
        "timeclock_session_query" => {
            let start = args
                .get("start")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidToolParameters("start is required".to_string()))?;
            let end = args
                .get("end")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidToolParameters("end is required".to_string()))?;
            let project_ids: Vec<String> = args
                .get("project_ids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let format = args
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("json");
            let output_file = args
                .get("output_file")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            session_query::run(start, end, &project_ids, format, output_file)
        }
        "timeclock_session_add_note" => {
            let session_id = args
                .get("session_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("session_id is required".to_string())
                })?;
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidToolParameters("text is required".to_string()))?;
            session_add_note::run(session_id, text)
        }
        "timeclock_session_correct" => {
            let session_id = args
                .get("session_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("session_id is required".to_string())
                })?;
            let time_in = args.get("time_in").and_then(|v| v.as_str());
            let time_out = args.get("time_out").and_then(|v| v.as_str());
            let tags: Option<Vec<String>> =
                args.get("tags").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                });
            session_correct::run(session_id, time_in, time_out, tags)
        }
        "timeclock_project_delete" => {
            let project_id = args
                .get("project_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("project_id is required".to_string())
                })?;
            let delete_entries = args
                .get("delete_entries")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            project_delete::run(project_id, delete_entries)
        }
        "timeclock_session_delete" => {
            let session_id = args
                .get("session_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    McpError::InvalidToolParameters("session_id is required".to_string())
                })?;
            session_delete::run(session_id)
        }
        _ => Err(McpError::ToolNotFound(name.to_string()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestEnv, capture_async};
    use tracing::Level;

    /// The reporting tool must surface the natural terms a user or model would
    /// reach for when asking about hours worked (timesheets, reports, CSV export)
    /// so tool-search can find it, rather than leading with mechanism.
    #[test]
    fn session_query_description_leads_with_reporting_terms() {
        let tools = TimeclockService.tools();
        let query = tools
            .iter()
            .find(|t| t.name == "timeclock_session_query")
            .expect("timeclock_session_query tool must be registered");
        let desc = query.description.to_lowercase();
        for term in ["timesheet", "report", "csv"] {
            assert!(
                desc.contains(term),
                "session_query description should mention '{term}'; got: {}",
                query.description
            );
        }
    }

    /// The value planted in every content-shaped argument below: a project
    /// id, a project name, a session note, a note-annotation text, a tag,
    /// or a session id. This server records billable time against clients
    /// and projects, so every one of those is content under the level
    /// contract (mcp-core#40 D10) -- it must never reach a span field or an
    /// INFO-or-louder event, at any tool handler.
    const SENTINEL: &str = "MARKER-timeclock-sentinel-7d21c8";

    /// The value planted in `timeclock_session_query`'s `output_file`: a
    /// filesystem path, the other leak-prone shape this server handles
    /// (`operations/session_query.rs` writes report content to it via
    /// `shellexpand` + `fs::write`).
    const SENTINEL_PATH_NAME: &str = "MARKER-timeclock-sentinel-path-4e9a1b.json";

    /// One entry per registered tool: its name, the span its handler opens
    /// (`main.rs`'s `#[tracing::instrument(name = "timeclock.<op>", ...)]`
    /// on the matching `operations/*.rs::run`), and an argument set with
    /// [`SENTINEL`] (and, for `timeclock_session_query`, a sentinel path)
    /// planted in every string-shaped field the tool accepts.
    ///
    /// `timeclock_project_list` takes no arguments, so it has nothing to
    /// plant a sentinel in; it stays in the table anyway so the coverage
    /// check below still requires it to be listed.
    fn leak_test_cases() -> Vec<(&'static str, &'static str, Value)> {
        let output_file = crate::storage::data_dir()
            .join(SENTINEL_PATH_NAME)
            .display()
            .to_string();
        vec![
            (
                "timeclock_project_list",
                "timeclock.project_list",
                json!({}),
            ),
            (
                "timeclock_project_upsert",
                "timeclock.project_upsert",
                json!({ "project_id": SENTINEL, "name": SENTINEL }),
            ),
            (
                "timeclock_project_delete",
                "timeclock.project_delete",
                json!({ "project_id": SENTINEL, "delete_entries": false }),
            ),
            (
                "timeclock_clock_in",
                "timeclock.clock_in",
                json!({ "project_id": SENTINEL, "note": SENTINEL, "tags": [SENTINEL] }),
            ),
            (
                "timeclock_clock_out",
                "timeclock.clock_out",
                json!({ "project_id": SENTINEL, "note": SENTINEL }),
            ),
            (
                "timeclock_session_get_active",
                "timeclock.session_get_active",
                json!({ "project_id": SENTINEL }),
            ),
            (
                "timeclock_session_query",
                "timeclock.session_query",
                json!({
                    "start": "2026-01-01T00:00:00Z",
                    "end": "2026-01-02T00:00:00Z",
                    "project_ids": [SENTINEL],
                    "format": "json",
                    "output_file": output_file,
                }),
            ),
            (
                "timeclock_session_add_note",
                "timeclock.session_add_note",
                json!({ "session_id": SENTINEL, "text": SENTINEL }),
            ),
            (
                "timeclock_session_delete",
                "timeclock.session_delete",
                json!({ "session_id": SENTINEL }),
            ),
            (
                "timeclock_session_correct",
                "timeclock.session_correct",
                json!({ "session_id": SENTINEL, "tags": [SENTINEL] }),
            ),
        ]
    }

    /// AC (mcp-core#40 D10, epic AC7, lesson 8): no tool handler ever puts
    /// [`SENTINEL`] (or the sentinel path) into a span field or an
    /// INFO-or-louder event.
    ///
    /// Table-driven over the whole tool list, not one tool: mcp-core#40
    /// lesson 8 records that a single-tool version of this test caught a
    /// dropped `skip_all` on the one operation it exercised and missed it
    /// on two others. The coverage check below makes that failure mode
    /// itself a test failure -- add a tool without adding its row here, and
    /// this test fails on the mismatch before it ever gets to leak-checking.
    ///
    /// The same run proves the positive half per tool too: every handler
    /// must open its listed span, so the test cannot pass by silently
    /// exercising fewer handlers than the table claims.
    #[test]
    fn tool_call_records_no_sentinel_content() {
        let _env = TestEnv::new();
        let cases = leak_test_cases();

        let tools = TimeclockService.tools();
        let registered: std::collections::BTreeSet<&str> =
            tools.iter().map(|t| t.name.as_str()).collect();
        let tested: std::collections::BTreeSet<&str> =
            cases.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(
            registered, tested,
            "this test's table must cover exactly the registered tool set (mcp-core#40 \
             lesson 8): a tool present in one set but not the other is untested or stale"
        );

        let recorded = capture_async(|| async {
            let svc = TimeclockService;
            for (name, _, args) in &cases {
                let _ = svc.call_tool(name, args).await;
            }
        });

        for (name, expected_span, _) in &cases {
            // The span-existence check is independent of whether a tool has
            // a sentinel-bearing argument to leak (timeclock_project_list
            // does not): it is the positive control proving every handler
            // was actually invoked, not proving any one leak absent.
            assert!(
                recorded.spans.iter().any(|s| s.name == *expected_span),
                "{name} must open a {expected_span} span; spans were {:?}",
                recorded.span_summary()
            );
        }

        for span in &recorded.spans {
            for (key, value) in &span.fields {
                assert!(
                    !value.contains(SENTINEL) && !value.contains(SENTINEL_PATH_NAME),
                    "a sentinel leaked into span {:?} field {key:?}: {value:?}; \
                     all spans were {:?}",
                    span.name,
                    recorded.span_summary()
                );
            }
        }

        for event in &recorded.events {
            // DEBUG/TRACE may legitimately carry tool arguments (D10) --
            // that is mcp-core's own dispatch layer, inherited rather than
            // added here. Only INFO and louder are checked.
            if event.level > Level::INFO {
                continue;
            }
            for (key, value) in &event.fields {
                assert!(
                    !value.contains(SENTINEL) && !value.contains(SENTINEL_PATH_NAME),
                    "a sentinel leaked into an INFO-or-louder event field {key:?}: \
                     {value:?}; all events were {:?}",
                    recorded.event_summary()
                );
            }
        }
    }
}
