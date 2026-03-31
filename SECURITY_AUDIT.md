# Security Audit — timeclock-mcp

**Date:** 2026-03-31
**Scope:** Time tracking MCP server

---

## Medium Severity

### 1. No Time Validation in Corrections (MEDIUM)

Session corrections don't validate `time_out >= time_in`, potentially creating invalid records.

**Recommendation:** Add validation.

---

### 2. Unbounded CSV Output (MEDIUM)

**File:** `src/operations/session_query.rs:74-89`

CSV rendering builds entire result in memory. Millions of sessions could exhaust memory.

**Recommendation:** Add pagination or streaming output.

---

## Positive Findings

- JSONL storage format (append-only, simple)
- No shell command execution
- No `unsafe` code
- Project IDs validated before file path construction
