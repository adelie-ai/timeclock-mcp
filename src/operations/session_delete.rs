#![deny(warnings)]

use serde_json::{json, Value};

use crate::error::Result;
use crate::storage;

/// Permanently delete a session by `session_id`.
///
/// The session is located across all project files; its project's JSONL is
/// rewritten without the matching record.  This is a hard delete — use
/// `timeclock.session.correct` if you only want to amend fields.
pub fn run(session_id: &str) -> Result<Value> {
    // find_session_by_id returns the session so we can echo it back, but we
    // use delete_session_by_id (which also handles not-found) to do the work.
    let found = storage::find_session_by_id(session_id)?;
    let project_id = found.map(|(pid, _)| pid);

    storage::delete_session_by_id(session_id)?;

    Ok(json!({
        "deleted_session": session_id,
        "project_id": project_id
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Session;
    use crate::storage;
    use crate::test_helpers::TestEnv;

    fn make_session(id: &str, project: &str) -> Session {
        Session {
            session_id: id.to_string(),
            project_id: project.to_string(),
            time_in: "2025-03-01T08:00:00Z".to_string(),
            time_out: Some("2025-03-01T09:00:00Z".to_string()),
            note: None,
            tags: vec![],
        }
    }

    #[test]
    fn deletes_existing_session() {
        let _env = TestEnv::new();
        storage::append_session(&make_session("del-1", "project_x")).unwrap();
        storage::append_session(&make_session("del-2", "project_x")).unwrap();

        let result = run("del-1").unwrap();
        assert_eq!(result["deleted_session"], "del-1");
        assert_eq!(result["project_id"], "project_x");

        let sessions = storage::read_sessions("project_x").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "del-2");
    }

    #[test]
    fn errors_on_missing_session() {
        let _env = TestEnv::new();
        let err = run("no-such-id").unwrap_err();
        assert!(err.to_string().contains("no-such-id"));
    }
}
