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

/// Path to the JSONL file for a given project's sessions.
pub fn session_file(project_id: &str) -> PathBuf {
    data_dir().join(format!("{project_id}.jsonl"))
}

/// Ensure the data directory exists.
pub fn ensure_data_dir() -> Result<()> {
    let dir = data_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        StorageError::CreateDirError(dir.display().to_string(), e.to_string())
    })?;
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
        let line = line
            .map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(project) = serde_json::from_str::<Project>(line) {
            map.insert(project.project_id.clone(), project);
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
    let path = session_file(project_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&path)
        .map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
    let mut map: HashMap<String, Session> = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line
            .map_err(|e| StorageError::ReadError(path.display().to_string(), e.to_string()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(session) = serde_json::from_str::<Session>(line) {
            map.insert(session.session_id.clone(), session);
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
    let mut file = fs::File::create(&path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    for p in filtered {
        let line = serde_json::to_string(p)? + "\n";
        file.write_all(line.as_bytes())
            .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    }
    Ok(())
}

/// Delete the JSONL session file for a project (if it exists).
pub fn delete_project_sessions(project_id: &str) -> Result<()> {
    let path = session_file(project_id);
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    }
    Ok(())
}

/// Rewrite a project's session JSONL with only the provided sessions.
pub fn rewrite_sessions(project_id: &str, sessions: &[Session]) -> Result<()> {
    ensure_data_dir()?;
    let path = session_file(project_id);
    let mut file = fs::File::create(&path)
        .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    for s in sessions {
        let line = serde_json::to_string(s)? + "\n";
        file.write_all(line.as_bytes())
            .map_err(|e| StorageError::WriteError(path.display().to_string(), e.to_string()))?;
    }
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
        for s in sessions {
            if s.session_id == session_id {
                return Ok(Some((stem.to_string(), s)));
            }
        }
    }
    Ok(None)
}
