#![deny(warnings)]

use chrono::DateTime;
use serde_json::{Value, json};

use crate::error::{Result, StorageError, ValidationError};
use crate::storage;

/// Correct fields on an existing session.
///
/// Amends one or more of `time_in`, `time_out`, `note`, or `tags`.
/// Implemented by reading the existing session and appending a replacement
/// record with the same `session_id` (last-record-wins).
pub fn run(
    session_id: &str,
    time_in: Option<&str>,
    time_out: Option<&str>,
    note: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<Value> {
    if session_id.is_empty() {
        return Err(ValidationError::MissingField("session_id".to_string()).into());
    }

    let (_project_id, mut session) =
        storage::find_session_by_id(session_id)?.ok_or_else(|| {
            StorageError::SessionNotFound(session_id.to_string())
        })?;

    if let Some(t) = time_in {
        let dt: DateTime<chrono::Utc> = t.parse().map_err(|e: chrono::ParseError| {
            ValidationError::InvalidTimestamp(t.to_string(), e.to_string())
        })?;
        session.time_in = dt.to_rfc3339();
    }
    if let Some(t) = time_out {
        let dt: DateTime<chrono::Utc> = t.parse().map_err(|e: chrono::ParseError| {
            ValidationError::InvalidTimestamp(t.to_string(), e.to_string())
        })?;
        session.time_out = Some(dt.to_rfc3339());
    }
    if let Some(n) = note {
        session.note = Some(n.to_string());
    }
    if let Some(t) = tags {
        session.tags = t;
    }

    // Validate ordering after applying changes
    if let Some(ref t_out) = session.time_out {
        let t_in: DateTime<chrono::Utc> = session.time_in.parse().map_err(|e: chrono::ParseError| {
            ValidationError::InvalidTimestamp(session.time_in.clone(), e.to_string())
        })?;
        let t_out_dt: DateTime<chrono::Utc> = t_out.parse().map_err(|e: chrono::ParseError| {
            ValidationError::InvalidTimestamp(t_out.clone(), e.to_string())
        })?;
        if t_out_dt < t_in {
            return Err(ValidationError::TimeOutBeforeTimeIn.into());
        }
    }

    storage::append_session(&session)?;
    Ok(json!({ "session": session.to_value() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::clock_in;
    use crate::storage;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_correct_note() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", None, None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"].as_str().unwrap().to_string();
        let result = run(&sid, None, None, Some("updated note"), None).unwrap();
        assert_eq!(result["session"]["note"], "updated note");
    }

    #[test]
    fn test_correct_not_found() {
        let _env = TestEnv::new();
        assert!(run("nonexistent-id", None, None, None, None).is_err());
    }

    #[test]
    fn test_correct_invalid_ordering() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", Some("2026-02-19T15:00:00Z"), None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"].as_str().unwrap().to_string();
        // Set time_out before time_in => error
        let err = run(&sid, None, Some("2026-02-19T14:00:00Z"), None, None);
        assert!(err.is_err());
    }

    #[test]
    fn test_correct_last_record_wins() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", None, Some("original"), vec![]).unwrap();
        let sid = clocked["session"]["session_id"].as_str().unwrap().to_string();
        run(&sid, None, None, Some("corrected"), None).unwrap();
        // Re-read and confirm corrected note is returned
        let sessions = storage::read_sessions("acme").unwrap();
        let s = sessions.iter().find(|s| s.session_id == sid).unwrap();
        assert_eq!(s.note.as_deref(), Some("corrected"));
    }
}
