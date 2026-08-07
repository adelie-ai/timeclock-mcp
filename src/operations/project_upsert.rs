#![deny(warnings)]

use crate::error::{Result, ValidationError};
use crate::models::Project;
use crate::storage;
use serde_json::{Value, json};

/// Create or update a project.
///
/// - `project_id`: optional; if omitted, derived from `name` (lowercased, spaces → '_').
/// - `name`: required.
///
/// `skip_all`: `project_id` and `name` are caller-supplied content (a
/// client or project name), never a span field (mcp-core#40 D10).
#[tracing::instrument(name = "timeclock.project_upsert", skip_all)]
pub fn run(project_id: Option<&str>, name: &str) -> Result<Value> {
    if name.is_empty() {
        return Err(ValidationError::MissingField("name".to_string()).into());
    }
    let project_id = match project_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => derive_id(name),
    };
    let project = Project {
        project_id,
        name: name.to_string(),
    };
    storage::upsert_project(&project)?;
    Ok(json!({ "project": Value::from(project) }))
}

fn derive_id(name: &str) -> String {
    // Use is_ascii_alphanumeric() to match validate_project_id's ASCII-only constraint.
    // Non-ASCII characters (accented letters, CJK, etc.) are replaced with '_' so the
    // derived id always passes validation without an opaque error.
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
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

    #[test]
    fn test_upsert_non_ascii_name_derives_valid_id() {
        // "Société" contains 'é' (non-ASCII). derive_id must produce a valid ASCII id
        // that passes validate_project_id (is_ascii_alphanumeric + '-' + '_' only).
        let _env = TestEnv::new();
        let result = run(None, "Société").unwrap();
        let id = result["project"]["project_id"].as_str().unwrap();
        // Must be non-empty and composed only of ASCII alphanumerics, '-', '_'.
        assert!(!id.is_empty());
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "derived id {id:?} contains non-ASCII or invalid characters"
        );
    }

    #[test]
    fn test_derive_id_ascii_only() {
        // Verify the mapping directly without storage.
        // "Café & Co." → "caf__co_" (é→'_', space→'_', '&'→'_', '.'→'_')
        // We just assert the output is all-ASCII.
        let id = super::derive_id("Café & Co.");
        assert!(
            id.is_ascii(),
            "derive_id produced non-ASCII character in {id:?}"
        );
    }
}
