#![deny(warnings)]

use chrono::DateTime;
use chrono::Utc;
use serde_json::{Value, json};

use crate::error::{Result, StorageError, ValidationError};
use crate::models::NoteEntry;
use crate::storage;

/// Correct fields on an existing session.
///
/// Amends one or more of `time_in`, `time_out`, `tags`. If `note` is provided
/// it is **appended** as a new timestamped note entry (use `timeclock.session.add_note`
/// for a dedicated note-only append).  Implemented by re-appending a replacement
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
        session.notes.push(NoteEntry {
            timestamp: Utc::now().to_rfc3339(),
            text: n.to_string(),
        });
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
        assert_eq!(result["session"]["notes"][0]["text"], "updated note");
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
        // Re-read and confirm both notes are present
        let sessions = storage::read_sessions("acme").unwrap();
        let s = sessions.iter().find(|s| s.session_id == sid).unwrap();
        assert_eq!(s.notes.len(), 2);
        assert_eq!(s.notes[0].text, "original");
        assert_eq!(s.notes[1].text, "corrected");
    }
}
