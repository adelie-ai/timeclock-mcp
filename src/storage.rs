#![deny(warnings)]

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::error::{Result, StorageError};
use crate::models::{Project, Session};

/// Base directory for all timeclock data.
///
/// Resolution order:
///   1. `TIMECLOCK_DATA_DIR` env var (used in tests and for custom overrides)
///   2. `$XDG_DATA_HOME/desktop-assistant/timeclock`
///   3. `~/.local/share/desktop-assistant/timeclock` (XDG default)
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TIMECLOCK_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let xdg_data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{home}/.local/share")
    });
    PathBuf::from(xdg_data_home)
        .join("desktop-assistant")
        .join("timeclock")
}

/// Path to the projects registry file.
fn projects_file() -> PathBuf {
    data_dir().join("_projects.jsonl")
}

/// Validate that a project_id is safe for use in file paths.
/// Only alphanumeric characters, hyphens, and underscores are allowed.
pub fn validate_project_id(project_id: &str) -> Result<()> {
    if project_id.is_empty() {
        return Err(StorageError::InvalidProjectId("project_id is empty".to_string()).into());
    }
    if !project_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(StorageError::InvalidProjectId(format!(
            "project_id contains invalid characters: {project_id}"
        ))
        .into());
    }
    Ok(())
}

/// Path to the JSONL file for a given project's sessions.
pub fn session_file(project_id: &str) -> PathBuf {
    data_dir().join(format!("{project_id}.jsonl"))
}

/// Ensure the data directory exists.
pub fn ensure_data_dir() -> Result<()> {
    let dir = data_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| StorageError::CreateDirError(dir.display().to_string(), e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

/// Read all known projects (last record per project_id wins).
pub fn read_projects() -> Result<Vec<Project>> {
    let path = projects_file();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path)
        .map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
    let mut map: HashMap<String, Project> = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line =
            line.map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Project>(line) {
            Ok(project) => {
                map.insert(project.project_id.clone(), project);
            }
            Err(e) => eprintln!(
                "timeclock-mcp: skipping corrupt line in {}: {e}",
                path.display()
            ),
        }
    }
    let mut projects: Vec<Project> = map.into_values().collect();
    projects.sort_by(|a, b| a.project_id.cmp(&b.project_id));
    Ok(projects)
}

/// Append (or replace) a project record. Last-write-wins by project_id.
pub fn upsert_project(project: &Project) -> Result<()> {
    ensure_data_dir()?;
    let path = projects_file();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    let line = serde_json::to_string(project)? + "\n";
    file.write_all(line.as_bytes())
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    Ok(())
}

/// Return true if a project with the given id exists in the registry.
pub fn project_exists(project_id: &str) -> Result<bool> {
    Ok(read_projects()?.iter().any(|p| p.project_id == project_id))
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// Read all sessions for a project (last record per session_id wins),
/// sorted by time_in ascending.
pub fn read_sessions(project_id: &str) -> Result<Vec<Session>> {
    validate_project_id(project_id)?;
    let path = session_file(project_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path)
        .map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
    let mut map: HashMap<String, Session> = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line =
            line.map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Session>(line) {
            Ok(session) => {
                map.insert(session.session_id.clone(), session);
            }
            Err(e) => eprintln!(
                "timeclock-mcp: skipping corrupt line in {}: {e}",
                path.display()
            ),
        }
    }
    let mut sessions: Vec<Session> = map.into_values().collect();
    sessions.sort_by(|a, b| a.time_in.cmp(&b.time_in));
    Ok(sessions)
}

/// Read sessions across all known projects (and any other *.jsonl files).
pub fn read_all_sessions() -> Result<Vec<Session>> {
    let dir = data_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut all = Vec::new();
    for entry in fs::read_dir(&dir)
        .map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?
    {
        let entry =
            entry.map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        // Skip metadata files (e.g. _projects.jsonl)
        if stem.starts_with('_') {
            continue;
        }
        let mut sessions = read_sessions(stem)?;
        all.append(&mut sessions);
    }
    all.sort_by(|a, b| a.time_in.cmp(&b.time_in));
    Ok(all)
}

/// Return the currently active (no time_out) session for a project, if any.
pub fn find_active_session(project_id: &str) -> Result<Option<Session>> {
    let sessions = read_sessions(project_id)?;
    Ok(sessions.into_iter().find(|s| s.time_out.is_none()))
}

/// Append a session record to the project's JSONL file.
pub fn append_session(session: &Session) -> Result<()> {
    validate_project_id(&session.project_id)?;
    ensure_data_dir()?;
    let path = session_file(&session.project_id);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    let line = serde_json::to_string(session)? + "\n";
    file.write_all(line.as_bytes())
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    Ok(())
}

