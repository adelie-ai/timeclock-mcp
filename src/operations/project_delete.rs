#![deny(warnings)]

use serde_json::{json, Value};

use crate::error::{Result, ValidationError};
use crate::storage;

/// Delete a project from the registry.
///
/// If `delete_entries` is `false` (the default / safe behaviour) and the
/// project has any recorded sessions the operation is refused — you must
/// explicitly pass `delete_entries = true` to also wipe the session data.
pub fn run(project_id: &str, delete_entries: bool) -> Result<Value> {
    let sessions = storage::read_sessions(project_id)?;

    if !delete_entries && !sessions.is_empty() {
        return Err(
            ValidationError::ProjectHasEntries(project_id.to_string(), sessions.len()).into(),
        );
    }

    if delete_entries {
        storage::delete_project_sessions(project_id)?;
    }

    storage::delete_project(project_id)?;

    Ok(json!({
        "deleted_project": project_id,
        "sessions_deleted": delete_entries && !sessions.is_empty(),
        "session_count": sessions.len()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Session;
    use crate::storage;
    use crate::test_helpers::TestEnv;

    #[test]
    fn deletes_empty_project() {
        let _env = TestEnv::new();
        storage::upsert_project(&crate::models::Project {
            project_id: "proj_a".to_string(),
            name: "Project A".to_string(),
        })
        .unwrap();
        let result = run("proj_a", false).unwrap();
        assert_eq!(result["deleted_project"], "proj_a");
        let projects = storage::read_projects().unwrap();
        assert!(projects.iter().all(|p| p.project_id != "proj_a"));
    }

    #[test]
    fn refuses_deletion_when_entries_exist() {
        let _env = TestEnv::new();
        storage::upsert_project(&crate::models::Project {
            project_id: "proj_b".to_string(),
            name: "Project B".to_string(),
        })
        .unwrap();
        let session = Session {
            session_id: "s1".to_string(),
            project_id: "proj_b".to_string(),
            time_in: "2025-01-01T09:00:00Z".to_string(),
            time_out: None,
            notes: vec![],
            tags: vec![],
        };
        storage::append_session(&session).unwrap();

        let err = run("proj_b", false).unwrap_err();
        assert!(err.to_string().contains("delete_entries=true"));
    }

    #[test]
    fn deletes_project_and_entries_when_flag_set() {
        let _env = TestEnv::new();
        storage::upsert_project(&crate::models::Project {
            project_id: "proj_c".to_string(),
            name: "Project C".to_string(),
        })
        .unwrap();
        let session = Session {
            session_id: "s2".to_string(),
            project_id: "proj_c".to_string(),
            time_in: "2025-01-01T09:00:00Z".to_string(),
            time_out: None,
            notes: vec![],
            tags: vec![],
        };
        storage::append_session(&session).unwrap();

        let result = run("proj_c", true).unwrap();
        assert_eq!(result["sessions_deleted"], true);

        let projects = storage::read_projects().unwrap();
        assert!(projects.iter().all(|p| p.project_id != "proj_c"));

        let sessions = storage::read_sessions("proj_c").unwrap();
        assert!(sessions.is_empty());
    }
}
