#![deny(warnings)]

use crate::error::Result;
use crate::storage;
use serde_json::{Value, json};

/// Return all currently active sessions.
///
/// - `project_id`: if `Some`, restrict to that project; otherwise all projects.
pub fn run(project_id: Option<&str>) -> Result<Value> {
    let sessions = match project_id {
        Some(id) => {
            let active = storage::find_active_session(id)?;
            active.into_iter().collect::<Vec<_>>()
        }
        None => storage::read_all_sessions()?
            .into_iter()
            .filter(|s| s.time_out.is_none())
            .collect(),
    };
    let list: Vec<Value> = sessions.iter().map(|s| s.to_value()).collect();
    Ok(json!({ "sessions": list }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::clock_in;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_get_active_empty() {
        let _env = TestEnv::new();
        let result = run(None).unwrap();
        assert_eq!(result["sessions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_get_active_returns_open_sessions() {
        let _env = TestEnv::new();
        clock_in::run("acme", None, None, vec![]).unwrap();
        clock_in::run("beta", None, None, vec![]).unwrap();
        let result = run(None).unwrap();
        assert_eq!(result["sessions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_get_active_filtered_by_project() {
        let _env = TestEnv::new();
        clock_in::run("acme", None, None, vec![]).unwrap();
        clock_in::run("beta", None, None, vec![]).unwrap();
        let result = run(Some("acme")).unwrap();
        let sessions = result["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["project_id"], "acme");
    }
}