/// Rewrite the project registry omitting the given project_id.
/// If the file does not exist this is a no-op.
///
/// Uses a write-to-tmp-then-rename pattern so a crash mid-write cannot truncate
/// the live registry file.
pub fn delete_project(project_id: &str) -> Result<()> {
    let path = projects_file();
    if !path.exists() {
        return Ok(());
    }
    let projects = read_projects()?;
    let filtered: Vec<&Project> = projects
        .iter()
        .filter(|p| p.project_id != project_id)
        .collect();
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| StorageError::WriteError(tmp.display().to_string(), e.to_string()))?;
        for p in filtered {
            let line = serde_json::to_string(p)? + "\n";
            file.write_all(line.as_bytes())
                .map_err(|e| StorageError::WriteError(tmp.display().to_string(), e.to_string()))?;
        }
        // file is flushed and closed here (drop)
    }
    fs::rename(&tmp, &path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    Ok(())
}

/// Delete the JSONL session file for a project (if it exists).
pub fn delete_project_sessions(project_id: &str) -> Result<()> {
    validate_project_id(project_id)?;
    let path = session_file(project_id);
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    }
    Ok(())
}

/// Rewrite a project's session JSONL with only the provided sessions.
///
/// Uses a write-to-tmp-then-rename pattern so a crash mid-write cannot truncate
/// the live session file.
pub fn rewrite_sessions(project_id: &str, sessions: &[Session]) -> Result<()> {
    validate_project_id(project_id)?;
    ensure_data_dir()?;
    let path = session_file(project_id);
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| StorageError::WriteError(tmp.display().to_string(), e.to_string()))?;
        for s in sessions {
            let line = serde_json::to_string(s)? + "\n";
            file.write_all(line.as_bytes())
                .map_err(|e| StorageError::WriteError(tmp.display().to_string(), e.to_string()))?;
        }
        // file is flushed and closed here (drop)
    }
    fs::rename(&tmp, &path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    Ok(())
}

/// Delete a session by session_id across all projects.
/// Returns `StorageError::SessionNotFound` if not found.
pub fn delete_session_by_id(session_id: &str) -> Result<()> {
    let dir = data_dir();
    if dir.exists() {
        for entry in fs::read_dir(&dir)
            .map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?
        {
            let entry = entry
                .map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.starts_with('_') {
                continue;
            }
            let sessions = read_sessions(stem)?;
            if sessions.iter().any(|s| s.session_id == session_id) {
                let remaining: Vec<Session> = sessions
                    .into_iter()
                    .filter(|s| s.session_id != session_id)
                    .collect();
                rewrite_sessions(stem, &remaining)?;
                return Ok(());
            }
        }
    }
    Err(StorageError::SessionNotFound(session_id.to_string()).into())
}

