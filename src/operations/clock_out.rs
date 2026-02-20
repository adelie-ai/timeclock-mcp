#![deny(warnings)]

use chrono::Utc;
use serde_json::{Value, json};

use crate::error::{Result, ValidationError};
use crate::models::NoteEntry;
use crate::storage;

/// End the active session for `project_id`.
///
/// - `time_out`: optional RFC3339 UTC; defaults to now.
/// - `note`: optional; replaces existing note if provided.
pub fn run(project_id: &str, time_out: Option<&str>, note: Option<&str>) -> Result<Value> {
    if project_id.is_empty() {
        return Err(ValidationError::MissingField("project_id".to_string()).into());
    }
    let mut session = storage::find_active_session(project_id)?.ok_or_else(|| {
        ValidationError::NotClockedIn(project_id.to_string())
    })?;

    let time_out_str = match time_out {
        Some(t) => parse_utc(t)?,
        None => Utc::now().to_rfc3339(),
    };

    // Validate ordering
    validate_ordering(&session.time_in, &time_out_str)?;

    session.time_out = Some(time_out_str);
    if let Some(n) = note {
        session.notes.push(NoteEntry {
            timestamp: Utc::now().to_rfc3339(),
            text: n.to_string(),
        });
    }

    storage::append_session(&session)?;
    Ok(json!({ "session": session.to_value() }))
}

fn parse_utc(s: &str) -> Result<String> {
    use chrono::DateTime;
    let dt: DateTime<Utc> = s.parse().map_err(|e: chrono::ParseError| {
        ValidationError::InvalidTimestamp(s.to_string(), e.to_string())
    })?;
    Ok(dt.to_rfc3339())
}

fn validate_ordering(time_in: &str, time_out: &str) -> Result<()> {
    use chrono::DateTime;
    let t_in: DateTime<Utc> = time_in
        .parse()
        .map_err(|e: chrono::ParseError| ValidationError::InvalidTimestamp(time_in.to_string(), e.to_string()))?;
    let t_out: DateTime<Utc> = time_out
        .parse()
        .map_err(|e: chrono::ParseError| ValidationError::InvalidTimestamp(time_out.to_string(), e.to_string()))?;
    if t_out < t_in {
        return Err(ValidationError::TimeOutBeforeTimeIn.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::clock_in;
    use crate::test_helpers::TestEnv;

    #[test]
    fn test_clock_out_basic() {
        let _env = TestEnv::new();
        clock_in::run("acme", None, None, vec![]).unwrap();
        let result = run("acme", None, None).unwrap();
        assert!(result["session"]["time_out"].is_string());
    }

    #[test]
    fn test_clock_out_not_clocked_in() {
        let _env = TestEnv::new();
        assert!(run("acme", None, None).is_err());
    }

    #[test]
    fn test_clock_out_time_before_time_in() {
        let _env = TestEnv::new();
        clock_in::run("acme", Some("2026-02-19T15:00:00Z"), None, vec![]).unwrap();
        assert!(run("acme", Some("2026-02-19T14:00:00Z"), None).is_err());
    }
}
