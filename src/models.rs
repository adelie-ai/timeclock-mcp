#![deny(warnings)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A project label used to group time sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub name: String,
}

impl From<Project> for Value {
    fn from(p: Project) -> Self {
        json!({ "project_id": p.project_id, "name": p.name })
    }
}

/// A timestamped note attached to a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteEntry {
    /// RFC3339 UTC — records when the note was written.
    pub timestamp: String,
    pub text: String,
}

/// A contiguous work interval for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub project_id: String,
    /// RFC3339 UTC
    pub time_in: String,
    /// RFC3339 UTC; None while session is active
    pub time_out: Option<String>,
    #[serde(default)]
    pub notes: Vec<NoteEntry>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Session {
    /// Compute duration in whole seconds.
    ///
    /// - If the session is closed, uses (time_out - time_in).
    /// - If the session is active, uses (Utc::now() - time_in).
    ///
    /// Returns None only if time parsing fails.
    pub fn duration_seconds(&self) -> Option<i64> {
        let t_in: DateTime<Utc> = self.time_in.parse().ok()?;
        let t_out: DateTime<Utc> = match self.time_out.as_ref() {
            Some(to) => to.parse().ok()?,
            None => Utc::now(),
        };
        Some((t_out - t_in).num_seconds())
    }

    /// Serialize to a JSON Value including the derived `duration_seconds` field.
    pub fn to_value(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "project_id": self.project_id,
            "time_in": self.time_in,
            "time_out": self.time_out,
            "notes": self.notes.iter().map(|n| json!({ "timestamp": n.timestamp, "text": n.text })).collect::<Vec<_>>(),
            "tags": self.tags,
            "duration_seconds": self.duration_seconds(),
        })
    }

    /// Render as a CSV row (no header). Fields:
    /// session_id, project_id, time_in, time_out, duration_seconds, notes, tags
    pub fn to_csv_row(&self) -> String {
        let notes_text = self
            .notes
            .iter()
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let fields: Vec<String> = vec![
            csv_field(&self.session_id),
            csv_field(&self.project_id),
            csv_field(&self.time_in),
            csv_field(self.time_out.as_deref().unwrap_or("")),
            self.duration_seconds()
                .map(|d| d.to_string())
                .unwrap_or_default(),
            csv_field(&notes_text),
            csv_field(&self.tags.join(";")),
        ];
        fields.join(",")
    }
}

/// Quote a CSV field, escaping embedded double-quotes.
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub const CSV_HEADER: &str =
    "session_id,project_id,time_in,time_out,duration_seconds,notes,tags";
