#![deny(warnings)]

use crate::error::{Result, ValidationError};
use crate::models::Project;
use crate::storage;
use serde_json::{Value, json};

/// Create or update a project.
///
/// - `project_id`: optional; if omitted, derived from `name` (lowercased, spaces → '_').
/// - `name`: required.
pub fn run(project_id: Option<&str>, name: &str) -> Result<Value> {
    if name.is_empty() {
        return Err(ValidationError::MissingField("name".to_string()).into());
    }
    let project_id = match project_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => derive_id(name),
    };
    let project = Project { project_id, name: name.to_string() };
    storage::upsert_project(&project)?;
    Ok(json!({ "project": Value::from(project) }))
}

fn derive_id(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_upsert_with_explicit_id() {
        let _env = TestEnv::new();
        let result = run(Some("acme"), "Acme Corp").unwrap();
        assert_eq!(result["project"]["project_id"], "acme");
        assert_eq!(result["project"]["name"], "Acme Corp");
    }

    #[test]
    fn test_upsert_derives_id() {
        let _env = TestEnv::new();
        let result = run(None, "My Project").unwrap();
        assert_eq!(result["project"]["project_id"], "my_project");
    }

    #[test]
    fn test_upsert_missing_name() {
        let _env = TestEnv::new();
        assert!(run(None, "").is_err());
    }
}
