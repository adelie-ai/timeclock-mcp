# Security Audit — timeclock-mcp

**Date:** 2026-03-31
**Scope:** Time tracking MCP server

---

## High Severity

### 1. Path Traversal via project_id

**File:** `src/storage.rs:36-38`

```rust
pub fn session_file(project_id: &str) -> PathBuf {
    data_dir().join(format!("{project_id}.jsonl"))
}
```

`project_id` is used directly in path construction. A value like `../../tmp/exploit` writes outside the data directory.

**Recommendation:** Validate that `project_id` contains only `[a-zA-Z0-9_-]` characters, or canonicalize and verify the path stays within `data_dir()`.

---

### 2. Unbounded Memory Allocation from Content-Length

**File:** `src/transport.rs` (same pattern as tasks-mcp)

No upper bound on Content-Length header before buffer allocation.

**Recommendation:** Add maximum Content-Length check.

---

## Medium Severity

### 3. Unbounded CSV Output

**File:** `src/operations/session_query.rs:74-89`

CSV rendering builds entire result in memory. Millions of sessions could exhaust memory.

**Recommendation:** Add pagination or streaming output.

---

### 4. No Time Validation in Corrections

Session corrections don't validate `time_out >= time_in`, potentially creating invalid records.

**Recommendation:** Add validation.

---

## Positive Findings

- JSONL storage format (append-only, simple)
- No shell command execution
- No `unsafe` code
