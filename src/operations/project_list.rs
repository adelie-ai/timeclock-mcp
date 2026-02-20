#![deny(warnings)]

use crate::error::Result;
use crate::storage;
use serde_json::{Value, json};

/// List all known projects, sorted by project_id.
pub fn run() -> Result<Value> {
    let projects = storage::read_projects()?;
    let list: Vec<Value> = projects.into_iter().map(Value::from).collect();
    Ok(json!({ "projects": list }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Project;
    use crate::storage;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_list_empty() {
        let _env = TestEnv::new();
        let result = run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_returns_projects() {
        // Write a project into a temp data dir and verify list includes it.
        let _env = TestEnv::new();
        let p = Project {
            project_id: "acme".to_string(),
            name: "Acme Corp".to_string(),
        };
        storage::upsert_project(&p).unwrap();
        let result = run().unwrap();
        let projects = result["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["project_id"], "acme");
    }
}
