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
                "Query sessions within a time window across one, many, or all projects.",
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
