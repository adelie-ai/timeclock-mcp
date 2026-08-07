#![deny(warnings)]

use chrono::DateTime;
use serde_json::{Value, json};

use crate::error::{Result, StorageError, ValidationError};
use crate::storage;

/// Correct fields on an existing session.
///
/// Amends one or more of `time_in`, `time_out`, `tags`.  Implemented by
/// re-appending a replacement record with the same `session_id` (last-record-wins).
///
/// To append a note, use `timeclock_session_add_note` instead.
///
/// `skip_all`: `session_id` and `tags` are caller-supplied content, never a
/// span field (mcp-core#40 D10).
#[tracing::instrument(name = "timeclock.session_correct", skip_all)]
pub fn run(
    session_id: &str,
    time_in: Option<&str>,
    time_out: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<Value> {
    if session_id.is_empty() {
        return Err(ValidationError::MissingField("session_id".to_string()).into());
    }

    let (_project_id, mut session) = storage::find_session_by_id(session_id)?
        .ok_or_else(|| StorageError::SessionNotFound(session_id.to_string()))?;

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
    if let Some(t) = tags {
        session.tags = t;
    }

    // Validate ordering after applying changes
    if let Some(ref t_out) = session.time_out {
        let t_in: DateTime<chrono::Utc> =
            session.time_in.parse().map_err(|e: chrono::ParseError| {
                ValidationError::InvalidTimestamp(session.time_in.clone(), e.to_string())
            })?;
        let t_out_dt: DateTime<chrono::Utc> = t_out.parse().map_err(|e: chrono::ParseError| {
            ValidationError::InvalidTimestamp(t_out.clone(), e.to_string())
        })?;
        if t_out_dt < t_in {
            return Err(ValidationError::TimeOutBeforeTimeIn.into());
        }
    }

    // Guard: if the corrected session is active (no time_out), ensure no *other* active
    // session already exists for the same project.  Allowing two open sessions for the
    // same project would make clock_out and session_get_active non-deterministic.
    if session.time_out.is_none() {
        let existing = storage::read_sessions(&session.project_id)?;
        let other_active = existing
            .iter()
            .any(|s| s.session_id != session.session_id && s.time_out.is_none());
        if other_active {
            return Err(ValidationError::AlreadyClockedIn(format!(
                "{} (another active session exists; clock out first)",
                session.project_id
            ))
            .into());
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
    fn test_correct_time_in() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", None, None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let result = run(&sid, Some("2026-02-19T08:00:00Z"), None, None).unwrap();
        assert_eq!(result["session"]["time_in"], "2026-02-19T08:00:00+00:00");
    }

    #[test]
    fn test_correct_not_found() {
        let _env = TestEnv::new();
        assert!(run("nonexistent-id", None, None, None).is_err());
    }

    #[test]
    fn test_correct_invalid_ordering() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", Some("2026-02-19T15:00:00Z"), None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Set time_out before time_in => error
        let err = run(&sid, None, Some("2026-02-19T14:00:00Z"), None);
        assert!(err.is_err());
    }

    #[test]
    fn test_correct_tags() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("acme", None, None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        run(
            &sid,
            None,
            None,
            Some(vec!["rust".to_string(), "review".to_string()]),
        )
        .unwrap();
        let sessions = storage::read_sessions("acme").unwrap();
        let s = sessions.iter().find(|s| s.session_id == sid).unwrap();
        assert_eq!(s.tags, vec!["rust", "review"]);
    }

    /// Reopening a closed session (clearing time_out) must be rejected when another
    /// active session already exists for the same project.
    #[test]
    fn test_correct_reopen_rejected_when_active_exists() {
        use crate::models::Session;
        use uuid::Uuid;
        let _env = TestEnv::new();

        // Create a closed session directly in storage (bypasses clock_in's active guard).
        let closed = Session {
            session_id: Uuid::new_v4().to_string(),
            project_id: "acme".to_string(),
            time_in: "2026-02-19T08:00:00Z".to_string(),
            time_out: Some("2026-02-19T09:00:00Z".to_string()),
            notes: vec![],
            tags: vec![],
        };
        storage::append_session(&closed).unwrap();
        let closed_sid = closed.session_id.clone();

        // Now clock in normally, creating an active session.
        clock_in::run("acme", None, None, vec![]).unwrap();

        // Attempt to reopen the closed session by clearing its time_out via session_correct.
        // This would create a second active session — must be rejected.
        // We hack this by passing a future time_out... no, we need to clear time_out.
        // session_correct doesn't expose a "clear time_out" parameter, so the double-active
        // path is reached if we directly manipulate storage. Use a raw append to simulate
        // the scenario and verify our guard fires.
        let mut reopened = closed.clone();
        reopened.time_out = None; // re-open it manually in storage
        storage::append_session(&reopened).unwrap();

        // Now there are two active sessions for "acme". Calling session_correct on the
        // re-opened session (which is currently active) while another active exists should
        // be rejected.
        let result = run(&closed_sid, Some("2026-02-19T08:30:00Z"), None, None);
        assert!(
            result.is_err(),
            "session_correct must reject when a second active session would result"
        );
    }
}
