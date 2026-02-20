#![deny(warnings)]

use chrono::Utc;
use serde_json::{Value, json};

use crate::error::{Result, StorageError, ValidationError};
use crate::models::NoteEntry;
use crate::storage;

/// Append a timestamped note to an existing session.
///
/// Works on both active and closed sessions; the note is appended to the
/// session's `notes` list and the updated record is re-written via the
/// standard append / last-record-wins mechanism.
pub fn run(session_id: &str, text: &str) -> Result<Value> {
    if session_id.is_empty() {
        return Err(ValidationError::MissingField("session_id".to_string()).into());
    }
    if text.is_empty() {
        return Err(ValidationError::MissingField("text".to_string()).into());
    }

    let (_project_id, mut session) =
        storage::find_session_by_id(session_id)?.ok_or_else(|| {
            StorageError::SessionNotFound(session_id.to_string())
        })?;

    session.notes.push(NoteEntry {
        timestamp: Utc::now().to_rfc3339(),
        text: text.to_string(),
    });

    storage::append_session(&session)?;
    Ok(json!({ "session": session.to_value() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::clock_in;
    use crate::test_helpers::TestEnv;

    #[test]
    fn adds_note_to_active_session() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("proj", None, None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"].as_str().unwrap().to_string();

        run(&sid, "first note").unwrap();
        run(&sid, "second note").unwrap();

        let sessions = storage::read_sessions("proj").unwrap();
        let s = sessions.iter().find(|s| s.session_id == sid).unwrap();
        assert_eq!(s.notes.len(), 2);
        assert_eq!(s.notes[0].text, "first note");
        assert_eq!(s.notes[1].text, "second note");
    }

    #[test]
    fn errors_on_missing_session() {
        let _env = TestEnv::new();
        assert!(run("no-such-id", "hello").is_err());
    }

    #[test]
    fn errors_on_empty_text() {
        let _env = TestEnv::new();
        let clocked = clock_in::run("proj2", None, None, vec![]).unwrap();
        let sid = clocked["session"]["session_id"].as_str().unwrap().to_string();
        assert!(run(&sid, "").is_err());
    }
}
