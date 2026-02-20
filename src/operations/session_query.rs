#![deny(warnings)]

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fs;

use crate::error::{Result, ValidationError};
use crate::models::{Session, CSV_HEADER};
use crate::storage;

/// Query sessions by time window, optionally filtered to a list of projects.
///
/// - `start` / `end`: RFC3339 UTC, inclusive range.
/// - `project_ids`: if empty or None, queries all projects.
/// - `format`: `"json"` (default) or `"csv"`.
/// - `output_file`: if provided, write results to disk and return a summary message.
pub fn run(
    start: &str,
    end: &str,
    project_ids: &[String],
    format: &str,
    output_file: Option<&str>,
) -> Result<Value> {
    let t_start: DateTime<Utc> = start
        .parse()
        .map_err(|e: chrono::ParseError| ValidationError::InvalidTimestamp(start.to_string(), e.to_string()))?;
    let t_end: DateTime<Utc> = end
        .parse()
        .map_err(|e: chrono::ParseError| ValidationError::InvalidTimestamp(end.to_string(), e.to_string()))?;

    // Gather sessions
    let all_sessions: Vec<Session> = if project_ids.is_empty() {
        storage::read_all_sessions()?
    } else {
        let mut v = Vec::new();
        for pid in project_ids {
            v.extend(storage::read_sessions(pid)?);
        }
        v.sort_by(|a, b| a.time_in.cmp(&b.time_in));
        v
    };

    // Filter to the time window (session starts within [start, end])
    let filtered: Vec<&Session> = all_sessions
        .iter()
        .filter(|s| {
            if let Ok(t_in) = s.time_in.parse::<DateTime<Utc>>() {
                t_in >= t_start && t_in <= t_end
            } else {
                false
            }
        })
        .collect();

    match format {
        "csv" => render_csv(&filtered, output_file),
        _ => render_json(&filtered, output_file),
    }
}

fn render_json(sessions: &[&Session], output_file: Option<&str>) -> Result<Value> {
    let list: Vec<Value> = sessions.iter().map(|s| s.to_value()).collect();
    if let Some(path) = output_file {
        let text = serde_json::to_string_pretty(&json!({ "sessions": &list }))?;
        write_file(path, &text)?;
        return Ok(json!({
            "message": format!("Written {} session(s) to {}", sessions.len(), path),
            "count": sessions.len(),
        }));
    }
    Ok(json!({ "sessions": list }))
}

fn render_csv(sessions: &[&Session], output_file: Option<&str>) -> Result<Value> {
    let mut lines = vec![CSV_HEADER.to_string()];
    for s in sessions {
        lines.push(s.to_csv_row());
    }
    let text = lines.join("\n") + "\n";

    if let Some(path) = output_file {
        write_file(path, &text)?;
        return Ok(json!({
            "message": format!("Written {} session(s) to {}", sessions.len(), path),
            "count": sessions.len(),
        }));
    }
    Ok(json!({ "csv": text }))
}

fn write_file(path: &str, content: &str) -> Result<()> {
    let expanded = shellexpand::full(path)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string());
    fs::write(&expanded, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Session;
    use crate::storage;
    use crate::test_helpers::TestEnv;

    fn make_closed_session(project_id: &str, time_in: &str, time_out: &str) -> Session {
        Session {
            session_id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            time_in: time_in.to_string(),
            time_out: Some(time_out.to_string()),
            notes: vec![],
            tags: vec![],
        }
    }

    #[test]
    fn test_query_returns_matching_sessions() {
        let _env = TestEnv::new();
        let s = make_closed_session("acme", "2026-02-19T10:00:00Z", "2026-02-19T11:00:00Z");
        storage::append_session(&s).unwrap();
        let result = run(
            "2026-02-19T00:00:00Z",
            "2026-02-19T23:59:59Z",
            &[],
            "json",
            None,
        )
        .unwrap();
        assert_eq!(result["sessions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_query_outside_window_excluded() {
        let _env = TestEnv::new();
        let s = make_closed_session("acme", "2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z");
        storage::append_session(&s).unwrap();
        let result = run(
            "2026-02-19T00:00:00Z",
            "2026-02-19T23:59:59Z",
            &[],
            "json",
            None,
        )
        .unwrap();
        assert_eq!(result["sessions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_query_csv_format() {
        let _env = TestEnv::new();
        let s = make_closed_session("acme", "2026-02-19T10:00:00Z", "2026-02-19T11:00:00Z");
        storage::append_session(&s).unwrap();
        let result = run(
            "2026-02-19T00:00:00Z",
            "2026-02-19T23:59:59Z",
            &[],
            "csv",
            None,
        )
        .unwrap();
        let csv = result["csv"].as_str().unwrap();
        assert!(csv.starts_with("session_id,"));
        assert!(csv.contains("acme"));
    }

    #[test]
    fn test_query_project_filter() {
        let _env = TestEnv::new();
        let s1 = make_closed_session("acme", "2026-02-19T10:00:00Z", "2026-02-19T11:00:00Z");
        let s2 = make_closed_session("beta", "2026-02-19T10:00:00Z", "2026-02-19T11:00:00Z");
        storage::append_session(&s1).unwrap();
        storage::append_session(&s2).unwrap();
        let result = run(
            "2026-02-19T00:00:00Z",
            "2026-02-19T23:59:59Z",
            &["acme".to_string()],
            "json",
            None,
        )
        .unwrap();
        let arr = result["sessions"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["project_id"], "acme");
    }
}
