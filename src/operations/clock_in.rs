#![deny(warnings)]

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::{Result, ValidationError};
use crate::models::{NoteEntry, Project, Session};
use crate::storage;

/// Start a new session for `project_id`.
///
/// - `time_in`: optional RFC3339 UTC string; defaults to now.
/// - `note`: optional.
/// - `tags`: optional.
///
/// Auto-registers the project if it is not yet known.
/// Errors if there is already an active session for the project.
pub fn run(
    project_id: &str,
    time_in: Option<&str>,
    note: Option<&str>,
    tags: Vec<String>,
) -> Result<Value> {
    if project_id.is_empty() {
        return Err(ValidationError::MissingField("project_id".to_string()).into());
    }
    // Parse / default time_in
    let time_in_str = match time_in {
        Some(t) => parse_utc(t)?,
        None => Utc::now().to_rfc3339(),
    };

    // Guard: no existing active session for this project
    if let Some(active) = storage::find_active_session(project_id)? {
        return Err(ValidationError::AlreadyClockedIn(format!(
            "{} (session {})",
            project_id, active.session_id
        ))
        .into());
    }

    // Auto-register project if unknown
    if !storage::project_exists(project_id)? {
        storage::upsert_project(&Project {
            project_id: project_id.to_string(),
            name: project_id.to_string(),
        })?;
    }

    let notes = note
        .map(|text| vec![NoteEntry { timestamp: Utc::now().to_rfc3339(), text: text.to_string() }])
        .unwrap_or_default();

    let session = Session {
        session_id: Uuid::new_v4().to_string(),
        project_id: project_id.to_string(),
        time_in: time_in_str,
        time_out: None,
        notes,
        tags,
    };
    storage::append_session(&session)?;
    Ok(json!({ "session": session.to_value() }))
}

/// Parse an RFC3339 string and re-format it as UTC RFC3339.
fn parse_utc(s: &str) -> Result<String> {
    use chrono::DateTime;
    let dt: DateTime<Utc> = s.parse().map_err(|e: chrono::ParseError| {
        ValidationError::InvalidTimestamp(s.to_string(), e.to_string())
    })?;
    Ok(dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_clock_in_basic() {
        let _env = TestEnv::new();
        let result = run("acme", None, None, vec![]).unwrap();
        assert!(result["session"]["session_id"].is_string());
        assert_eq!(result["session"]["project_id"], "acme");
        assert!(result["session"]["time_out"].is_null());
    }

    #[test]
    fn test_clock_in_duplicate_errors() {
        let _env = TestEnv::new();
        run("acme", None, None, vec![]).unwrap();
        let second = run("acme", None, None, vec![]);
        assert!(second.is_err());
    }

    #[test]
    fn test_clock_in_different_projects_ok() {
        let _env = TestEnv::new();
        run("proj_a", None, None, vec![]).unwrap();
        run("proj_b", None, None, vec![]).unwrap(); // should succeed
    }
}