/// Look up a session by session_id across all projects.
/// Returns (project_id, session) if found.
pub fn find_session_by_id(session_id: &str) -> Result<Option<(String, Session)>> {
    let dir = data_dir();
    if !dir.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(&dir)
        .map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?
    {
        let entry =
            entry.map_err(|e| StorageError::ReadError(dir.display().to_string(), e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem.starts_with('_') {
            continue;
        }
        let sessions = read_sessions(stem)?;
        for s in sessions {
            if s.session_id == session_id {
                return Ok(Some((stem.to_string(), s)));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestEnv, counter_total};
    use mcp_core::telemetry::metrics::Label;
    use tracing::Level;

    /// Stands in for the operator's home directory: the path segment that
    /// must never reach a WARN-or-louder field or event when a storage line
    /// fails to parse (mcp-core#40 D10). `TestEnv::with_path_segment` nests
    /// the data directory under this, so the full file path a corrupt-line
    /// error touches always contains it.
    const SENTINEL: &str = "MARKER-timeclock-storage-home-9c41f2";

    /// AC (mcp-core#40, per-server checklist): a corrupt line in the
    /// projects registry is logged as a WARN carrying a bounded reason and
    /// no path, a DEBUG carrying the full path, and a bounded-reason
    /// storage-failure metric -- never an `eprintln!`.
    #[test]
    fn corrupt_project_registry_line_warn_omits_path_but_debug_keeps_it() {
        let _env = TestEnv::with_path_segment(SENTINEL);
        ensure_data_dir().expect("ensure_data_dir must succeed under a fresh TestEnv");
        std::fs::write(projects_file(), "not valid json\n")
            .expect("seed a corrupt projects registry line");

        let reason_labels = [Label::new("reason", "corrupt_line")];
        let before = counter_total("timeclock.storage_failures", &reason_labels);

        let recorded = crate::test_helpers::capture(|| {
            let result = read_projects();
            assert!(
                result.is_ok(),
                "a corrupt line must be skipped, not fail the whole read: {result:?}"
            );
            assert!(
                result.unwrap().is_empty(),
                "the corrupt line must not appear as a parsed project"
            );
        });

        assert!(
            recorded
                .spans
                .iter()
                .any(|s| s.name == "timeclock.storage.read_projects"),
            "read_projects must open a timeclock.storage.read_projects span; spans were {:?}",
            recorded.span_summary()
        );

        let warn = recorded
            .events
            .iter()
            .find(|e| e.level == Level::WARN)
            .unwrap_or_else(|| {
                panic!(
                    "a corrupt line must log a WARN; events were {:?}",
                    recorded.event_summary()
                )
            });
        assert_eq!(
            warn.fields.get("store").map(String::as_str),
            Some("projects"),
            "the WARN must name which store was affected: {:?}",
            warn.fields
        );
        assert_eq!(
            warn.fields.get("reason").map(String::as_str),
            Some("corrupt_line"),
            "the WARN must carry a bounded reason: {:?}",
            warn.fields
        );
        assert!(
            !warn.fields.contains_key("path"),
            "a full path must not reach the WARN field set at all: {:?}",
            warn.fields
        );
        for value in warn.fields.values() {
            assert!(
                !value.contains(SENTINEL),
                "the path's sentinel segment reached a WARN field: {value:?}"
            );
        }

        for event in &recorded.events {
            // DEBUG/TRACE may legitimately carry the path (D10) -- that is
            // the point of the split. Only INFO and louder are checked.
            if event.level > Level::INFO {
                continue;
            }
            for (key, value) in &event.fields {
                assert!(
                    !value.contains(SENTINEL),
                    "the sentinel path leaked into an INFO-or-louder event field {key:?}: \
                     {value:?}; all events were {:?}",
                    recorded.event_summary()
                );
            }
        }

        let at_debug = recorded.events.iter().any(|event| {
            event.level == Level::DEBUG
                && event.fields.values().any(|v| v.contains(SENTINEL))
        });
        assert!(
            at_debug,
            "the full path must still be available at DEBUG, or the level contract has \
             nothing to hold back; events were {:?}",
            recorded.event_summary()
        );

        assert_eq!(
            counter_total("timeclock.storage_failures", &reason_labels),
            before + 1,
            "a corrupt line must increment timeclock.storage_failures, labelled \
             reason=corrupt_line"
        );
    }

    /// Same criterion as the projects-registry test above, applied to a
    /// project's session file (`storage.rs:147`'s print site).
    #[test]
    fn corrupt_session_line_warn_omits_path_but_debug_keeps_it() {
        let _env = TestEnv::with_path_segment(SENTINEL);
        ensure_data_dir().expect("ensure_data_dir must succeed under a fresh TestEnv");
        std::fs::write(session_file("acme"), "not valid json\n")
            .expect("seed a corrupt session line");

        let reason_labels = [Label::new("reason", "corrupt_line")];
        let before = counter_total("timeclock.storage_failures", &reason_labels);

        let recorded = crate::test_helpers::capture(|| {
            let result = read_sessions("acme");
            assert!(
                result.is_ok(),
                "a corrupt line must be skipped, not fail the whole read: {result:?}"
            );
            assert!(
                result.unwrap().is_empty(),
                "the corrupt line must not appear as a parsed session"
            );
        });

        assert!(
            recorded
                .spans
                .iter()
                .any(|s| s.name == "timeclock.storage.read_sessions"),
            "read_sessions must open a timeclock.storage.read_sessions span; spans were {:?}",
            recorded.span_summary()
        );

        let warn = recorded
            .events
            .iter()
            .find(|e| e.level == Level::WARN)
            .unwrap_or_else(|| {
                panic!(
                    "a corrupt line must log a WARN; events were {:?}",
                    recorded.event_summary()
                )
            });
        assert_eq!(
            warn.fields.get("store").map(String::as_str),
            Some("sessions"),
            "the WARN must name which store was affected: {:?}",
            warn.fields
        );
        assert_eq!(
            warn.fields.get("reason").map(String::as_str),
            Some("corrupt_line"),
            "the WARN must carry a bounded reason: {:?}",
            warn.fields
        );
        assert!(
            !warn.fields.contains_key("path"),
            "a full path must not reach the WARN field set at all: {:?}",
            warn.fields
        );
        for value in warn.fields.values() {
            assert!(
                !value.contains(SENTINEL),
                "the path's sentinel segment reached a WARN field: {value:?}"
            );
        }

        for event in &recorded.events {
            if event.level > Level::INFO {
                continue;
            }
            for (key, value) in &event.fields {
                assert!(
                    !value.contains(SENTINEL),
                    "the sentinel path leaked into an INFO-or-louder event field {key:?}: \
                     {value:?}; all events were {:?}",
                    recorded.event_summary()
                );
            }
        }

        let at_debug = recorded.events.iter().any(|event| {
            event.level == Level::DEBUG
                && event.fields.values().any(|v| v.contains(SENTINEL))
        });
        assert!(
            at_debug,
            "the full path must still be available at DEBUG, or the level contract has \
             nothing to hold back; events were {:?}",
            recorded.event_summary()
        );

        assert_eq!(
            counter_total("timeclock.storage_failures", &reason_labels),
            before + 1,
            "a corrupt line must increment timeclock.storage_failures, labelled \
             reason=corrupt_line"
        );
    }
}
