pub mod clock_in;
pub mod clock_out;
pub mod project_delete;
pub mod project_list;
pub mod project_upsert;
pub mod session_add_note;
pub mod session_correct;
pub mod session_delete;
pub mod session_get_active;
pub mod session_query;

/// Parse an RFC3339 string and re-format it as canonical UTC RFC3339.
///
/// Shared by `clock_in` and `clock_out` to avoid duplication.
pub(crate) fn parse_utc(s: &str) -> crate::error::Result<String> {
    use crate::error::ValidationError;
    use chrono::{DateTime, Utc};
    let dt: DateTime<Utc> = s.parse().map_err(|e: chrono::ParseError| {
        ValidationError::InvalidTimestamp(s.to_string(), e.to_string())
    })?;
    Ok(dt.to_rfc3339())
}
